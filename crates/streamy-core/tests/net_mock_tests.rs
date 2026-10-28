use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use streamy_core::core::framing::message;
use streamy_core::net::client::SourceClient;
use streamy_core::net::icecast::IcecastSourceClient;
use streamy_core::net::uvox::UvoxClient;

fn read_msg(stream: &mut TcpStream) -> (u16, u16, Vec<u8>) {
    let mut hdr = [0u8; 6];
    stream.read_exact(&mut hdr).expect("read header");
    assert_eq!(hdr[0], 0x5A);
    assert_eq!(hdr[1], 0x00);
    let ct = ((hdr[2] as u16) << 8) | (hdr[3] as u16);
    let cls = (ct >> 12) & 0x0F;
    let msg_type = ct & 0x0FFF;
    let len = ((hdr[4] as usize) << 8) | (hdr[5] as usize);
    let mut p = vec![0u8; len];
    stream.read_exact(&mut p).expect("read payload");
    let mut tr = [0u8; 1];
    stream.read_exact(&mut tr).expect("read trailer");
    assert_eq!(tr[0], 0x00);
    (cls, msg_type, p)
}

fn send_resp(stream: &mut TcpStream, cls: u16, msg_type: u16, payload: &[u8]) {
    let msg = message(cls, msg_type, payload);
    stream.write_all(&msg).expect("write response");
    stream.flush().expect("flush response");
}

#[test]
fn test_uvox_client_handshake_send_metadata_and_close() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind mock listener");
    let addr = listener.local_addr().unwrap();

    let server_thread = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("Accept failed");
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

        // 1) 0x1009 Cipher
        let (c, t, p) = read_msg(&mut stream);
        assert_eq!((c, t), (1, 0x009));
        assert_eq!(&p[..3], b"2.1");
        send_resp(&mut stream, 1, 0x009, b"ACK:foobar\0");

        // 2) 0x1001 Auth
        let (c, t, _) = read_msg(&mut stream);
        assert_eq!((c, t), (1, 0x001));
        send_resp(&mut stream, 1, 0x001, b"ACK\0");

        // 3) 0x1040 Mime
        let (c, t, _) = read_msg(&mut stream);
        assert_eq!((c, t), (1, 0x040));
        send_resp(&mut stream, 1, 0x040, b"ACK\0");

        // 4) 0x1002 Setup
        let (c, t, _) = read_msg(&mut stream);
        assert_eq!((c, t), (1, 0x002));
        send_resp(&mut stream, 1, 0x002, b"ACK\0");

        // 5) 0x1003 Buffer
        let (c, t, _) = read_msg(&mut stream);
        assert_eq!((c, t), (1, 0x003));
        send_resp(&mut stream, 1, 0x003, b"ACK\0");

        // 6) 0x1100 Icy-name
        let (c, t, _) = read_msg(&mut stream);
        assert_eq!((c, t), (1, 0x100));
        send_resp(&mut stream, 1, 0x100, b"ACK\0");

        // 7) 0x1101 Icy-genre
        let (c, t, _) = read_msg(&mut stream);
        assert_eq!((c, t), (1, 0x101));
        send_resp(&mut stream, 1, 0x101, b"ACK\0");

        // 8) 0x1102 Icy-url
        let (c, t, _) = read_msg(&mut stream);
        assert_eq!((c, t), (1, 0x102));
        send_resp(&mut stream, 1, 0x102, b"ACK\0");

        // 9) 0x1103 Icy-pub
        let (c, t, _) = read_msg(&mut stream);
        assert_eq!((c, t), (1, 0x103));
        send_resp(&mut stream, 1, 0x103, b"ACK\0");

        // 10) 0x1008 Max-payload
        let (c, t, _) = read_msg(&mut stream);
        assert_eq!((c, t), (1, 0x008));
        send_resp(&mut stream, 1, 0x008, b"ACK\0");

        // 11) 0x1004 Standby
        let (c, t, _) = read_msg(&mut stream);
        assert_eq!((c, t), (1, 0x004));
        send_resp(&mut stream, 1, 0x004, b"ACK:Data transfer mode\0");

        // 12) Data message (class 7 / type 0)
        let (c, t, p) = read_msg(&mut stream);
        assert_eq!((c, t), (7, 0x000));
        assert_eq!(p, vec![1, 2, 3, 4, 5]);

        // 13) Metadata message (class 3 / type 0x902)
        let (c, t, p) = read_msg(&mut stream);
        assert_eq!((c, t), (3, 0x902));
        let xml_str = String::from_utf8_lossy(&p[6..]);
        assert!(xml_str.contains("<TIT2>Artist - Test Track</TIT2>"));

        // 14) Close / Terminate message (class 1 / type 0x005)
        let (c, t, _) = read_msg(&mut stream);
        assert_eq!((c, t), (1, 0x005));
    });

    let mut client = UvoxClient::new(
        addr.ip().to_string(),
        addr.port(),
        1,
        "password123".to_string(),
        "audio/mpeg".to_string(),
        44_100,
        1,
        None,
    );

    client
        .connect(128, "Station Name", "Talk")
        .expect("Handshake failed");

    client.send(&[1, 2, 3, 4, 5]).expect("Send audio failed");
    client
        .update_now_playing("Artist - Test Track")
        .expect("Update now playing failed");

    client.close();

    server_thread.join().expect("Server thread panicked");
}

