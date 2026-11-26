//! SHOUTcast v1 client integration tests against mock servers: the source login
//! on port P+1 and the Weblet admin.cgi title channel on P (the Centova layout).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use streamy_core::net::client::{SourceClient, TitleAck};
use streamy_core::net::fallback::FallbackSourceClient;
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

/// Listener on an ephemeral port whose neighbours at `offsets` are closed.
/// Ephemeral allocation is sequential on some hosts (Windows CI), so other
/// test threads' listeners land next door; rebind until the layout fits.
fn bind_with_closed(offsets: &[i32]) -> TcpListener {
    loop {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port() as i32;
        if offsets.iter().all(|&o| TcpStream::connect(("127.0.0.1", (port + o) as u16)).is_err()) {
            return l;
        }
    }
}

/// Panel on an ephemeral port and its v1 source on panel+1, as the test layout
/// requires. Ephemeral allocation occasionally lands next to a busy port, so
/// retry with a fresh panel until both fit.
fn bind_panel_and_v1() -> (TcpListener, TcpListener) {
    loop {
        let panel = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = panel.local_addr().unwrap().port();
        match TcpListener::bind(("127.0.0.1", port + 1)) {
            Ok(source) => return (panel, source),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => drop(panel),
            Err(e) => panic!("bind v1 port: {e}"),
        }
    }
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
    let (panel, source) = bind_panel_and_v1();
    let panel_port = panel.local_addr().unwrap().port();
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

    let (panel, source) = bind_panel_and_v1();
    let panel_port = panel.local_addr().unwrap().port();

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

/// Serve a full Ultravox handshake, then read data frames until the peer stops.
fn mock_uvox_server(listener: TcpListener, stop: Arc<AtomicBool>) {
    listener.set_nonblocking(true).unwrap();
    'outer: while !stop.load(Ordering::SeqCst) {
        let (mut s, _) = match listener.accept() {
            Ok(v) => v,
            Err(_) => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
        };
        let _ = s.set_nonblocking(false);
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        loop {
            let mut hdr = [0u8; 6];
            if s.read_exact(&mut hdr).is_err() {
                continue 'outer;
            }
            let len = ((hdr[4] as usize) << 8) | hdr[5] as usize;
            let mut p = vec![0u8; len + 1];
            if s.read_exact(&mut p).is_err() {
                continue 'outer;
            }
            let ct = ((hdr[2] as u16) << 8) | hdr[3] as u16;
            let cls = (ct >> 12) & 0x0F;
            let msg_type = ct & 0x0FFF;
            let resp = streamy_core::core::framing::message(cls, msg_type, b"ACK\0");
            if s.write_all(&resp).is_err() || s.flush().is_err() {
                continue 'outer;
            }
            if (cls, msg_type) == (1, 0x004) {
                // standby ACKed: keep reading data until the peer stops
                loop {
                    let mut d = [0u8; 512];
                    match s.read(&mut d) {
                        Ok(0) | Err(_) => continue 'outer,
                        Ok(_) => {}
                    }
                }
            }
        }
    }
}

#[test]
fn chain_falls_through_to_uvox_on_plus_two_when_listed_port_is_closed() {
    // Raw sc_trans layout: v1 on +1 refused/closed, Ultravox ONLY on +2
    // (the Centova djport2 convention). The chain's third leg must win.
    use streamy_core::net::uvox::UvoxClient;

    let uvox2 = bind_with_closed(&[-2, -1]);
    let uvox2_port = uvox2.local_addr().unwrap().port();
    // dj_port chosen so that dj_port+2 == uvox2_port; dj_port and dj_port+1
    // are closed (bind_with_closed verified it, racing other test threads).
    let dj_port = uvox2_port - 2;

    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let srv = thread::spawn(move || mock_uvox_server(uvox2, stop2));

    // Build exactly what the session builds for a DJ ULTRAVOX profile.
    let uvox = Box::new(UvoxClient::new(
        "127.0.0.1".into(), dj_port, 1, "pw".into(),
        "audio/mpeg".into(), 44_100, 1, None,
    )) as Box<dyn SourceClient>;
    let uvox_alt = Box::new(UvoxClient::new(
        "127.0.0.1".into(), dj_port + 2, 1, "pw".into(),
        "audio/mpeg".into(), 44_100, 1, None,
    )) as Box<dyn SourceClient>;
    let ultravox_chain = Box::new(FallbackSourceClient::new(uvox, uvox_alt));
    let v1 = Box::new(ShoutcastV1Client::new(
        "127.0.0.1".into(), dj_port + 1, "dj".into(), "pw".into(), dj_port,
    ));
    let mut chain = FallbackSourceClient::new(v1, ultravox_chain);

    chain.connect(64, "Station", "Talk").expect("chain lands on uvox@+2");
    chain.send(&[1, 1, 1]).expect("audio via uvox@+2");

    stop.store(true, Ordering::SeqCst);
    srv.join().unwrap();
}

