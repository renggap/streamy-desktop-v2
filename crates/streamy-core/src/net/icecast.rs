//! Icecast / Liquidsoap-harbor HTTP source client.
//! Live-verified: `SOURCE <mount> ICE/1.0` + HTTP Basic auth -> 200 OK.
//!
//! The audio body is sent verbatim. Titles go out-of-band via `/admin/metadata`
//! (supported by Icecast and Liquidsoap harbor alike): inline ICY metadata in a
//! *source* stream is not understood by harbor and would be relayed to listeners
//! as corrupt audio.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use crate::core::wire_safety::{header_value, parse_http_status, strip_control_chars, truncate, MAX_HEADER_VALUE, MAX_TITLE};
use crate::net::client::{
    connect_any, resolve, AbortHandle, SourceClient, SourceClientError, StationInfo,
    HANDSHAKE_TIMEOUT_MS, SEND_TIMEOUT_MS,
};

const USER_AGENT: &str = concat!("StreamyDesktop/", env!("CARGO_PKG_VERSION"));
/// Icecast's conventional source user, used when the profile leaves it blank.
const DEFAULT_SOURCE_USER: &str = "source";

pub struct IcecastSourceClient {
    host: String,
    port: u16,
    mount: String,
    user: String,
    password: String,
    mime: String,
    station: StationInfo,

