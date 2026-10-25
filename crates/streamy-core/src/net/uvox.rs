//! SHOUTcast 2 / Ultravox 2.1 source client.
//! Live-verified against DNAS2 2.6.1 for BOTH audio/mpeg and audio/aacp.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::core::framing::{adts_header, message};
use crate::core::wire_safety::{header_value, strip_control_chars, MAX_HEADER_VALUE};
use crate::core::xtea::encipher;
use crate::net::client::{SourceClient, SourceClientError};

const HANDSHAKE_TIMEOUT_MS: u64 = 6_000;
const CLOSE_JOIN_MS: u64 = 500;
const ICY_PUB: &str = "1";

pub struct UvoxClient {
    host: String,
    port: u16,
    sid: u32,
    password: String,
    mime: String,
    sample_rate_hz: u32,
    channels: u8,
    dj_user: Option<String>,

    data_class: u16,
    data_type: u16,

    stream: Option<TcpStream>,
}

impl UvoxClient {
    pub fn new(
        host: String,
        port: u16,
        sid: u32,
        password: String,
        mime: String,
        sample_rate_hz: u32,
        channels: u8,
        dj_user: Option<String>,
    ) -> Self {
        let (data_class, data_type) = if mime == "audio/aacp" || mime == "audio/aac" {
            (8, 0x003) // AAC / AACP
        } else {
            (7, 0x000) // MP3
        };

        Self {
            host,
            port,
            sid,
            password,
            mime,
            sample_rate_hz,
            channels,
            dj_user,
            data_class,
            data_type,
            stream: None,
        }
    }

    fn read_payload(stream: &mut TcpStream) -> Result<String, SourceClientError> {
        let mut header = [0u8; 6];
        stream.read_exact(&mut header)?;
        let len = (((header[4] as usize) & 0xFF) << 8) | ((header[5] as usize) & 0xFF);

        let mut payload = vec![0u8; len];
        stream.read_exact(&mut payload)?;

        let mut trailer = [0u8; 1];
        stream.read_exact(&mut trailer)?;

        let text = String::from_utf8_lossy(&payload)
            .trim_end_matches('\0')
            .to_string();
        Ok(text)
    }

    fn request(
        stream: &mut TcpStream,
        cls: u16,
        msg_type: u16,
        payload: Option<&str>,
    ) -> Result<String, SourceClientError> {
        let p = match payload {
            Some(s) => {
                let mut v = s.as_bytes().to_vec();
                v.push(0x00); // ASCIIZ null terminator
                v
            }
            None => Vec::new(),
        };
        let msg = message(cls, msg_type, &p);
        stream.write_all(&msg)?;
        stream.flush()?;
        Self::read_payload(stream)
    }

    fn require_ack(resp: &str, label: &str) -> Result<(), SourceClientError> {
        if !resp.starts_with("ACK") {
            Err(SourceClientError::Protocol(format!("{label} NAK: {resp}")))
        } else {
            Ok(())
        }
    }
}

