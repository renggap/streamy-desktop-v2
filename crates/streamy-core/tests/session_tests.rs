use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use parking_lot::Mutex;
use tempfile::NamedTempFile;

use streamy_core::audio::source::SineAudioSource;
use streamy_core::core::audio_config_rules::{AudioConfig, AudioConfigRules, BroadcastMode, Codec, DjProtocol};
use streamy_core::core::framing::message;
use streamy_core::core::teardown_state::{RecordState, StreamState};
use streamy_core::session::record_session::RecordSession;
use streamy_core::session::stream_session::{StreamProfile, StreamSession};

fn run_mock_uvox_server(port: u16, server_running: Arc<AtomicBool>, connect_count: Arc<AtomicU32>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let listener = match TcpListener::bind(format!("127.0.0.1:{}", port)) {
            Ok(l) => l,
            Err(_) => {
                let mut bound = None;
                for _ in 0..20 {
                    thread::sleep(Duration::from_millis(100));
                    if let Ok(l) = TcpListener::bind(format!("127.0.0.1:{}", port)) {
                        bound = Some(l);
                        break;
                    }
                }
                bound.expect("bind failed after retries")
            }
        };
        listener.set_nonblocking(true).unwrap();

        while server_running.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    connect_count.fetch_add(1, Ordering::SeqCst);
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));

                    // Handle handshake
                    let send_resp = |s: &mut TcpStream, cls: u16, t: u16, p: &[u8]| {
                        let msg = message(cls, t, p);
                        let _ = s.write_all(&msg);
                        let _ = s.flush();
                    };

                    let read_msg = |s: &mut TcpStream| -> bool {
                        let mut hdr = [0u8; 6];
                        if s.read_exact(&mut hdr).is_err() { return false; }
                        let len = ((hdr[4] as usize) << 8) | (hdr[5] as usize);
                        let mut p = vec![0u8; len];
                        if s.read_exact(&mut p).is_err() { return false; }
                        let mut tr = [0u8; 1];
                        if s.read_exact(&mut tr).is_err() { return false; }
                        true
                    };

                    // Handshake steps
                    if read_msg(&mut stream) { send_resp(&mut stream, 1, 0x009, b"ACK:foobar\0"); }
                    if read_msg(&mut stream) { send_resp(&mut stream, 1, 0x001, b"ACK\0"); }
                    if read_msg(&mut stream) { send_resp(&mut stream, 1, 0x040, b"ACK\0"); }
                    if read_msg(&mut stream) { send_resp(&mut stream, 1, 0x002, b"ACK\0"); }
                    if read_msg(&mut stream) { send_resp(&mut stream, 1, 0x003, b"ACK\0"); }
                    if read_msg(&mut stream) { send_resp(&mut stream, 1, 0x100, b"ACK\0"); }
                    if read_msg(&mut stream) { send_resp(&mut stream, 1, 0x101, b"ACK\0"); }
                    if read_msg(&mut stream) { send_resp(&mut stream, 1, 0x102, b"ACK\0"); }
                    if read_msg(&mut stream) { send_resp(&mut stream, 1, 0x103, b"ACK\0"); }
                    if read_msg(&mut stream) { send_resp(&mut stream, 1, 0x008, b"ACK\0"); }
                    if read_msg(&mut stream) { send_resp(&mut stream, 1, 0x004, b"ACK:Data transfer mode\0"); }

                    // Loop reading data frames until connection dropped or server stopped
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                    while server_running.load(Ordering::SeqCst) {
                        let mut buf = [0u8; 1024];
                        match stream.read(&mut buf) {
                            Ok(0) => break,
                            Ok(_) => {},
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                                continue;
                            }
                            Err(_) => break,
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    })
}

