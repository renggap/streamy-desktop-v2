//! Icecast / Liquidsoap-harbor HTTP source client.
//! Live-verified: `SOURCE <mount> ICE/1.0` + HTTP Basic auth -> 200 OK.
//! Features ICY inline metadata every 8192 bytes + best-effort /admin/metadata update.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use parking_lot::Mutex;

use crate::core::wire_safety::{header_value, parse_http_status, strip_control_chars, MAX_HEADER_VALUE, MAX_TITLE};
use crate::net::client::{SourceClient, SourceClientError};

const HANDSHAKE_TIMEOUT_MS: u64 = 6_000;
const METAINT: usize = 8_192;

pub struct IcecastSourceClient {
    host: String,
    port: u16,
    mount: String,
    user: String,
    password: String,
    mime: String,

    stream: Option<TcpStream>,
    meta_state: Arc<IcyState>,
}

struct IcyState {
    title: Mutex<String>,
    bytes_since_meta: AtomicUsize,
}

impl IcecastSourceClient {
    pub fn new(
        host: String,
        port: u16,
        mount: String,
        user: String,
        password: String,
        mime: String,
    ) -> Self {
        Self {
            host,
            port,
            mount,
            user,
            password,
            mime,
            stream: None,
            meta_state: Arc::new(IcyState {
                title: Mutex::new(String::new()),
                bytes_since_meta: AtomicUsize::new(0),
            }),
        }
    }

    fn read_status_line(stream: &mut TcpStream) -> Result<String, SourceClientError> {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line)?;
        Ok(line.trim_end_matches(['\r', '\n']).to_string())
    }

    fn admin_update_metadata(&self, title: &str) {
        let host = self.host.clone();
        let port = self.port;
        let mount = self.mount.clone();
        let user = self.user.clone();
        let password = self.password.clone();
        let title = title.to_string();

        std::thread::spawn(move || {
            let encoded_mount = url_encode(&mount);
            let encoded_song = url_encode(&title);
            let path = format!("/admin/metadata?mode=updinfo&mount={}&song={}", encoded_mount, encoded_song);
            let auth = BASE64.encode(format!("{}:{}", user, password));

            let addrs: Vec<SocketAddr> = match format!("{}:{}", host, port).to_socket_addrs() {
                Ok(a) => a.collect(),
                Err(_) => return,
            };
            if addrs.is_empty() {
                return;
            }

            let mut s = match TcpStream::connect_timeout(&addrs[0], Duration::from_secs(3)) {
                Ok(s) => s,
                Err(_) => return,
            };
            let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
            let _ = s.set_write_timeout(Some(Duration::from_secs(3)));

            let req = format!(
                "GET {} HTTP/1.0\r\nHost: {}:{}\r\nAuthorization: Basic {}\r\nConnection: close\r\n\r\n",
                path, host, port, auth
            );
            let _ = s.write_all(req.as_bytes());
            let _ = s.flush();
            let mut buf = Vec::new();
            let _ = s.read_to_end(&mut buf);
        });
    }
}

impl SourceClient for IcecastSourceClient {
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

        let auth = BASE64.encode(format!("{}:{}", self.user, self.password));
        let safe_mount = header_value(&self.mount, MAX_HEADER_VALUE);
        let safe_name = header_value(name, MAX_HEADER_VALUE);
        let safe_genre = header_value(genre, MAX_HEADER_VALUE);

        let req = format!(
            "SOURCE {} ICE/1.0\r\n\
             Authorization: Basic {}\r\n\
             Content-Type: {}\r\n\
             icy-name: {}\r\n\
             icy-genre: {}\r\n\
             icy-url: http://streamy.app\r\n\
             icy-pub: 1\r\n\
             icy-br: {}\r\n\
             icy-metaint: {}\r\n\
             \r\n",
            safe_mount, auth, self.mime, safe_name, safe_genre, bitrate_kbps, METAINT
        );

        s.write_all(req.as_bytes())?;
        s.flush()?;

        let status = Self::read_status_line(&mut s)?;
        let code = parse_http_status(&status);
        if !(200..=299).contains(&code) {
            return Err(SourceClientError::Protocol(format!(
                "icecast source rejected: {}",
                status
            )));
        }

        s.set_read_timeout(None)?;
        s.set_write_timeout(None)?;

        self.meta_state.bytes_since_meta.store(0, Ordering::SeqCst);
        self.stream = Some(s);
        Ok(())
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), SourceClientError> {
        let s = self.stream.as_mut().ok_or(SourceClientError::Closed)?;
        let mut offset = 0;
        let mut remaining = frame.len();

        while remaining > 0 {
            let current_count = self.meta_state.bytes_since_meta.load(Ordering::SeqCst);
            let to_write = remaining.min(METAINT - current_count);

            s.write_all(&frame[offset..offset + to_write])?;
            offset += to_write;
            remaining -= to_write;

            let new_count = current_count + to_write;
            if new_count >= METAINT {
                Self::write_metadata_block_internal(&self.meta_state, s)?;
                self.meta_state.bytes_since_meta.store(0, Ordering::SeqCst);
            } else {
                self.meta_state.bytes_since_meta.store(new_count, Ordering::SeqCst);
            }
        }

        s.flush()?;
        Ok(())
    }

    fn update_now_playing(&mut self, title: &str) -> Result<(), SourceClientError> {
        let safe = strip_control_chars(title);
        let safe_capped = if safe.len() > MAX_TITLE {
            safe[..MAX_TITLE].to_string()
        } else {
            safe
        };

        {
            let mut t = self.meta_state.title.lock();
            *t = safe_capped.clone();
        }

        self.admin_update_metadata(&safe_capped);
        Ok(())
    }

    fn close(&mut self) {
        if let Some(mut s) = self.stream.take() {
            let _ = s.flush();
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

impl IcecastSourceClient {
    fn write_metadata_block_internal(meta_state: &Arc<IcyState>, stream: &mut TcpStream) -> Result<(), SourceClientError> {
        let title = meta_state.title.lock().clone();
        if title.is_empty() {
            stream.write_all(&[0x00])?;
            return Ok(());
        }

        let escaped = title.replace('\\', "\\\\").replace('\'', "\\'");
        let raw = format!("StreamTitle='{}';", escaped);
        let raw_bytes = raw.as_bytes();
        let padded_len = ((raw_bytes.len() + 15) / 16) * 16;
        let len_byte = (padded_len / 16) as u8;

        let mut block = vec![0u8; 1 + padded_len];
        block[0] = len_byte;
        block[1..1 + raw_bytes.len()].copy_from_slice(raw_bytes);

        stream.write_all(&block)?;
        Ok(())
    }
}

impl Drop for IcecastSourceClient {
    fn drop(&mut self) {
        self.close();
    }
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_encode() {
        assert_eq!(url_encode("Hello World"), "Hello%20World");
        assert_eq!(url_encode("/stream"), "%2Fstream");
    }
}