impl SourceClient for UvoxClient {
    fn connect(
        &mut self,
        bitrate_kbps: u32,
        name: &str,
        genre: &str,
    ) -> Result<(), SourceClientError> {
        self.close();

        let addrs: Vec<SocketAddr> = format!("{}:{}", self.host, self.port)
            .to_socket_addrs()
            .map_err(|e| SourceClientError::Io(e))?
            .collect();
        if addrs.is_empty() {
            return Err(SourceClientError::Protocol("Address resolution failed".into()));
        }

        let mut s = TcpStream::connect_timeout(
            &addrs[0],
            Duration::from_millis(HANDSHAKE_TIMEOUT_MS),
        )?;
        s.set_nodelay(true)?;
        s.set_read_timeout(Some(Duration::from_millis(HANDSHAKE_TIMEOUT_MS)))?;
        s.set_write_timeout(Some(Duration::from_millis(HANDSHAKE_TIMEOUT_MS)))?;

        // 1) Request Cipher -> key
        let cipher_resp = Self::request(&mut s, 1, 0x009, Some("2.1"))?;
        if !cipher_resp.starts_with("ACK") {
            return Err(SourceClientError::Protocol(format!(
                "cipher NAK: {}",
                cipher_resp
            )));
        }
        let cipher_key_str = cipher_resp
            .strip_prefix("ACK:")
            .unwrap_or("")
            .trim_end_matches('\0');
        let cipher_key = cipher_key_str.as_bytes();

        // 2) Authenticate: UID = djlogin (DJ mode) or blank (SOURCE)
        let uid = self.dj_user.as_deref().unwrap_or("");
        let enc_uid = encipher(uid.as_bytes(), cipher_key);
        let enc_blob = encipher(self.password.as_bytes(), cipher_key);
        let auth_payload = format!("2.1:{}:{}:{}", self.sid, enc_uid, enc_blob);
        let auth_resp = Self::request(&mut s, 1, 0x001, Some(&auth_payload))?;
        if !auth_resp.starts_with("ACK") {
            return Err(SourceClientError::Protocol(format!(
                "auth NAK: {}",
                auth_resp
            )));
        }

        // 3) Stream config
        Self::require_ack(&Self::request(&mut s, 1, 0x040, Some(&self.mime))?, "mime")?;
        Self::require_ack(
            &Self::request(
                &mut s,
                1,
                0x002,
                Some(&format!("{}:{}", bitrate_kbps, bitrate_kbps)),
            )?,
            "setup",
        )?;
        Self::require_ack(
            &Self::request(&mut s, 1, 0x003, Some("2048:2048"))?,
            "buffer",
        )?;
        Self::require_ack(
            &Self::request(
                &mut s,
                1,
                0x100,
                Some(&header_value(name, MAX_HEADER_VALUE)),
            )?,
            "icy-name",
        )?;
        Self::require_ack(
            &Self::request(
                &mut s,
                1,
                0x101,
                Some(&header_value(genre, MAX_HEADER_VALUE)),
            )?,
            "icy-genre",
        )?;
        Self::require_ack(
            &Self::request(&mut s, 1, 0x102, Some("http://streamy.app"))?,
            "icy-url",
        )?;
        Self::require_ack(
            &Self::request(&mut s, 1, 0x103, Some(ICY_PUB))?,
            "icy-pub",
        )?;
        Self::require_ack(
            &Self::request(&mut s, 1, 0x008, Some("16377:16377"))?,
            "max-payload",
        )?;

        // 4) Standby
        let standby = Self::request(&mut s, 1, 0x004, None)?;
        if !standby.starts_with("ACK") {
            return Err(SourceClientError::Protocol(format!(
                "standby NAK: {}",
                standby
            )));
        }

        // Streaming: writes own the socket; no read timeout
        s.set_read_timeout(None)?;
        s.set_write_timeout(None)?;

        self.stream = Some(s);
        Ok(())
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), SourceClientError> {
        let s = self.stream.as_mut().ok_or(SourceClientError::Closed)?;
        let msg = if self.data_class == 8 {
            // AAC requires ADTS header declaring AAC-LC core rate (sample_rate_hz / 2)
            let adts = adts_header(frame, self.sample_rate_hz / 2, self.channels);
            message(self.data_class, self.data_type, &adts)
        } else {
            message(self.data_class, self.data_type, frame)
        };
        s.write_all(&msg)?;
        s.flush()?;
        Ok(())
    }

    fn update_now_playing(&mut self, title: &str) -> Result<(), SourceClientError> {
        let s = match self.stream.as_mut() {
            Some(s) => s,
            None => return Ok(()),
        };

        let safe = strip_control_chars(title);
        let safe_capped = if safe.len() > MAX_HEADER_VALUE {
            &safe[..MAX_HEADER_VALUE]
        } else {
            &safe
        };
        let xml_esc = safe_capped
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" ?><metadata><TIT2>{}</TIT2></metadata>",
            xml_esc
        );
        let xml_bytes = xml.as_bytes();

        let mut payload = vec![0u8; 6 + xml_bytes.len()];
        // MetadataID = 1, Span = 1, Index = 1 (BE u16)
        payload[1] = 1;
        payload[3] = 1;
        payload[5] = 1;
        payload[6..].copy_from_slice(xml_bytes);

        let msg = message(3, 0x902, &payload);
        let _ = s.write_all(&msg);
        let _ = s.flush();
        Ok(())
    }

    fn close(&mut self) {
        if let Some(s) = self.stream.take() {
            // Bounded 500ms join for terminate message
            let done = Arc::new(AtomicBool::new(false));
            let done_clone = done.clone();

            let mut s_clone = s.try_clone().ok();
            thread::spawn(move || {
                if let Some(ref mut cl) = s_clone {
                    let term = message(1, 0x005, &[]);
                    let _ = cl.write_all(&term);
                    let _ = cl.flush();
                }
                done_clone.store(true, Ordering::SeqCst);
            });

            let start = std::time::Instant::now();
            while !done.load(Ordering::SeqCst) && start.elapsed() < Duration::from_millis(CLOSE_JOIN_MS) {
                thread::sleep(Duration::from_millis(10));
            }

            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

impl Drop for UvoxClient {
    fn drop(&mut self) {
        self.close();
    }
}