#[tokio::test]
async fn test_automated_kill_server_backoff_restart_resume() {
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").expect("get ephemeral port");
        l.local_addr().unwrap().port()
    };
    let server_running = Arc::new(AtomicBool::new(true));
    let connect_count = Arc::new(AtomicU32::new(0));

    let handle = run_mock_uvox_server(port, server_running.clone(), connect_count.clone());

    let observed = Arc::new(Mutex::new(Vec::new()));
    let obs_clone = observed.clone();

    let session = StreamSession::new(Arc::new(move |st| {
        obs_clone.lock().push(st);
    }));

    let profile = StreamProfile {
        id: 1,
        name: "Test Station".to_string(),
        host: "127.0.0.1".to_string(),
        port,
        sid: 1,
        password: "test".to_string(),
        mount: "/stream".to_string(),
        codec: Codec::MP3,
        bitrate_kbps: 128,
        sample_rate_hz: 44_100,
        mono: true,
        mode: BroadcastMode::SOURCE,
        dj_user: None,
        dj_password: None,
        dj_port: None,
        dj_protocol: DjProtocol::ULTRAVOX,
        dj_mount: None,
        ..Default::default()
    };

    let audio = Box::new(SineAudioSource::new(44_100, true, 1024));
    session.start(profile, audio, None).unwrap();

    // 1) Wait for Live state
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(8) {
        if matches!(session.state(), StreamState::Live { .. }) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(matches!(session.state(), StreamState::Live { .. }));
    assert_eq!(connect_count.load(Ordering::SeqCst), 1);

    // 2) Kill server -> triggers backoff and reconnecting
    server_running.store(false, Ordering::SeqCst);
    let _ = handle.join();

    tokio::time::sleep(Duration::from_millis(600)).await;

    // 3) Restart server
    let server_running2 = Arc::new(AtomicBool::new(true));
    let handle2 = run_mock_uvox_server(port, server_running2.clone(), connect_count.clone());

    // 4) Wait for auto-resume back to Live
    let start2 = std::time::Instant::now();
    while start2.elapsed() < Duration::from_secs(10) {
        if matches!(session.state(), StreamState::Live { .. }) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(matches!(session.state(), StreamState::Live { .. }));
    assert!(connect_count.load(Ordering::SeqCst) >= 2);

    session.stop_and_shutdown(false);
    server_running2.store(false, Ordering::SeqCst);
    let _ = handle2.join();
}

#[tokio::test]
async fn test_twenty_rapid_start_stop_cycles_no_leaks() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let obs_clone = observed.clone();

    let session = StreamSession::new(Arc::new(move |st| {
        obs_clone.lock().push(st);
    }));

    for i in 0..20 {
        let profile = StreamProfile {
            id: i + 1,
            name: "Cycle".to_string(),
            host: "127.0.0.1".to_string(),
            port: 9999, // Unreachable port -> will fail or connect
            sid: 1,
            password: "test".to_string(),
            mount: "/stream".to_string(),
            codec: Codec::MP3,
            bitrate_kbps: 128,
            sample_rate_hz: 44_100,
            mono: true,
            mode: BroadcastMode::SOURCE,
            dj_user: None,
            dj_password: None,
            dj_port: None,
            dj_protocol: DjProtocol::ULTRAVOX,
            dj_mount: None,
            ..Default::default()
        };

        let audio = Box::new(SineAudioSource::new(44_100, true, 1024));
        let _ = session.start(profile, audio, None);
        tokio::time::sleep(Duration::from_millis(15)).await;
        session.stop_and_shutdown(false);
        assert_eq!(session.state(), StreamState::Idle);
    }
}

#[tokio::test]
async fn test_standalone_recorder_produces_valid_mp3() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let obs_clone = observed.clone();

    let recorder = RecordSession::new(Arc::new(move |st| {
        obs_clone.lock().push(st);
    }));

    // create_new guard: recorder must refuse an existing file, so remove the
    // NamedTempFile placeholder first and reuse its reserved unique path.
    let tmp_file = NamedTempFile::new().unwrap();
    let tmp_path = tmp_file.path().to_path_buf();
    drop(tmp_file);
    let _ = std::fs::remove_file(&tmp_path);

    let config = AudioConfig {
        codec: Codec::MP3,
        bitrate_kbps: 128,
        sample_rate_hz: 44_100,
        mono: true,
    };

    let audio = Box::new(SineAudioSource::new(44_100, true, 1024));
    let path = recorder.start(config, tmp_path.clone(), audio, None).unwrap();
    assert!(matches!(recorder.state(), RecordState::Recording { .. }));

    // Record for 300 ms
    tokio::time::sleep(Duration::from_millis(300)).await;

    recorder.stop_and_shutdown(false);
    assert_eq!(recorder.state(), RecordState::Idle);

    // Verify written file is non-empty and starts with valid MP3 sync
    let mut file = std::fs::File::open(&path).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    assert!(!bytes.is_empty(), "Recorded file must not be empty");

    // MP3 frame sync word: 11 bits set (0xFF, bits 7-5 of next byte)
    assert_eq!(bytes[0], 0xFF);
    assert_eq!(bytes[1] & 0xE0, 0xE0);
    assert_eq!((bytes[1] >> 1) & 0x03, 0x01); // Layer III
}

