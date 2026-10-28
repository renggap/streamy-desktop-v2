//! StreamTransport: dedicated worker thread, bounded drop-newest channel, auto-reconnect backoff ladder.
//! Pinned to match Android `StreamTransport.kt` and `StreamTransportBackoffTest.kt`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use parking_lot::Mutex;
use tokio::sync::{mpsc, Notify};

use crate::core::backoff::{next_backoff_ms, CAPACITY, MAX_ATTEMPTS};
use crate::core::teardown_state::TransportState;
use crate::net::client::SourceClientError;

pub type ConnectFn = Arc<dyn Fn() -> Result<(), SourceClientError> + Send + Sync>;
pub type SendFn = Arc<dyn Fn(&[u8]) -> Result<(), SourceClientError> + Send + Sync>;
pub type StateFn = Arc<dyn Fn(TransportState) + Send + Sync>;
pub type BackoffFn = Arc<dyn Fn(u32) -> u64 + Send + Sync>;
pub type ExitFn = Box<dyn FnOnce() + Send>;

/// How hard the worker tries to (re)establish the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectPolicy {
    /// When false, a dropped connection or failed first connect is terminal.
    pub auto_reconnect: bool,
    /// Attempt budget before the stream has ever gone live (bad host/port fails fast).
    pub initial_max_attempts: u32,
    /// Attempt budget after a live connection drops.
    pub max_attempts: u32,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            auto_reconnect: true,
            initial_max_attempts: MAX_ATTEMPTS,
            max_attempts: MAX_ATTEMPTS,
        }
    }
}

pub struct StreamTransport {
    connect: ConnectFn,
    send: SendFn,
    on_state: StateFn,
    backoff_fn: BackoffFn,
    policy: Mutex<ReconnectPolicy>,
    on_exit: Mutex<Option<ExitFn>>,

    queue_tx: mpsc::Sender<Vec<u8>>,
    queue_rx: Mutex<Option<mpsc::Receiver<Vec<u8>>>>,

    drops: Arc<AtomicU64>,
    bytes_sent: Arc<AtomicU64>,
    stopped: Arc<AtomicBool>,
    stop_notify: Arc<Notify>,
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
            policy: Mutex::new(ReconnectPolicy::default()),
            on_exit: Mutex::new(None),
            queue_tx: tx,
            queue_rx: Mutex::new(Some(rx)),
            drops: Arc::new(AtomicU64::new(0)),
            bytes_sent: Arc::new(AtomicU64::new(0)),
            stopped: Arc::new(AtomicBool::new(false)),
            stop_notify: Arc::new(Notify::new()),
        }
    }

    /// Must be called before `start()`.
    pub fn set_policy(&self, policy: ReconnectPolicy) {
        *self.policy.lock() = policy;
    }

    /// Runs once on the worker thread after the loop ends (stop, give-up or failure).
    /// Must be called before `start()`.
    pub fn set_on_exit(&self, f: ExitFn) {
        *self.on_exit.lock() = Some(f);
    }

    pub fn dropped_frames(&self) -> u64 {
        self.drops.load(Ordering::SeqCst)
    }

    /// Total payload bytes successfully written to the server, across reconnects.
    pub fn bytes_sent(&self) -> u64 {
        self.bytes_sent.load(Ordering::SeqCst)
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
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

    /// Spawn the worker. Idempotent: a second call is a no-op.
    pub fn start(&self) {
        let Some(mut rx) = self.queue_rx.lock().take() else {
            return;
        };
        let stopped = self.stopped.clone();
        let stop_notify = self.stop_notify.clone();
        let on_state = self.on_state.clone();
        let connect = self.connect.clone();
        let send = self.send.clone();
        let backoff_fn = self.backoff_fn.clone();
        let bytes_sent = self.bytes_sent.clone();
        let policy = *self.policy.lock();
        let on_exit = self.on_exit.lock().take();

        // Dedicated thread + current-thread runtime: start() is called from
        // Tauri commands (plain threads, no ambient Tokio context), where
        // tokio::spawn panics with "there is no reactor running".
        std::thread::Builder::new()
            .name("streamy-transport".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("transport runtime");
                rt.block_on(async move {
                    let mut attempt: u32 = 0;
                    let mut ever_live = false;

                    while !stopped.load(Ordering::SeqCst) {
                        on_state(if attempt == 0 {
                            TransportState::Connecting
                        } else {
                            TransportState::Reconnecting { attempt }
                        });

                        // Run connect() off the async executor: it does blocking DNS + TCP + handshake.
                        let conn_res = {
                            let connect = connect.clone();
                            tokio::task::spawn_blocking(move || connect())
                                .await
                                .unwrap_or_else(|e| {
                                    Err(SourceClientError::Protocol(format!("connect task failed: {e}")))
                                })
                        };

                        if stopped.load(Ordering::SeqCst) {
                            break;
                        }

                        let err = match conn_res {
                            Ok(()) => {
                                // Clear pre-reconnect backlog so live speech resumes live
                                while rx.try_recv().is_ok() {}

                                on_state(TransportState::Live);
                                ever_live = true;

                                let send_err = loop {
                                    let frame = tokio::select! {
                                        biased;
                                        _ = stop_notify.notified() => None,
                                        f = rx.recv() => f,
                                    };
                                    let Some(f) = frame else { break None };
                                    if stopped.load(Ordering::SeqCst) {
                                        break None;
                                    }
                                    match send(&f) {
                                        Ok(()) => {
                                            bytes_sent.fetch_add(f.len() as u64, Ordering::SeqCst);
                                        }
                                        Err(e) => break Some(e),
                                    }
                                };

                                match send_err {
                                    Some(e) if !stopped.load(Ordering::SeqCst) => {
                                        tracing::warn!("stream connection lost: {e}");
                                        if !policy.auto_reconnect {
                                            on_state(TransportState::Failed {
                                                reason: format!("connection lost: {e}"),
                                            });
                                            break;
                                        }
                                        // Reconnect straight away; failures fall into the ladder below.
                                        attempt = 1;
                                        continue;
                                    }
                                    _ => break,
                                }
                            }
                            Err(e) => e,
                        };

                        tracing::warn!("stream connect attempt failed: {err}");

                        // A rejection before we were ever live is a config problem (bad
                        // password, wrong SID). After a drop the server may still hold the
                        // old source slot for a moment, so keep retrying.
                        if (err.is_non_retryable() && !ever_live) || !policy.auto_reconnect {
                            on_state(TransportState::Failed {
                                reason: err.to_string(),
                            });
                            break;
                        }

                        attempt += 1;
                        let budget = if ever_live {
                            policy.max_attempts
                        } else {
                            policy.initial_max_attempts
                        };
                        if attempt >= budget {
                            on_state(TransportState::Failed {
                                reason: format!("gave_up after {attempt} attempts: {err}"),
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
                            _ = stop_notify.notified() => break,
                        }
                    }
                });
                if let Some(f) = on_exit {
                    f();
                }
            })
            .expect("spawn transport thread");
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        // notify_one stores a permit when the worker isn't parked yet, so a stop
        // racing the worker's next await is never lost.
        self.stop_notify.notify_one();
    }
}

impl Drop for StreamTransport {
    fn drop(&mut self) {
        self.stop();
    }
}
