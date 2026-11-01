//! Icecast / Liquidsoap-harbor HTTP source client.
//! Live-verified: `SOURCE <mount> ICE/1.0` + HTTP Basic auth -> 200 OK.
//!
//! The audio body is sent verbatim. Titles go out-of-band via `/admin/metadata`
//! (supported by Icecast and Liquidsoap harbor alike): inline ICY metadata in a
//! *source* stream is not understood by harbor and would be relayed to listeners
//! as corrupt audio.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use crate::core::wire_safety::{header_value, parse_http_status, strip_control_chars, truncate, MAX_HEADER_VALUE, MAX_TITLE};
use crate::net::client::{
    admin_get, connect_any, resolve, AbortHandle, SourceClient, SourceClientError, StationInfo,
    TitleAck, TitleAckFn, HANDSHAKE_TIMEOUT_MS, SEND_TIMEOUT_MS,
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
    /// Where the server's verdict on a title push is reported (an accepted
    /// /admin/metadata 2xx is what the server really acted on).
    title_ack: Option<TitleAckFn>,
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
            title_ack: None,
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

    /// Admin metadata update (HTTP GET), synchronous here: callers bound it
    /// (the transport worker between frames, ADMIN_TIMEOUT_MS inside). Reports
    /// the server's real verdict through the ack callback on completion.
    fn admin_update_metadata(&self, title: &str) {
        let path = format!(
            "/admin/metadata?mode=updinfo&charset=UTF-8&mount={}&song={}",
            crate::net::shoutcast_v1::url_encode(&self.mount),
            crate::net::shoutcast_v1::url_encode(title)
        );
        let reply = admin_get(
            &self.host,
            self.port,
            &path,
            &[("Authorization", &format!("Basic {}", self.auth_header()))],
        );
        if !reply.ok() {
            tracing::warn!("icecast metadata update rejected (HTTP {})", reply.code);
        }
        if let Some(ack) = self.title_ack.as_ref() {
            ack(TitleAck { title: title.to_string(), ok: reply.ok() });
        }
    }
}

impl SourceClient for IcecastSourceClient {
    fn clone_box(&self) -> Box<dyn SourceClient> {
        Box::new(IcecastSourceClient {
            host: self.host.clone(),
            port: self.port,
            mount: self.mount.clone(),
            user: self.user.clone(),
            password: self.password.clone(),
            mime: self.mime.clone(),
            station: self.station.clone(),
            stream: None,
            abort: AbortHandle::default(),
            title_ack: None,
        })
    }
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

    /// Push now-playing via the Icecast admin GET. Reports the server's real
    /// verdict through the ack callback: `true` only on a 2xx — the server's
    /// actual acceptance (what BUTT/Mixxx treat as success). Only while the
    /// source is up: before that there's no mount to update (connect() replays
    /// it) and no verdict is reported. Failures are `false`, never faked.
    fn update_now_playing(&mut self, title: &str) -> Result<(), SourceClientError> {
        let safe = strip_control_chars(title);
        let safe_capped = truncate(&safe, MAX_TITLE);
        if self.stream.is_some() {
            self.admin_update_metadata(&safe_capped);
        }
        Ok(())
    }

    fn set_title_ack(&mut self, ack: TitleAckFn) {
        self.title_ack = Some(ack);
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

// Query encoding now lives in shoutcast_v1::url_encode (shared, canonical %20).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::shoutcast_v1::url_encode;

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