#[tokio::test]
async fn failed_connect_surfaces_error_and_releases_the_pipeline() {
    let session = StreamSession::new(Arc::new(|_| {}));
    // Nothing listens on port 1: with auto-reconnect off the first refusal is terminal.
    let profile = StreamProfile {
        name: "Fail".to_string(),
        host: "127.0.0.1".to_string(),
        port: 1,
        auto_reconnect: false,
        ..Default::default()
    };
    let audio = Box::new(SineAudioSource::new(44_100, true, 1024));
    session.start(profile, audio, None).unwrap();

    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(10) && session.is_active() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!session.is_active(), "failed session must tear itself down");
    assert!(matches!(session.state(), StreamState::Error { .. }));

    // A user stop afterwards clears the error.
    session.stop_and_shutdown(false);
    assert_eq!(session.state(), StreamState::Idle);
}

#[test]
fn arming_a_recording_while_idle_leaves_no_file() {
    let session = StreamSession::new(Arc::new(|_| {}));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("orphan.mp3");
    assert!(session
        .arm_recording(path.to_string_lossy().to_string(), AudioConfigRules::record_config(128, true))
        .is_err());
    assert!(!path.exists());
}

#[test]
fn now_playing_is_queued_until_live() {
    let session = StreamSession::new(Arc::new(|_| {}));
    assert_eq!(session.update_now_playing("  Morning Talk  "), Ok(false));
    assert_eq!(session.now_playing().as_deref(), Some("Morning Talk"));
    assert!(session.update_now_playing("   ").is_err());
}

/// (bitrate kbps, mono) of the first MPEG-1 Layer III frame in `data`.
fn mp3_header(data: &[u8]) -> (u32, bool) {
    const KBPS: [u32; 15] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
    let i = data
        .windows(2)
        .position(|w| w[0] == 0xFF && w[1] & 0xFE == 0xFA)
        .expect("no MPEG-1 Layer III frame");
    (KBPS[(data[i + 2] >> 4) as usize], data[i + 3] >> 6 == 3)
}

#[tokio::test]
async fn on_air_recording_uses_its_own_quality() {
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").expect("get ephemeral port");
        l.local_addr().unwrap().port()
    };
    let server_running = Arc::new(AtomicBool::new(true));
    let handle = run_mock_uvox_server(port, server_running.clone(), Arc::new(AtomicU32::new(0)));
    let session = StreamSession::new(Arc::new(|_| {}));
    let profile = StreamProfile {
        host: "127.0.0.1".to_string(),
        port,
        sid: 1,
        password: "test".to_string(),
        codec: Codec::MP3,
        bitrate_kbps: 64,
        sample_rate_hz: 44_100,
        mono: true,
        ..Default::default()
    };
    session
        .start(profile, Box::new(SineAudioSource::new(44_100, true, 1024)), None)
        .unwrap();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(8) && !matches!(session.state(), StreamState::Live { .. }) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(matches!(session.state(), StreamState::Live { .. }));

    let dir = tempfile::tempdir().unwrap();
    let hi = dir.path().join("hi.mp3");
    session
        .arm_recording(hi.to_string_lossy().to_string(), AudioConfigRules::record_config(320, false))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;
    session.disarm_recording();

    // Same quality as the broadcast: the broadcast's own frames are kept.
    let same = dir.path().join("same.mp3");
    session
        .arm_recording(same.to_string_lossy().to_string(), AudioConfigRules::record_config(64, true))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;
    session.stop_and_shutdown(false);
    server_running.store(false, Ordering::SeqCst);
    let _ = handle.join();

    assert_eq!(mp3_header(&std::fs::read(&hi).unwrap()), (320, false));
    assert_eq!(mp3_header(&std::fs::read(&same).unwrap()), (64, true));
}
