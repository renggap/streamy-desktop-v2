use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use parking_lot::Mutex;
use streamy_core::core::teardown_state::TransportState;
use streamy_core::net::client::SourceClientError;
use streamy_core::net::transport::StreamTransport;

#[tokio::test]
async fn test_reported_backoff_equals_slept_delay() {
    let observed_states = Arc::new(Mutex::new(Vec::new()));
    let sleep_log = Arc::new(Mutex::new(Vec::new()));

    let obs_clone = observed_states.clone();
    let sleep_clone = sleep_log.clone();

    let transport = StreamTransport::new(
        Arc::new(|| Err(SourceClientError::Io(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused")))),
        Arc::new(|_| Ok(())),
        Arc::new(move |state| {
            obs_clone.lock().push(state);
        }),
        Some(Arc::new(move |attempt| {
            let ms = (attempt as u64 * 10).min(50);
            sleep_clone.lock().push(ms);
            ms
        })),
    );

    transport.start();

    // Wait until at least one backoff occurred
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if !sleep_log.lock().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    transport.stop();

    let states = observed_states.lock().clone();
    let backoff_states: Vec<_> = states
        .iter()
        .filter_map(|s| match s {
            TransportState::Backoff { ms, .. } => Some(*ms),
            _ => None,
        })
        .collect();

    assert!(!backoff_states.is_empty());
    let sleeps = sleep_log.lock().clone();
    assert_eq!(backoff_states.last(), sleeps.last());
}

#[tokio::test]
async fn test_reconnect_emits_connecting_then_backoff() {
    let observed_states = Arc::new(Mutex::new(Vec::new()));
    let obs_clone = observed_states.clone();

    let transport = StreamTransport::new(
        Arc::new(|| Err(SourceClientError::Io(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused")))),
        Arc::new(|_| Ok(())),
        Arc::new(move |state| {
            obs_clone.lock().push(state);
        }),
        Some(Arc::new(|_| 10)),
    );

    transport.start();

    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        let has_backoff = observed_states
            .lock()
            .iter()
            .any(|s| matches!(s, TransportState::Backoff { attempt: 1, .. }));
        if has_backoff {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    transport.stop();

    let states = observed_states.lock().clone();
    assert!(states.contains(&TransportState::Connecting));
    assert!(states
        .iter()
        .any(|s| matches!(s, TransportState::Backoff { attempt: 1, .. })));
}

#[tokio::test]
async fn test_transport_gives_up_after_max_attempts_and_emits_failed() {
    let attempts = Arc::new(AtomicU32::new(0));
    let attempts_clone = attempts.clone();

    let observed_states = Arc::new(Mutex::new(Vec::new()));
    let obs_clone = observed_states.clone();

    let transport = StreamTransport::new(
        Arc::new(move || {
            attempts_clone.fetch_add(1, Ordering::SeqCst);
            Err(SourceClientError::Io(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused")))
        }),
        Arc::new(|_| Ok(())),
        Arc::new(move |state| {
            obs_clone.lock().push(state);
        }),
        Some(Arc::new(|_| 5)), // fast backoff: 5ms
    );

    transport.start();

    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        let failed = observed_states
            .lock()
            .iter()
            .any(|s| matches!(s, TransportState::Failed { .. }));
        if failed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    transport.stop();

    assert_eq!(attempts.load(Ordering::SeqCst), 10);
    let states = observed_states.lock().clone();
    let backoff_count = states
        .iter()
        .filter(|s| matches!(s, TransportState::Backoff { .. }))
        .count();
    assert_eq!(backoff_count, 9);
    assert_eq!(
        states.last(),
        Some(&TransportState::Failed {
            reason: "gave_up after 10 attempts".to_string()
        })
    );
}

#[tokio::test]
async fn test_stop_during_backoff_ends_loop_immediately() {
    let attempts = Arc::new(AtomicU32::new(0));
    let attempts_clone = attempts.clone();

    let observed_states = Arc::new(Mutex::new(Vec::new()));
    let obs_clone = observed_states.clone();

    let transport = StreamTransport::new(
        Arc::new(move || {
            attempts_clone.fetch_add(1, Ordering::SeqCst);
            Err(SourceClientError::Io(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused")))
        }),
        Arc::new(|_| Ok(())),
        Arc::new(move |state| {
            obs_clone.lock().push(state);
        }),
        Some(Arc::new(|_| 60_000)), // 60s backoff — would hang if cancel didn't work
    );

    transport.start();

    // Wait until Backoff is emitted
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if observed_states
            .lock()
            .iter()
            .any(|s| matches!(s, TransportState::Backoff { .. }))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let stop_start = std::time::Instant::now();
    transport.stop();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Must have exited immediately (< 1s)
    assert!(stop_start.elapsed() < Duration::from_secs(1));
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_enqueue_after_stop_is_suppressed_not_backpressure() {
    let observed_states = Arc::new(Mutex::new(Vec::new()));
    let obs_clone = observed_states.clone();

    let transport = StreamTransport::new(
        Arc::new(|| Err(SourceClientError::Io(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused")))),
        Arc::new(|_| Ok(())),
        Arc::new(move |state| {
            obs_clone.lock().push(state);
        }),
        Some(Arc::new(|_| 5)),
    );

    transport.start();

    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if observed_states
            .lock()
            .iter()
            .any(|s| matches!(s, TransportState::Backoff { .. }))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    transport.stop();
    let drops_before = transport.dropped_frames();

    // Enqueue 5 frames after stop
    for _ in 0..5 {
        transport.enqueue(vec![0u8; 16]);
    }

    tokio::time::sleep(Duration::from_millis(50)).await;

    let states = observed_states.lock().clone();
    assert!(!states.iter().any(|s| matches!(s, TransportState::Backpressure { .. })));
    assert_eq!(transport.dropped_frames(), drops_before);
}

#[tokio::test]
async fn test_no_terminal_state_after_stop() {
    let observed_states = Arc::new(Mutex::new(Vec::new()));
    let obs_clone = observed_states.clone();

    let connect_fail = Arc::new(AtomicBool::new(true));
    let fail_clone = connect_fail.clone();

    let transport = StreamTransport::new(
        Arc::new(move || {
            if fail_clone.load(Ordering::SeqCst) {
                Err(SourceClientError::Io(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused")))
            } else {
                Ok(())
            }
        }),
        Arc::new(|_| Ok(())),
        Arc::new(move |state| {
            obs_clone.lock().push(state);
        }),
        Some(Arc::new(|_| 5)),
    );

    transport.start();

    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if observed_states
            .lock()
            .iter()
            .any(|s| matches!(s, TransportState::Backoff { .. }))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    transport.stop();
    connect_fail.store(false, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(100)).await;

    let states = observed_states.lock().clone();
    assert!(!states.iter().any(|s| matches!(s, TransportState::Live)));
}

#[tokio::test]
async fn test_protocol_exception_fails_fast_without_retries() {
    let attempts = Arc::new(AtomicU32::new(0));
    let attempts_clone = attempts.clone();

    let observed_states = Arc::new(Mutex::new(Vec::new()));
    let obs_clone = observed_states.clone();

    let transport = StreamTransport::new(
        Arc::new(move || {
            attempts_clone.fetch_add(1, Ordering::SeqCst);
            Err(SourceClientError::Protocol("auth NAK: Bad password".to_string()))
        }),
        Arc::new(|_| Ok(())),
        Arc::new(move |state| {
            obs_clone.lock().push(state);
        }),
        Some(Arc::new(|_| 5)),
    );

    transport.start();

    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if observed_states
            .lock()
            .iter()
            .any(|s| matches!(s, TransportState::Failed { .. }))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    transport.stop();

    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    let states = observed_states.lock().clone();
    assert!(matches!(states.last(), Some(TransportState::Failed { .. })));
}

/// Regression: start() is called from Tauri sync commands, which run on plain
/// threads with no Tokio reactor. tokio::spawn there panicked with
/// "there is no reactor running" (crash.log transport.rs:96). The worker now
/// owns a dedicated thread + runtime, so start() must work reactor-free.
#[test]
fn test_start_works_outside_tokio_context() {
    let started = Arc::new(AtomicBool::new(false));
    let started_clone = started.clone();

    let transport = Arc::new(StreamTransport::new(
        Arc::new(move || {
            started_clone.store(true, Ordering::SeqCst);
            // Hold the connection "live" so the worker sits in its idle loop.
            Ok(())
        }),
        Arc::new(|_| Ok(())),
        Arc::new(|_| {}),
        Some(Arc::new(|_| 10)),
    ));

    // Plain thread: no Tokio runtime in scope, exactly like a Tauri sync command.
    let t = {
        let transport = transport.clone();
        std::thread::spawn(move || {
            transport.start();
        })
    };
    t.join().expect("start() must not panic outside Tokio");

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !started.load(Ordering::SeqCst) {
        assert!(std::time::Instant::now() < deadline, "worker never connected");
        std::thread::sleep(Duration::from_millis(5));
    }

    // Session (Arc holder) still alive: worker must still be running.
    transport.stop();
}