#[test]
fn test_icecast_client_sends_audio_verbatim_and_title_via_admin() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind mock listener");
    let addr = listener.local_addr().unwrap();

    let server_thread = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("Accept failed");
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

        // Read HTTP request headers
        let mut req_header = Vec::new();
        let mut buf = [0u8; 1];
        while stream.read_exact(&mut buf).is_ok() {
            req_header.push(buf[0]);
            if req_header.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let req_str = String::from_utf8_lossy(&req_header);
        assert!(req_str.starts_with("SOURCE /live ICE/1.0"));
        assert!(req_str.contains("Authorization: Basic"));
        assert!(req_str.contains("ice-name: Station Name"));
        // A source stream must not advertise inline metadata: harbor would
        // relay the metadata blocks to listeners as audio.
        assert!(!req_str.contains("icy-metaint"));

        stream.write_all(b"HTTP/1.0 200 OK\r\n\r\n").expect("write 200 OK");
        stream.flush().unwrap();

        // Audio arrives byte-for-byte, with nothing spliced in.
        let mut audio_buf = vec![0u8; 8192];
        stream.read_exact(&mut audio_buf).expect("read 8192 audio bytes");
        assert!(audio_buf.iter().all(|&b| b == 0xAA));

        // The title goes out-of-band on a second connection.
        let (mut admin, _) = listener.accept().expect("admin accept failed");
        admin.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut req = Vec::new();
        while admin.read_exact(&mut buf).is_ok() {
            req.push(buf[0]);
            if req.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let req = String::from_utf8_lossy(&req);
        assert!(req.starts_with("GET /admin/metadata?mode=updinfo"), "{req}");
        assert!(req.contains("mount=%2Flive&song=Icecast%20Test%20Title"), "{req}");
        admin.write_all(b"HTTP/1.0 200 OK\r\n\r\n").unwrap();
    });

    let mut client = IcecastSourceClient::new(
        addr.ip().to_string(),
        addr.port(),
        "live".to_string(),
        "source".to_string(),
        "hackme".to_string(),
        "audio/mpeg".to_string(),
    );

    client
        .connect(128, "Station Name", "Talk")
        .expect("Icecast connect failed");

    let audio = vec![0xAA; 8192];
    client.send(&audio).expect("Send audio failed");

    client
        .update_now_playing("Icecast Test Title")
        .expect("Update now playing failed");

    server_thread.join().expect("Server thread panicked");
    client.close();
}
