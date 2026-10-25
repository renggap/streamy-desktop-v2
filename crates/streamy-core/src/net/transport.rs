//! StreamTransport: dedicated worker task, bounded drop-newest channel, auto-reconnect backoff ladder.
//! Pinned to match Android `StreamTransport.kt` and `StreamTransportBackoffTest.kt`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinHandle;

use crate::core::backoff::{next_backoff_ms, should_give_up, CAPACITY};
use crate::core::teardown_state::TransportState;
use crate::net::client::SourceClientError;

pub type ConnectFn = Arc<dyn Fn() -> Result<(), SourceClientError> + Send + Sync>;
pub type SendFn = Arc<dyn Fn(&[u8]) -> Result<(), SourceClientError> + Send + Sync>;
pub type StateFn = Arc<dyn Fn(TransportState) + Send + Sync>;
pub type BackoffFn = Arc<dyn Fn(u32) -> u64 + Send + Sync>;

pub struct StreamTransport {
    connect: ConnectFn,
    send: SendFn,
    on_state: StateFn,
    backoff_fn: BackoffFn,

    queue_tx: mpsc::Sender<Vec<u8>>,
    queue_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Vec<u8>>>>,

    drops: Arc<AtomicU64>,
    stopped: Arc<AtomicBool>,
    stop_notify: Arc<Notify>,
    worker_handle: Arc<parking_lot::Mutex<Option<JoinHandle<()>>>>,
}

impl StreamTransport {
    pub fn new(
        connect: ConnectFn,
        send: SendFn,
        on_state: StateFn,
        backoff_fn: Option<BackoffFn>,
    ) -> Self {
        let (tx, rx) = mpsc::channel(CAPACITY);
        let backoff = backoff_fn.unwrap_or_else(|| {
            Arc::new(|attempt| {
                let mut rng = rand::rng();
                next_backoff_ms(attempt, &mut rng)
            })
        });

        Self {
            connect,
            send,
            on_state,
            backoff_fn: backoff,
            queue_tx: tx,
            queue_rx: Arc::new(tokio::sync::Mutex::new(rx)),
            drops: Arc::new(AtomicU64::new(0)),
            stopped: Arc::new(AtomicBool::new(false)),
            stop_notify: Arc::new(Notify::new()),
            worker_handle: Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    pub fn dropped_frames(&self) -> u64 {
        self.drops.load(Ordering::SeqCst)
    }

    /// Non-blocking enqueue. If queue is full, DROP THE NEW frame (drop-newest).
    /// After stop() the queue is closed and returns silently without emitting phantom Backpressure.
    pub fn enqueue(&self, frame: Vec<u8>) {
        if self.stopped.load(Ordering::SeqCst) {
            return;
        }

        match self.queue_tx.try_send(frame) {
            Ok(_) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                // Drop newest: increment drop count and report backpressure
                let current_drops = self.drops.fetch_add(1, Ordering::SeqCst) + 1;
                (self.on_state)(TransportState::Backpressure { drops: current_drops });
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // Channel closed (stopped) -> silent: do not bump drops, do not emit Backpressure
            }
        }
    }

    pub fn start(&self) {
        let stopped = self.stopped.clone();
        let stop_notify = self.stop_notify.clone();
        let on_state = self.on_state.clone();
        let connect = self.connect.clone();
        let send = self.send.clone();
        let backoff_fn = self.backoff_fn.clone();
        let rx_mutex = self.queue_rx.clone();

        let handle = tokio::spawn(async move {
            let mut attempt = 0;
            while !stopped.load(Ordering::SeqCst) {
                let current_state = if attempt == 0 {
                    TransportState::Connecting
                } else {
                    TransportState::Reconnecting { attempt }
                };
                on_state(current_state);

                // Run connect()
                let conn_res = connect();

                if stopped.load(Ordering::SeqCst) {
                    break;
                }

                match conn_res {
                    Ok(_) => {
                        // Clear pre-reconnect backlog so live speech resumes live
                        {
                            let mut rx = rx_mutex.lock().await;
                            while let Ok(_) = rx.try_recv() {
                                // drain
                            }
                        }

                        on_state(TransportState::Live);
                        attempt = 0;

                        // Streaming loop
                        loop {
                            if stopped.load(Ordering::SeqCst) {
                                break;
                            }

                            let frame = {
                                let mut rx = rx_mutex.lock().await;
                                tokio::select! {
                                    f = rx.recv() => f,
                                    _ = stop_notify.notified() => None,
                                }
                            };

                            match frame {
                                Some(f) => {
                                    if stopped.load(Ordering::SeqCst) {
                                        break;
                                    }
                                    if let Err(_err) = send(&f) {
                                        // Socket write failed -> trigger reconnect
                                        break;
                                    }
                                }
                                None => {
                                    // Channel closed or stopped
                                    break;
                                }
                            }
                        }

                        if stopped.load(Ordering::SeqCst) {
                            break;
                        }
                    }
                    Err(e) => {
                        if stopped.load(Ordering::SeqCst) {
                            break;
                        }

                        if e.is_non_retryable() {
                            on_state(TransportState::Failed {
                                reason: e.to_string(),
                            });
                            break;
                        }

                        attempt += 1;
                        if should_give_up(attempt) {
                            on_state(TransportState::Failed {
                                reason: format!("gave_up after {} attempts", attempt),
                            });
                            break;
                        }

                        let backoff = backoff_fn(attempt);
                        on_state(TransportState::Backoff {
                            attempt,
                            ms: backoff,
                        });

                        // Cancellable sleep: wakes immediately when stop() is called
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_millis(backoff)) => {},
                            _ = stop_notify.notified() => {
                                break;
                            }
                        }
                    }
                }
            }
        });

        *self.worker_handle.lock() = Some(handle);
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.stop_notify.notify_waiters();
        if let Some(h) = self.worker_handle.lock().take() {
            h.abort();
        }
    }
}

impl Drop for StreamTransport {
    fn drop(&mut self) {
        self.stop();
    }
}