    stream: Option<TcpStream>,
    abort: AbortHandle,
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
        let mount = normalize_mount(&mount);
        let user = if user.trim().is_empty() {
            DEFAULT_SOURCE_USER.to_string()
        } else {
            user.trim().to_string()
        };
        Self {
            host,
            port,
            mount,
            user,
            password,
            mime,
            station: StationInfo::default(),
            stream: None,
            abort: AbortHandle::default(),
        }
    }

    pub fn with_station(mut self, station: StationInfo) -> Self {
        self.station = station;
        self
    }

    fn auth_header(&self) -> String {
        BASE64.encode(format!("{}:{}", self.user, self.password))
    }

    fn read_status_line(stream: &mut TcpStream) -> Result<String, SourceClientError> {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line.is_empty() {
            return Err(SourceClientError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "server closed the connection during the handshake",
            )));
        }
        Ok(line.trim_end_matches(['\r', '\n']).to_string())
    }

    fn admin_update_metadata(&self, title: &str) {
        let host = self.host.clone();
        let port = self.port;
        let mount = self.mount.clone();
        let auth = self.auth_header();
        let title = title.to_string();

        std::thread::spawn(move || {
            let path = format!(
                "/admin/metadata?mode=updinfo&mount={}&song={}&charset=UTF-8",
                url_encode(&mount),
                url_encode(&title)
            );
            let addrs = match resolve(&host, port) {
                Ok(a) => a,
                Err(_) => return,
            };
            let mut s = match connect_any(&addrs) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("icecast metadata update: connect failed: {e}");
                    return;
                }
            };
            let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
            let _ = s.set_write_timeout(Some(Duration::from_secs(3)));

            let req = format!(
                "GET {} HTTP/1.0\r\nHost: {}:{}\r\nUser-Agent: {}\r\nAuthorization: Basic {}\r\nConnection: close\r\n\r\n",
                path, host, port, USER_AGENT, auth
            );
            let _ = s.write_all(req.as_bytes());
            let _ = s.flush();
            let mut buf = Vec::new();
            let _ = s.read_to_end(&mut buf);
            let status = String::from_utf8_lossy(&buf);
            let code = parse_http_status(status.lines().next().unwrap_or(""));
            if !(200..=299).contains(&code) {
                tracing::warn!("icecast metadata update rejected (HTTP {code})");
            }
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

        let addrs = resolve(&self.host, self.port)?;
        let mut s = connect_any(&addrs)?;
        self.abort.arm(&s);
        s.set_nodelay(true)?;
        s.set_read_timeout(Some(Duration::from_millis(HANDSHAKE_TIMEOUT_MS)))?;
        s.set_write_timeout(Some(Duration::from_millis(HANDSHAKE_TIMEOUT_MS)))?;

        let safe_mount = header_value(&self.mount, MAX_HEADER_VALUE);
        let safe_name = header_value(name, MAX_HEADER_VALUE);
        let safe_genre = header_value(genre, MAX_HEADER_VALUE);
        let safe_url = header_value(&self.station.url, MAX_HEADER_VALUE);

        let req = format!(
            "SOURCE {} ICE/1.0\r\n\
             Host: {}:{}\r\n\
             User-Agent: {}\r\n\
             Authorization: Basic {}\r\n\
             Content-Type: {}\r\n\
             ice-name: {}\r\n\
             ice-genre: {}\r\n\
             ice-url: {}\r\n\
             ice-public: {}\r\n\
             ice-bitrate: {}\r\n\
             icy-name: {}\r\n\
             icy-genre: {}\r\n\
             icy-url: {}\r\n\
             icy-pub: {}\r\n\
             icy-br: {}\r\n\
             \r\n",
            safe_mount,
            self.host,
            self.port,
            USER_AGENT,
            self.auth_header(),
            self.mime,
            safe_name,
            safe_genre,
            safe_url,
            self.station.public as u8,
            bitrate_kbps,
            safe_name,
            safe_genre,
            safe_url,
            self.station.public as u8,
            bitrate_kbps,
        );

        s.write_all(req.as_bytes())?;
        s.flush()?;

        let status = Self::read_status_line(&mut s)?;
        let code = parse_http_status(&status);
        if !(200..=299).contains(&code) {
            let reason = match code {
                401 | 403 => "auth rejected",
                -1 => "handshake rejected (not an Icecast/harbor server?)",
                _ => "source rejected",
            };
            return Err(SourceClientError::Protocol(format!("{reason}: {status}")));
        }

        // Streaming: reads stay unblocked, writes are bounded so a stalled
        // server cannot pin the client mutex forever.
        s.set_read_timeout(None)?;
        s.set_write_timeout(Some(Duration::from_millis(SEND_TIMEOUT_MS)))?;

        self.stream = Some(s);
        Ok(())
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), SourceClientError> {
        let s = self.stream.as_mut().ok_or(SourceClientError::Closed)?;
        s.write_all(frame)?;
        s.flush()?;
        Ok(())
    }

    fn update_now_playing(&mut self, title: &str) -> Result<(), SourceClientError> {
        let safe = strip_control_chars(title);
        let safe_capped = truncate(&safe, MAX_TITLE);
        self.admin_update_metadata(&safe_capped);
        Ok(())
    }

    fn abort_handle(&self) -> AbortHandle {
        self.abort.clone()
    }

    fn close(&mut self) {
        self.abort.disarm();
        if let Some(mut s) = self.stream.take() {
            let _ = s.flush();
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

impl Drop for IcecastSourceClient {
    fn drop(&mut self) {
        self.close();
    }
}

/// Mounts must be absolute paths; users often type `live` instead of `/live`.
fn normalize_mount(mount: &str) -> String {
    let m = mount.trim();
    if m.is_empty() {
        "/".to_string()
    } else if m.starts_with('/') {
        m.to_string()
    } else {
        format!("/{m}")
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

    #[test]
    fn mount_is_normalized_to_absolute_path() {
        assert_eq!(normalize_mount("live"), "/live");
        assert_eq!(normalize_mount(" /live "), "/live");
        assert_eq!(normalize_mount(""), "/");
    }

    #[test]
    fn blank_user_defaults_to_source() {
        let c = IcecastSourceClient::new(
            "h".into(), 1, "/m".into(), "  ".into(), "p".into(), "audio/mpeg".into(),
        );
        assert_eq!(c.user, "source");
    }

    #[test]
    fn update_now_playing_long_emoji_title_does_not_panic() {
        let mut c = IcecastSourceClient::new(
            "h".into(), 1, "/m".into(), "".into(), "p".into(), "audio/mpeg".into(),
        );
        // The truncation path is the part pinned against a byte-slice panic.
        let title: String = "\u{1F3A4}".repeat(400);
        assert!(c.update_now_playing(&title).is_ok());
    }
}
