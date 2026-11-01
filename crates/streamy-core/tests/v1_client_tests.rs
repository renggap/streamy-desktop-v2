//! SHOUTcast v1 client integration tests against mock servers: the source login
//! on port P+1 and the Weblet admin.cgi title channel on P (the Centova layout).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use streamy_core::net::client::{SourceClient, TitleAck};
use streamy_core::net::shoutcast_v1::ShoutcastV1Client;

fn read_line_crude(s: &mut TcpStream) -> String {
    let mut out = Vec::new();
    let mut b = [0u8; 1];
    while s.read_exact(&mut b).is_ok() {
        if b[0] == b'\n' {
            break;
        }
        if b[0] != b'\r' {
            out.push(b[0]);
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Mock sc_trans v1 port: expects `user:pass` login, answers OK2 + caps, then
/// reads raw MP3 bytes. Returns what it received.
fn mock_v1_source(listener: TcpListener, login_rx: mpsc::Sender<String>, audio_rx: mpsc::Sender<Vec<u8>>) {
    let (mut s, _) = listener.accept().expect("v1 accept");
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let login = read_line_crude(&mut s);
    login_rx.send(login).unwrap();
    s.write_all(b"OK2\r\nicy-caps:11\r\n\r\n").unwrap();
    s.flush().unwrap();
    // icy headers from the client
    let mut headers = Vec::new();
    let mut b = [0u8; 1];
    while s.read_exact(&mut b).is_ok() {
        headers.push(b[0]);
        if headers.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let headers = String::from_utf8_lossy(&headers).to_string();
    assert!(headers.contains("icy-name:Test Station"), "{headers}");
    assert!(headers.contains("content-type:audio/mpeg"), "{headers}");
    // raw audio bytes follow (if the test sends any)
    let mut audio = vec![0u8; 5];
    if s.read_exact(&mut audio).is_ok() {
        audio_rx.send(audio).unwrap();
    }
}

/// Mock Weblet: answers the admin.cgi GET with the given body.
fn mock_weblet(listener: TcpListener, reply: &'static str, req_rx: mpsc::Sender<String>) {
    let (mut s, _) = listener.accept().expect("weblet accept");
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut req = Vec::new();
    let mut b = [0u8; 1];
    while s.read_exact(&mut b).is_ok() {
        req.push(b[0]);
        if req.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let req = String::from_utf8_lossy(&req).to_string();
    req_rx.send(req).unwrap();
    s.write_all(reply.as_bytes()).unwrap();
    s.flush().unwrap();
}

/// Wait until at least `n` verdicts arrived (the pushes are off-thread now).
fn wait_for_acks(acks: &std::sync::Arc<std::sync::Mutex<Vec<TitleAck>>>, n: usize, what: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while acks.lock().unwrap().len() < n {
        if std::time::Instant::now() > deadline {
            panic!("timed out waiting for {what}");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn v1_login_ok2_send_audio_and_title_verdicts_true() {
    let panel = TcpListener::bind("127.0.0.1:0").unwrap();
    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let panel_addr = panel.local_addr().unwrap();
    let source_addr = source.local_addr().unwrap();

    let (login_tx, login_rx) = mpsc::channel();
    let (audio_tx, audio_rx) = mpsc::channel();
    let (req_tx, req_rx) = mpsc::channel();

    let src_thread = thread::spawn(move || mock_v1_source(source, login_tx, audio_tx));
    let weblet_thread = thread::spawn(move || {
        mock_weblet(
            panel,
            "HTTP/1.0 200 OK\r\nContent-Type: text/html\r\n\r\nTitle Updated",
            req_tx,
        )
    });

    let mut client = ShoutcastV1Client::new(
        source_addr.ip().to_string(),
        source_addr.port(),
        "djuser".into(),
        "djpass".into(),
        panel_addr.port(),
    );

    let acks = std::sync::Arc::new(std::sync::Mutex::new(Vec::<TitleAck>::new()));
    let sink = acks.clone();
    client.set_title_ack(std::sync::Arc::new(move |a| sink.lock().unwrap().push(a)));

    client.connect(64, "Test Station", "Talk").expect("v1 connect");
    client.send(&[1, 2, 3, 4, 5]).expect("v1 send");

    // Title push: goes to the Weblet, which confirms (off-thread: poll for it).
    client.update_now_playing("Kajian Pagi 84698").expect("title push");
    wait_for_acks(&acks, 1, "one verdict");

    assert_eq!(login_rx.recv().unwrap(), "djuser:djpass");
    assert_eq!(audio_rx.recv().unwrap(), vec![1, 2, 3, 4, 5]);

    let req = req_rx.recv_timeout(Duration::from_secs(3)).expect("weblet got the title");
    assert!(req.starts_with("GET /admin.cgi?mode=updinfo"), "{req}");
    // %20, never +: sc_trans keeps a literal '+' (live-proven).
    assert!(req.contains("song=Kajian%20Pagi%2084698"), "{req}");
    assert!(req.contains("pass=djuser%3Adjpass"), "{req}");
    assert!(!req.contains('+'), "{req}");

    let acks = acks.lock().unwrap();
    assert_eq!(acks.len(), 1, "exactly one verdict");
    assert!(acks[0].ok, "weblet said Title Updated");
    assert_eq!(acks[0].title, "Kajian Pagi 84698");

    src_thread.join().unwrap();
    weblet_thread.join().unwrap();
}

#[test]
fn v1_title_verdicts_false_when_weblet_refuses() {
    let panel = TcpListener::bind("127.0.0.1:0").unwrap();
    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let panel_addr = panel.local_addr().unwrap();
    let source_addr = source.local_addr().unwrap();

    let (login_tx, _login_rx) = mpsc::channel();
    let (audio_tx, _audio_rx) = mpsc::channel();
    let (req_tx, _req_rx) = mpsc::channel();

    let src_thread = thread::spawn(move || mock_v1_source(source, login_tx, audio_tx));
    let weblet_thread = thread::spawn(move || {
        // A wrong password answers 404 (live-proven on sc_trans's Weblet).
        mock_weblet(panel, "HTTP/1.0 404 Not Found\r\n\r\nnot found", req_tx)
    });

    let mut client = ShoutcastV1Client::new(
        source_addr.ip().to_string(),
        source_addr.port(),
        "djuser".into(),
        "djpass".into(),
        panel_addr.port(),
    );
    let acks = std::sync::Arc::new(std::sync::Mutex::new(Vec::<TitleAck>::new()));
    let sink = acks.clone();
    client.set_title_ack(std::sync::Arc::new(move |a| sink.lock().unwrap().push(a)));

    client.connect(64, "Test Station", "Talk").expect("v1 connect");
    client.update_now_playing("Refused Title").expect("title push");
    wait_for_acks(&acks, 1, "one verdict");

    let acks = acks.lock().unwrap();
    assert_eq!(acks.len(), 1);
    assert!(!acks[0].ok, "404 must not be reported as success");

    src_thread.join().unwrap();
    weblet_thread.join().unwrap();
}

#[test]
fn v1_title_before_connect_is_deferred_and_replayed_after_login() {
    let panel = TcpListener::bind("127.0.0.1:0").unwrap();
    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let panel_addr = panel.local_addr().unwrap();
    let source_addr = source.local_addr().unwrap();

    let (login_tx, _login_rx) = mpsc::channel();
    let (audio_tx, _audio_rx) = mpsc::channel();
    let (req_tx, req_rx) = mpsc::channel();

    let src_thread = thread::spawn(move || mock_v1_source(source, login_tx, audio_tx));
    let weblet_thread = thread::spawn(move || {
        mock_weblet(
            panel,
            "HTTP/1.0 200 OK\r\n\r\nTitle Updated",
            req_tx,
        )
    });

    let mut client = ShoutcastV1Client::new(
        source_addr.ip().to_string(),
        source_addr.port(),
        "djuser".into(),
        "djpass".into(),
        panel_addr.port(),
    );
    let acks = std::sync::Arc::new(std::sync::Mutex::new(Vec::<TitleAck>::new()));
    let sink = acks.clone();
    client.set_title_ack(std::sync::Arc::new(move |a| sink.lock().unwrap().push(a)));

    // Not connected: no verdict, no network call.
    client.update_now_playing("Early Title").expect("queued title");
    assert!(acks.lock().unwrap().is_empty(), "no verdict before login");

    // connect() replays the title to the Weblet (off-thread: wait for it).
    client.connect(64, "Test Station", "Talk").expect("v1 connect");
    let req = req_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("replayed after handshake");
    assert!(req.contains("song=Early%20Title"), "{req}");

    src_thread.join().unwrap();
    weblet_thread.join().unwrap();
}

#[test]
fn v1_bad_password_reports_auth_nak() {
    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let source_addr = source.local_addr().unwrap();

    let srv = thread::spawn(move || {
        let (mut s, _) = source.accept().expect("accept");
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let _ = read_line_crude(&mut s);
        s.write_all(b"invalid password\r\n").unwrap();
    });

    let mut client = ShoutcastV1Client::new(
        source_addr.ip().to_string(),
        source_addr.port(),
        "djuser".into(),
        "wrongpass".into(),
        8000,
    );
    let err = client.connect(64, "n", "g").expect_err("must fail");
    assert!(
        err.to_string().starts_with("auth NAK"),
        "got: {err}"
    );
    srv.join().unwrap();
}

#[test]
fn v1_http_port_is_not_a_v1_port() {
    // An HTTP server on the candidate port answers a status line, not OK2:
    // that's the "not a v1 port at all" case the fallback must detect.
    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let source_addr = source.local_addr().unwrap();

    let srv = thread::spawn(move || {
        let (mut s, _) = source.accept().expect("accept");
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let _ = read_line_crude(&mut s);
        s.write_all(b"HTTP/1.0 404 Not Found\r\n\r\n").unwrap();
    });

    let mut client = ShoutcastV1Client::new(
        source_addr.ip().to_string(),
        source_addr.port(),
        "djuser".into(),
        "djpass".into(),
        8000,
    );
    let err = client.connect(64, "n", "g").expect_err("must fail");
    assert!(
        err.to_string().starts_with("v1 NAK"),
        "got: {err}"
    );
    srv.join().unwrap();
}

#[test]
fn fallback_v1_refused_then_uvox_wins_the_chain() {
    // The Centova scenario: the entered port's +1 speaks v1 but REFUSES the
    // login (auth NAK), and a real Ultravox DJ port answers on +2. The chain
    // must land on Ultravox, not fail on the v1 leg's error.
    use streamy_core::net::fallback::FallbackSourceClient;
    use streamy_core::net::uvox::UvoxClient;

    let v1port = TcpListener::bind("127.0.0.1:0").unwrap();
    let uvoxport = TcpListener::bind("127.0.0.1:0").unwrap();
    let v1_addr = v1port.local_addr().unwrap();
    let uvox_addr = uvoxport.local_addr().unwrap();

    let v1_thread = thread::spawn(move || {
        let (mut s, _) = v1port.accept().expect("v1 accept");
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let _ = read_line_crude(&mut s);
        s.write_all(b"invalid password\r\n").unwrap();
    });

    // Minimal Ultravox server: ACK every message until standby completes, then read one data frame.
    let uvox_thread = thread::spawn(move || {
        let (mut s, _) = uvoxport.accept().expect("uvox accept");
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        loop {
            let mut hdr = [0u8; 6];
            s.read_exact(&mut hdr).expect("uvox hdr");
            let len = ((hdr[4] as usize) << 8) | hdr[5] as usize;
            let mut p = vec![0u8; len];
            s.read_exact(&mut p).expect("uvox payload");
            let mut tr = [0u8; 1];
            s.read_exact(&mut tr).expect("uvox trailer");
            let ct = ((hdr[2] as u16) << 8) | hdr[3] as u16;
            let cls = (ct >> 12) & 0x0F;
            let msg_type = ct & 0x0FFF;
            let resp = streamy_core::core::framing::message(cls, msg_type, b"ACK\0");
            s.write_all(&resp).unwrap();
            s.flush().unwrap();
            if (cls, msg_type) == (1, 0x004) {
                break; // standby ACKed: handshake done, data may follow
            }
        }
        let mut hdr = [0u8; 6];
        s.read_exact(&mut hdr).expect("data hdr");
        let len = ((hdr[4] as usize) << 8) | hdr[5] as usize;
        let mut p = vec![0u8; len + 1];
        s.read_exact(&mut p).expect("data payload+trailer");
    });

    let v1 = Box::new(ShoutcastV1Client::new(
        v1_addr.ip().to_string(),
        v1_addr.port(),
        "dj".into(),
        "bad".into(),
        v1_addr.port().saturating_sub(1),
    )) as Box<dyn SourceClient>;
    let uvox = Box::new(UvoxClient::new(
        uvox_addr.ip().to_string(),
        uvox_addr.port(),
        1,
        "pw".into(),
        "audio/mpeg".into(),
        44_100,
        1,
        None,
    )) as Box<dyn SourceClient>;

    let mut chain = FallbackSourceClient::new(v1, uvox);
    chain.connect(64, "Station", "Talk").expect("chain must land on uvox");
    chain.send(&[9, 9, 9]).expect("send via uvox");

    v1_thread.join().unwrap();
    uvox_thread.join().unwrap();
}

#[test]
fn v1_title_push_never_stalls_the_audio_path() {
    // Regression guard: the admin GET runs off-thread. With a Weblet that never
    // answers, update_now_playing must return immediately (the transport worker
    // calls it between audio frames; a synchronous GET would stall ~9s).
    let panel = TcpListener::bind("127.0.0.1:0").unwrap();
    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let panel_addr = panel.local_addr().unwrap();
    let source_addr = source.local_addr().unwrap();

    // Weblet accepts and never replies (worst case: blackhole).
    let blackhole = thread::spawn(move || {
        let (_s, _) = panel.accept().expect("weblet accept");
        thread::sleep(Duration::from_secs(5));
    });

    let (login_tx, _login_rx) = mpsc::channel();
    let (audio_tx, _audio_rx) = mpsc::channel();
    let src_thread = thread::spawn(move || mock_v1_source(source, login_tx, audio_tx));

    let mut client = ShoutcastV1Client::new(
        source_addr.ip().to_string(),
        source_addr.port(),
        "djuser".into(),
        "djpass".into(),
        panel_addr.port(),
    );
    client.connect(64, "Test Station", "Talk").expect("v1 connect");

    let t0 = std::time::Instant::now();
    client.update_now_playing("Slow Weblet Title").expect("title push");
    let elapsed = t0.elapsed();
    assert!(elapsed < Duration::from_millis(200), "push blocked for {elapsed:?}");
    // Audio keeps flowing meanwhile.
    client.send(&[7, 7, 7]).expect("send not blocked");

    src_thread.join().unwrap();
    blackhole.join().unwrap();
}

#[test]
fn session_dj_ultravox_builds_v1_first_chain_and_reports_verdict() {
    // End-to-end through the session: a DJ ULTRAVOX profile must try v1 on
    // djport+1 first; when the v1 login succeeds, audio goes raw and the title
    // verdict comes back through the session's ack callback.
    use streamy_core::audio::source::SineAudioSource;
    use streamy_core::core::audio_config_rules::{BroadcastMode, Codec, DjProtocol};
    use streamy_core::core::teardown_state::StreamState;
    use streamy_core::session::stream_session::{StreamProfile, StreamSession};

    // Panel port: the Weblet (title channel).
    let panel = TcpListener::bind("127.0.0.1:0").unwrap();
    let panel_port = panel.local_addr().unwrap().port();
    // v1 source port = panel + 1 in the test's layout: bind it explicitly.
    let source = TcpListener::bind(("127.0.0.1", panel_port + 1)).expect("bind v1 port");
    // The uvox candidates (panel port and panel+2) must REFUSE or stay closed
    // so the chain proves it picked the v1 leg first. Bind them to nothing:
    // a closed port errors fast with ECONNREFUSED on loopback.
    let dj_port = panel_port; // profile lists the panel port

    let (login_tx, login_rx) = mpsc::channel();
    let (audio_tx, _audio_rx) = mpsc::channel();
    let weblet_reqs = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let weblet_reqs2 = weblet_reqs.clone();

    let src_stop = Arc::new(AtomicBool::new(false));
    let src_stop2 = src_stop.clone();
    let src_thread = thread::spawn(move || {
        source.set_nonblocking(true).unwrap();
        'outer: while !src_stop2.load(Ordering::SeqCst) {
            let (mut s, _) = match source.accept() {
                Ok(v) => v,
                Err(_) => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
            };
            let _ = s.set_nonblocking(false);
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let login = read_line_crude(&mut s);
        login_tx.send(login).unwrap();
        s.write_all(b"OK2\r\nicy-caps:11\r\n\r\n").unwrap();
        s.flush().unwrap();
        // drain icy headers
        let mut b = [0u8; 1];
        let mut headers = Vec::new();
        while s.read_exact(&mut b).is_ok() {
            headers.push(b[0]);
            if headers.ends_with(b"\r\n\r\n") {
                break;
            }
        }
            // read some audio until the peer stops
            let mut audio = vec![0u8; 512];
            loop {
                match s.read(&mut audio) {
                    Ok(0) | Err(_) => continue 'outer,
                    Ok(_) => {}
                }
            }
            #[allow(unreachable_code)]
            {
                let _: mpsc::Sender<Vec<u8>> = audio_tx;
            }
        }
    });
    let weblet_stop = Arc::new(AtomicBool::new(false));
    let weblet_stop2 = weblet_stop.clone();
    let weblet_thread = thread::spawn(move || {
        // Serve pushes until flagged (non-blocking accept so the flag is seen).
        let panel2 = panel.try_clone().unwrap();
        drop(panel);
        panel2.set_nonblocking(true).unwrap();
        while !weblet_stop2.load(Ordering::SeqCst) {
            let (mut s, _) = match panel2.accept() {
                Ok(v) => v,
                Err(_) => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
            };
            let _ = s.set_nonblocking(false);
            s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut req = Vec::new();
            let mut b = [0u8; 1];
            while s.read_exact(&mut b).is_ok() {
                req.push(b[0]);
                if req.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            if req.is_empty() {
                continue;
            }
            weblet_reqs2.lock().unwrap().push(String::from_utf8_lossy(&req).to_string());
            s.write_all(b"HTTP/1.0 200 OK\r\n\r\nTitle Updated").unwrap();
            s.flush().unwrap();
        }
    });

    let acks = Arc::new(std::sync::Mutex::new(Vec::<streamy_core::net::TitleAck>::new()));
    let acks2 = acks.clone();

    let session = StreamSession::new(Arc::new(|_| {}));
    session.set_title_ack_callback(Arc::new(move |a| acks2.lock().unwrap().push(a)));

    let profile = StreamProfile {
        id: 1,
        name: "Centova Test".into(),
        host: "127.0.0.1".into(),
        port: dj_port,
        sid: 1,
        password: "djpass".into(),
        codec: Codec::MP3,
        bitrate_kbps: 64,
        sample_rate_hz: 44_100,
        mono: true,
        mode: BroadcastMode::DJ,
        dj_user: Some("djuser".into()),
        dj_password: Some("djpass".into()),
        dj_port: Some(dj_port),
        dj_protocol: DjProtocol::ULTRAVOX,
        ..Default::default()
    };

    session
        .start(profile, Box::new(SineAudioSource::new(44_100, true, 1024)), None)
        .expect("session start");

    // Wait for the v1 leg to log in: the chain must have picked it first.
    let login = login_rx.recv_timeout(Duration::from_secs(5)).expect("v1 leg used");
    assert_eq!(login, "djuser:djpass");

    // Push a title; the Weblet confirms it.
    session.update_now_playing("Kajian Pagi").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while acks.lock().unwrap().is_empty() {
        if std::time::Instant::now() > deadline {
            panic!("no verdict arrived");
        }
        thread::sleep(Duration::from_millis(20));
    }
    let verdicts = acks.lock().unwrap();
    assert!(verdicts[0].ok, "weblet confirmed");
    assert_eq!(verdicts[0].title, "Kajian Pagi");

    // Session reached Live through the v1 leg.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if matches!(session.state(), StreamState::Live { .. }) {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("never went live; state = {:?}", session.state());
        }
        thread::sleep(Duration::from_millis(20));
    }

    session.stop_and_shutdown(false);
    weblet_stop.store(true, Ordering::SeqCst);
    src_stop.store(true, Ordering::SeqCst);
    src_thread.join().unwrap();
    weblet_thread.join().unwrap();
}

#[test]
fn session_announces_are_silent_user_pushes_toast() {
    // Gating: only the USER-initiated push's verdict reaches the UI callback.
    // A playlist announce with the SAME title text must not fire it twice.
    use streamy_core::audio::source::SineAudioSource;
    use streamy_core::core::audio_config_rules::{BroadcastMode, Codec, DjProtocol};
    use streamy_core::core::teardown_state::StreamState;
    use streamy_core::session::stream_session::{StreamProfile, StreamSession};

    let panel = TcpListener::bind("127.0.0.1:0").unwrap();
    let panel_port = panel.local_addr().unwrap().port();
    let source = TcpListener::bind(("127.0.0.1", panel_port + 1)).expect("bind v1 port");

    let src_stop = Arc::new(AtomicBool::new(false));
    let src_stop2 = src_stop.clone();
    let src_thread = thread::spawn(move || {
        source.set_nonblocking(true).unwrap();
        'outer: while !src_stop2.load(Ordering::SeqCst) {
            let (mut s, _) = match source.accept() {
                Ok(v) => v,
                Err(_) => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
            };
            let _ = s.set_nonblocking(false);
            let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
            let _ = read_line_crude(&mut s);
            s.write_all(b"OK2\r\nicy-caps:11\r\n\r\n").unwrap();
            s.flush().unwrap();
            let mut b = [0u8; 1];
            let mut headers = Vec::new();
            while s.read_exact(&mut b).is_ok() {
                headers.push(b[0]);
                if headers.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let mut audio = vec![0u8; 512];
            loop {
                match s.read(&mut audio) {
                    Ok(0) | Err(_) => continue 'outer,
                    Ok(_) => {}
                }
            }
        }
    });
    // Weblet accepts title pushes and confirms, until the session stops.
    let weblet_stop = Arc::new(AtomicBool::new(false));
    let weblet_stop2 = weblet_stop.clone();
    let panel2 = panel.try_clone().unwrap();
    let weblet_thread = thread::spawn(move || {
        panel2.set_nonblocking(true).unwrap();
        while !weblet_stop2.load(Ordering::SeqCst) {
            let (mut s, _) = match panel2.accept() {
                Ok(v) => v,
                Err(_) => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
            };
            let _ = s.set_nonblocking(false);
            s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut req = Vec::new();
            let mut b = [0u8; 1];
            while s.read_exact(&mut b).is_ok() {
                req.push(b[0]);
                if req.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            if req.is_empty() {
                continue;
            }
            s.write_all(b"HTTP/1.0 200 OK\r\n\r\nTitle Updated").unwrap();
            s.flush().unwrap();
        }
    });

    let ui_acks = Arc::new(std::sync::Mutex::new(Vec::<streamy_core::net::TitleAck>::new()));
    let ui_acks2 = ui_acks.clone();

    let session = StreamSession::new(Arc::new(|_| {}));
    session.set_title_ack_callback(Arc::new(move |a| ui_acks2.lock().unwrap().push(a)));

    let profile = StreamProfile {
        id: 1,
        name: "Gate Test".into(),
        host: "127.0.0.1".into(),
        port: panel_port,
        sid: 1,
        password: "djpass".into(),
        codec: Codec::MP3,
        bitrate_kbps: 64,
        sample_rate_hz: 44_100,
        mono: true,
        mode: BroadcastMode::DJ,
        dj_user: Some("djuser".into()),
        dj_password: Some("djpass".into()),
        dj_port: Some(panel_port),
        dj_protocol: DjProtocol::ULTRAVOX,
        ..Default::default()
    };

    session
        .start(profile, Box::new(SineAudioSource::new(44_100, true, 1024)), None)
        .expect("session start");

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !matches!(session.state(), StreamState::Live { .. }) {
        if std::time::Instant::now() > deadline {
            panic!("never went live; state = {:?}", session.state());
        }
        thread::sleep(Duration::from_millis(20));
    }

    // Playlist announce: reaches the server, NO UI verdict.
    session.announce_now_playing("Track A").unwrap();
    thread::sleep(Duration::from_millis(300));
    assert!(ui_acks.lock().unwrap().is_empty(), "announce must be silent");

    // User push: exactly ONE UI verdict.
    session.update_now_playing("Track A").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while ui_acks.lock().unwrap().is_empty() {
        if std::time::Instant::now() > deadline {
            panic!("user push verdict never arrived; state={:?} np={:?}", session.state(), session.now_playing());
        }
        thread::sleep(Duration::from_millis(20));
    }
    thread::sleep(Duration::from_millis(200));
    let verdicts = ui_acks.lock().unwrap();
    assert_eq!(verdicts.len(), 1, "exactly one verdict for the user push");
    assert!(verdicts[0].ok);

    session.stop_and_shutdown(false);
    weblet_stop.store(true, Ordering::SeqCst);
    src_stop.store(true, Ordering::SeqCst);
    src_thread.join().unwrap();
    weblet_thread.join().unwrap();
}