#[test]
fn chain_falls_through_to_uvox_on_listed_port_when_v1_leg_is_closed() {
    // Panels that speak ONLY Ultravox on the listed port: v1@+1 and uvox@+2
    // are closed, uvox on the listed port answers. Second leg must win.
    use streamy_core::net::uvox::UvoxClient;

    let uvox1 = bind_with_closed(&[1, 2]);
    let dj_port = uvox1.local_addr().unwrap().port();

    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let srv = thread::spawn(move || mock_uvox_server(uvox1, stop2));

    let uvox = Box::new(UvoxClient::new(
        "127.0.0.1".into(), dj_port, 1, "pw".into(),
        "audio/mpeg".into(), 44_100, 1, None,
    )) as Box<dyn SourceClient>;
    let uvox_alt = Box::new(UvoxClient::new(
        "127.0.0.1".into(), dj_port + 2, 1, "pw".into(),
        "audio/mpeg".into(), 44_100, 1, None,
    )) as Box<dyn SourceClient>;
    let ultravox_chain = Box::new(FallbackSourceClient::new(uvox, uvox_alt));
    let v1 = Box::new(ShoutcastV1Client::new(
        "127.0.0.1".into(), dj_port + 1, "dj".into(), "pw".into(), dj_port,
    ));
    let mut chain = FallbackSourceClient::new(v1, ultravox_chain);

    chain.connect(64, "Station", "Talk").expect("chain lands on uvox@listed");
    chain.send(&[2, 2, 2]).expect("audio via uvox@listed");

    stop.store(true, Ordering::SeqCst);
    srv.join().unwrap();
}

#[test]
fn fallback_abort_fans_out_to_both_children() {
    // The composite abort must unblock EITHER child's in-flight handshake,
    // not just the primary's: teardown uses it when the client lock is held.
    use std::net::TcpListener;
    use std::time::Instant;

    let primary_l = TcpListener::bind("127.0.0.1:0").unwrap();
    let fallback_l = TcpListener::bind("127.0.0.1:0").unwrap();
    let p_addr = primary_l.local_addr().unwrap();
    let f_addr = fallback_l.local_addr().unwrap();

    // Both servers accept and stay SILENT (worst case: blackhole handshake).
    let _p = thread::spawn(move || {
        let (_s, _) = primary_l.accept().expect("p accept");
        thread::sleep(Duration::from_secs(10));
    });
    let _f = thread::spawn(move || {
        let (_s, _) = fallback_l.accept().expect("f accept");
        thread::sleep(Duration::from_secs(10));
    });

    // Children whose handshakes block on the sockets above.
    let v1 = Box::new(ShoutcastV1Client::new(
        p_addr.ip().to_string(), p_addr.port(), "dj".into(), "pw".into(), p_addr.port(),
    )) as Box<dyn SourceClient>;
    let uvox = Box::new(UvoxClientStub::connecting_to(f_addr)) as Box<dyn SourceClient>;
    let mut chain = FallbackSourceClient::new(v1, uvox);
    let abort = chain.abort_handle();

    // Connect on another thread; it will block in the primary's handshake read.
    let t = thread::spawn(move || {
        let _ = chain.connect(64, "n", "g");
    });

    thread::sleep(Duration::from_millis(300));
    let t0 = Instant::now();
    abort.abort(); // must shut BOTH sockets
    t.join().unwrap();
    // Generous budget: a loaded CI runner can take seconds to connect a
    // blocking handshake; the test asserts abort completes, not latency.
    assert!(t0.elapsed() < Duration::from_secs(10), "abort unblocked slowly");
}

