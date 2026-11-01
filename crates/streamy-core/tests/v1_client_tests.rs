//! SHOUTcast v1 client integration tests against mock servers: the source login
//! on port P+1 and the Weblet admin.cgi title channel on P (the Centova layout).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
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

    // Title push: goes to the Weblet, which confirms.
    client.update_now_playing("Kajian Pagi 84698").expect("title push");

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