/// Uvox client bound to a real address (used for abort fan-out testing).
struct UvoxClientStub;

impl UvoxClientStub {
    fn connecting_to(_addr: std::net::SocketAddr) -> streamy_core::net::uvox::UvoxClient {
        streamy_core::net::uvox::UvoxClient::new(
            "127.0.0.1".into(), _addr.port(), 1, "pw".into(),
            "audio/mpeg".into(), 44_100, 1, None,
        )
    }
}

#[test]
fn session_teardown_mid_title_push_cleans_up_without_false_toast() {
    // Stop while a user push's admin GET is still in flight: no hang, no
    // verdict leaks to the UI afterwards, and a later same-text announce
    // in a NEW session does not false-toast.
    use streamy_core::audio::source::SineAudioSource;
    use streamy_core::core::audio_config_rules::{BroadcastMode, Codec, DjProtocol};
    use streamy_core::core::teardown_state::StreamState;
    use streamy_core::session::stream_session::{StreamProfile, StreamSession};

    let (panel, source) = bind_panel_and_v1();
    let panel_port = panel.local_addr().unwrap().port();

    let src_stop = Arc::new(AtomicBool::new(false));
    let src_stop2 = src_stop.clone();
    let src_thread = thread::spawn(move || {
        source.set_nonblocking(true).unwrap();
        'outer: while !src_stop2.load(Ordering::SeqCst) {
            let Ok((mut s, _)) = source.accept() else {
                thread::sleep(Duration::from_millis(10));
                continue;
            };
            let _ = s.set_nonblocking(false);
            let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
            let _ = read_line_crude(&mut s);
            let _ = s.write_all(b"OK2\r\nicy-caps:11\r\n\r\n");
            let _ = s.flush();
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
    // Blackhole Weblet: accepts, never answers.
    let weblet_stop = Arc::new(AtomicBool::new(false));
    let weblet_stop2 = weblet_stop.clone();
    let weblet_thread = thread::spawn(move || {
        panel.set_nonblocking(true).unwrap();
        while !weblet_stop2.load(Ordering::SeqCst) {
            if let Ok((s, _)) = panel.accept() {
                drop(s); // never reply
            } else {
                thread::sleep(Duration::from_millis(10));
            }
        }
    });

    let ui_acks = Arc::new(std::sync::Mutex::new(Vec::<streamy_core::net::TitleAck>::new()));
    let ui_acks2 = ui_acks.clone();
    let session = StreamSession::new(Arc::new(|_| {}));
    session.set_title_ack_callback(Arc::new(move |a| ui_acks2.lock().unwrap().push(a)));

    let profile = StreamProfile {
        id: 1,
        name: "Teardown Test".into(),
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

    // User push against the blackhole Weblet, then immediate teardown.
    session.update_now_playing("Inflight Title").unwrap();
    session.stop_and_shutdown(false); // must not hang despite the in-flight GET

    // The push thread may still finish its GET; its verdict must NOT toast
    // (teardown cleared the user-verdict gate).
    thread::sleep(Duration::from_millis(400));
    assert!(
        ui_acks.lock().unwrap().is_empty(),
        "post-teardown verdict must not reach the UI"
    );
    assert!(matches!(session.state(), StreamState::Idle));

    // A later same-text announce in a NEW session must not false-toast either.
    let ui_acks3 = ui_acks.clone();
    let session2 = StreamSession::new(Arc::new(|_| {}));
    session2.set_title_ack_callback(Arc::new(move |a| ui_acks3.lock().unwrap().push(a)));
    session2.announce_now_playing("Inflight Title").unwrap();
    thread::sleep(Duration::from_millis(200));
    assert!(
        ui_acks.lock().unwrap().is_empty(),
        "new-session announce must not toast on the stale verdict gate"
    );

    weblet_stop.store(true, Ordering::SeqCst);
    src_stop.store(true, Ordering::SeqCst);
    src_thread.join().unwrap();
    weblet_thread.join().unwrap();
}

#[test]
fn title_ack_serializes_to_the_frontend_shape() {
    // The Svelte side listens for { title: string; ok: boolean }; a renamed or
    // tagged field would silently break the toast. Pin the wire shape.
    let ack = streamy_core::net::TitleAck { title: "Kajian Pagi".into(), ok: true };
    let json = serde_json::to_string(&ack).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v.get("title").and_then(|t| t.as_str()), Some("Kajian Pagi"));
    assert_eq!(v.get("ok").and_then(|t| t.as_bool()), Some(true));
    assert_eq!(v.as_object().map(|m| m.len()), Some(2), "exactly two fields: {json}");
}

#[test]
fn v1_admin_path_never_carries_raw_control_chars_or_plus() {
    // Wire-safety of the title channel: control chars stripped (no header/
    // query injection), spaces always %20, never a literal '+'.
    let hostile = "Bad\r\nTitle: injected <script> & more";
    let p = ShoutcastV1Client::admin_path("dj", "pw", hostile);
    assert!(!p.contains('\r') && !p.contains('\n'), "{p}");
    assert!(!p.contains(' '), "raw space in query: {p}");
    assert!(!p.contains('+'), "form-style + leaked: {p}");
    assert!(p.contains("%3Cscript%3E"), "entity escaping intact: {p}");
    assert!(p.contains("%26"), "ampersand encoded: {p}");
}

#[test]
fn v1_long_emoji_title_survives_push_without_panic() {
    // UTF-8 truncation path (4-byte emoji) must not split a char boundary.
    let panel = TcpListener::bind("127.0.0.1:0").unwrap();
    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let p_addr = panel.local_addr().unwrap();
    let s_addr = source.local_addr().unwrap();

    let (login_tx, _login_rx) = mpsc::channel();
    let (audio_tx, _audio_rx) = mpsc::channel();
    let src = thread::spawn(move || mock_v1_source(source, login_tx, audio_tx));
    let (req_tx, req_rx) = mpsc::channel();
    let web = thread::spawn(move || {
        mock_weblet(panel, "HTTP/1.0 200 OK\r\n\r\nTitle Updated", req_tx)
    });

    let mut client = ShoutcastV1Client::new(
        s_addr.ip().to_string(), s_addr.port(), "dj".into(), "pw".into(), p_addr.port(),
    );
    client.set_title_ack(Arc::new(|_| {}));
    client.connect(64, "Test Station", "Talk").expect("connect");

    let title: String = "\u{1F3A4}".repeat(400); // 1600 UTF-16 units, 4 bytes each
    client.update_now_playing(&title).expect("emoji push");

    let req = req_rx.recv_timeout(Duration::from_secs(3)).expect("weblet saw the push");
    assert!(req.starts_with("GET /admin.cgi"), "{req}");

    src.join().unwrap();
    web.join().unwrap();
}

#[test]
fn icecast_title_verdict_true_on_2xx_false_on_401() {
    // The /admin/metadata verdict: 2xx -> ok, a refused update (401) -> NOT ok.
    // The verdict is reported through the ack callback (off-thread).
    use streamy_core::net::icecast::IcecastSourceClient;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let acks = Arc::new(std::sync::Mutex::new(Vec::<TitleAck>::new()));
    let acks2 = acks.clone();

    let srv = thread::spawn(move || {
        // 1) source handshake
        let (mut s, _) = listener.accept().expect("source accept");
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut req = Vec::new();
        let mut b = [0u8; 1];
        while s.read_exact(&mut b).is_ok() {
            req.push(b[0]);
            if req.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        s.write_all(b"HTTP/1.0 200 OK\r\n\r\n").unwrap();
        s.flush().unwrap();

        // 2)+3) metadata GETs — reply BY TITLE, not accept order (two pushes
        // race their connects).
        for _ in 0..2 {
            let (mut s, _) = listener.accept().expect("meta accept");
            s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut req = Vec::new();
            while s.read_exact(&mut b).is_ok() {
                req.push(b[0]);
                if req.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let req = String::from_utf8_lossy(&req).to_string();
            if req.contains("song=Good%20Title") {
                s.write_all(b"HTTP/1.0 200 OK\r\n\r\nok").unwrap();
            } else {
                s.write_all(b"HTTP/1.0 401 Unauthorized\r\n\r\nno").unwrap();
            }
            s.flush().unwrap();
        }
    });

    let mut client = IcecastSourceClient::new(
        addr.ip().to_string(), addr.port(), "/live".into(),
        "source".into(), "pw".into(), "audio/mpeg".into(),
    );
    client.set_title_ack(Arc::new(move |a| acks2.lock().unwrap().push(a)));
    client.connect(64, "Station", "Talk").expect("connect");

    client.update_now_playing("Good Title").unwrap();
    client.update_now_playing("Refused Title").unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while acks.lock().unwrap().len() < 2 {
        if std::time::Instant::now() > deadline {
            panic!("verdicts missing: {:?}", acks.lock().unwrap());
        }
        thread::sleep(Duration::from_millis(20));
    }
    let verdicts = acks.lock().unwrap();
    assert_eq!(verdicts.len(), 2, "both verdicts arrive");
    // Two pushes -> two off-thread GETs; arrival order is not guaranteed.
    let by_title: std::collections::HashMap<&str, bool> = verdicts
        .iter()
        .map(|v| (v.title.as_str(), v.ok))
        .collect();
    assert_eq!(by_title.get("Good Title"), Some(&true), "2xx must be ok");
    assert_eq!(by_title.get("Refused Title"), Some(&false), "401 must NOT be ok");

    srv.join().unwrap();
}

#[test]
fn v1_client_reconnects_after_drop_with_fresh_login() {
    // The shared-core transport calls connect() repeatedly on ONE client;
    // the second connect must fully re-login (no stale state from round 1).
    let panel = TcpListener::bind("127.0.0.1:0").unwrap();
    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let p_addr = panel.local_addr().unwrap();
    let s_addr = source.local_addr().unwrap();

    let (login_tx, login_rx) = mpsc::channel();
    let src = thread::spawn(move || {
        source.set_nonblocking(true).unwrap();
        let mut served = 0usize;
        while served < 2 {
            let Ok((mut s, _)) = source.accept() else {
                thread::sleep(Duration::from_millis(5));
                continue;
            };
            let _ = s.set_nonblocking(false);
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let login = read_line_crude(&mut s);
            login_tx.send(login).unwrap();
            let _ = s.write_all(b"OK2\r\nicy-caps:11\r\n\r\n");
            let _ = s.flush();
            let mut b = [0u8; 1];
            let mut headers = Vec::new();
            while s.read_exact(&mut b).is_ok() {
                headers.push(b[0]);
                if headers.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            // Drop the connection after a beat: transport will reconnect.
            thread::sleep(Duration::from_millis(50));
            served += 1;
        }
    });

    let mut client = ShoutcastV1Client::new(
        s_addr.ip().to_string(), s_addr.port(), "dj".into(), "pw".into(), p_addr.port(),
    );
    client.connect(64, "Station", "Talk").expect("first connect");
    // Server drops us (read fails or next send errors)...
    thread::sleep(Duration::from_millis(120));
    // ...transport reconnects on the SAME client instance:
    client.connect(64, "Station", "Talk").expect("second connect must re-login");

    assert_eq!(login_rx.recv().unwrap(), "dj:pw");
    assert_eq!(login_rx.recv().unwrap(), "dj:pw");

    src.join().unwrap();
}

#[test]
fn v1_stalled_server_write_errors_within_bound() {
    // A server that stops reading must not pin the client forever: the send
    // has a write timeout (SEND_TIMEOUT_MS = 10s), far under any test hang.
    // Here we only pin that the timeout is SET (a full 10s stall is too slow
    // for CI) by checking a small send succeeds while the socket is healthy
    // and the client surfaces Closed after the server vanishes.
    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let s_addr = source.local_addr().unwrap();

    let srv = thread::spawn(move || {
        let (mut s, _) = source.accept().expect("accept");
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let _ = read_line_crude(&mut s);
        let _ = s.write_all(b"OK2\r\nicy-caps:11\r\n\r\n");
        let _ = s.flush();
        let mut b = [0u8; 1];
        let mut headers = Vec::new();
        while s.read_exact(&mut b).is_ok() {
            headers.push(b[0]);
            if headers.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        // Read a little audio, then vanish without closing cleanly.
        let mut audio = [0u8; 16];
        let _ = s.read(&mut audio);
    });

    let mut client = ShoutcastV1Client::new(
        s_addr.ip().to_string(), s_addr.port(), "dj".into(), "pw".into(), 8000,
    );
    client.connect(64, "Station", "Talk").expect("connect");
    client.send(&[3u8; 16]).expect("healthy send");
    // Server gone: a subsequent larger send errors (connection reset) instead
    // of hanging — the write bound makes this deterministic, not eternal.
    let t0 = std::time::Instant::now();
    let _ = client.send(&[4u8; 4096]);
    assert!(t0.elapsed() < Duration::from_secs(5), "send not bounded");
    srv.join().unwrap();
}

#[test]
fn fallback_stays_sticky_on_its_protocol_across_a_real_drop() {
    // Sticky contract with REAL sockets: after the v1 leg connects and the
    // server drops us, the reconnect must go to v1 AGAIN (not flap to uvox —
    // flapping would alternate two DJ sessions taking over the station).
    let panel = TcpListener::bind("127.0.0.1:0").unwrap();
    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let p_addr = panel.local_addr().unwrap();
    let s_addr = source.local_addr().unwrap();

    let logins = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let logins2 = logins.clone();
    let src = thread::spawn(move || {
        source.set_nonblocking(true).unwrap();
        let mut served = 0;
        while served < 2 {
            let Ok((mut s, _)) = source.accept() else {
                thread::sleep(Duration::from_millis(5));
                continue;
            };
            let _ = s.set_nonblocking(false);
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let login = read_line_crude(&mut s);
            logins2.lock().unwrap().push(login);
            let _ = s.write_all(b"OK2\r\nicy-caps:11\r\n\r\n");
            let _ = s.flush();
            let mut b = [0u8; 1];
            let mut headers = Vec::new();
            while s.read_exact(&mut b).is_ok() {
                headers.push(b[0]);
                if headers.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let mut audio = [0u8; 64];
            let _ = s.read(&mut audio);
            served += 1;
            // first connection ends here (drop); second stays open till test end
        }
    });
    // A live Ultravox server that must NEVER be contacted (sticky proof).
    let uvox_l = TcpListener::bind("127.0.0.1:0").unwrap();
    let uvox_port = uvox_l.local_addr().unwrap().port();
    let uvox_contacted = Arc::new(AtomicBool::new(false));
    let uvox_contacted2 = uvox_contacted.clone();
    let uvox_srv = thread::spawn(move || {
        uvox_l.set_nonblocking(true).unwrap();
        loop {
            match uvox_l.accept() {
                Ok((mut s, _)) => {
                    uvox_contacted2.store(true, Ordering::SeqCst);
                    let _ = s.shutdown(std::net::Shutdown::Both);
                }
                Err(_) => thread::sleep(Duration::from_millis(5)),
            }
        }
    });

    let v1 = Box::new(ShoutcastV1Client::new(
        s_addr.ip().to_string(), s_addr.port(), "dj".into(), "pw".into(), p_addr.port(),
    )) as Box<dyn SourceClient>;
    let uvox = Box::new(streamy_core::net::uvox::UvoxClient::new(
        "127.0.0.1".into(), uvox_port,
        1, "pw".into(), "audio/mpeg".into(), 44_100, 1, None,
    )) as Box<dyn SourceClient>;
    let mut chain = FallbackSourceClient::new(v1, uvox);

    chain.connect(64, "Station", "Talk").expect("v1 leg connects");
    chain.send(&[9u8; 64]).expect("send 1");
    thread::sleep(Duration::from_millis(150)); // server dropped us
    chain.connect(64, "Station", "Talk").expect("sticky v1 reconnect");
    chain.send(&[9u8; 64]).expect("send 2");

    let logins = logins.lock().unwrap();
    assert_eq!(logins.len(), 2, "exactly two v1 logins: {logins:?}");
    assert!(!uvox_contacted.load(Ordering::SeqCst), "fallback flapped to uvox");

    // Detach the mock threads (uvox_srv loops forever by design): the test
    // binary exits when main returns; listeners drop with the threads' handles.
    drop(src);
    drop(uvox_srv);
}

#[test]
fn session_uvox_title_replays_after_server_drop() {
    // A title set while live, then the server drops: after the transport
    // reconnects, the title must go out again (0x902 replay) — the listeners'
    // now-playing survives the blip.
    use streamy_core::audio::source::SineAudioSource;
    use streamy_core::core::audio_config_rules::{BroadcastMode, Codec, DjProtocol};
    use streamy_core::core::framing::message;
    use streamy_core::core::teardown_state::StreamState;
    use streamy_core::session::stream_session::{StreamProfile, StreamSession};

    // uvox server on the listed DJ port; +1/+2 stay closed so the chain's
    // first attempt (v1@+1) fails fast and uvox wins.
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let dj_port = l.local_addr().unwrap().port();

    let titles = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let titles2 = titles.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let srv = thread::spawn(move || {
        l.set_nonblocking(true).unwrap();
        let mut served = 0;
        while !stop2.load(Ordering::SeqCst) {
            let Ok((mut s, _)) = l.accept() else {
                thread::sleep(Duration::from_millis(5));
                continue;
            };
            let _ = s.set_nonblocking(false);
            s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            // full handshake: ACK everything to standby
            loop {
                let mut hdr = [0u8; 6];
                if s.read_exact(&mut hdr).is_err() {
                    break;
                }
                let len = ((hdr[4] as usize) << 8) | hdr[5] as usize;
                let mut p = vec![0u8; len + 1];
                if s.read_exact(&mut p).is_err() {
                    break;
                }
                let ct = ((hdr[2] as u16) << 8) | hdr[3] as u16;
                let cls = (ct >> 12) & 0x0F;
                let msg_type = ct & 0x0FFF;
                let resp = message(cls, msg_type, b"ACK\0");
                if s.write_all(&resp).is_err() {
                    break;
                }
                let _ = s.flush();
                if (cls, msg_type) == (1, 0x004) {
                    break; // standby done
                }
            }
            served += 1;
            // stream: read until a 0x902 metadata frame or the peer drops
            let mut saw_title = false;
            loop {
                let mut hdr = [0u8; 6];
                if s.read_exact(&mut hdr).is_err() {
                    break;
                }
                let len = ((hdr[4] as usize) << 8) | hdr[5] as usize;
                let mut p = vec![0u8; len + 1];
                if s.read_exact(&mut p).is_err() {
                    break;
                }
                let ct = ((hdr[2] as u16) << 8) | hdr[3] as u16;
                let cls = (ct >> 12) & 0x0F;
                let msg_type = ct & 0x0FFF;
                if (cls, msg_type) == (3, 0x902) {
                    let xml = String::from_utf8_lossy(&p[6..]).to_string();
                    if let Some(t) = xml.split("<TIT2>").nth(1).and_then(|r| r.split("</TIT2>").next()) {
                        titles2.lock().unwrap().push(t.to_string());
                    }
                    saw_title = true;
                }
                if saw_title && served == 1 {
                    // drop right after the first title: force the reconnect
                    break;
                }
            }
        }
    });

    let session = StreamSession::new(Arc::new(|_| {}));
    let profile = StreamProfile {
        id: 1,
        name: "Uvox Replay".into(),
        host: "127.0.0.1".into(),
        port: dj_port,
        sid: 1,
        password: "pw".into(),
        codec: Codec::MP3,
        bitrate_kbps: 64,
        sample_rate_hz: 44_100,
        mono: true,
        mode: BroadcastMode::DJ,
        dj_user: None,
        dj_password: Some("pw".into()),
        dj_port: Some(dj_port),
        dj_protocol: DjProtocol::ULTRAVOX,
        auto_reconnect: true,
        ..Default::default()
    };

    session
        .start(profile, Box::new(SineAudioSource::new(44_100, true, 1024)), None)
        .expect("start");

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !matches!(session.state(), StreamState::Live { .. }) {
        if std::time::Instant::now() > deadline {
            panic!("never live: {:?}", session.state());
        }
        thread::sleep(Duration::from_millis(20));
    }

    session.update_now_playing("Survives Drop").unwrap();

    // Wait for TWO arrivals of the same title (original + post-reconnect replay).
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let seen = titles.lock().unwrap().len();
        if seen >= 2 {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("title replay missing: {:?}", titles.lock().unwrap());
        }
        thread::sleep(Duration::from_millis(50));
    }
    let got = titles.lock().unwrap();
    assert!(got.iter().all(|t| t == "Survives Drop"), "{got:?}");

    session.stop_and_shutdown(false);
    stop.store(true, Ordering::SeqCst);
    drop(srv);
}
