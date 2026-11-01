//! SHOUTcast **v1** source client — the legacy DJ login sc_trans2 exposes and Centova
//! panels advertise as "Protocol: Shoutcast (v1)". Live-verified on Android against
//! centova1.radioislam.my.id (sc_trans2): the panel lists port 8092, the login is
//! accepted on 8092 + 1 = 8093.
//!
//! Wire: one text line `djlogin:djpassword\r\n` (or just the password when the user
//! is blank) -> `OK2\r\nicy-caps:11\r\n\r\n` on success, `invalid password\r\n` on a
//! bad login. Then `icy-*` header lines + a blank line, then raw MP3 bytes. No
//! framing, no cipher.
//!
//! The port given to [new](ShoutcastV1Client::new) is the port this client DIALS —
//! v1 sources connect to the panel/listener port + 1; the caller does that arithmetic.
//! Now-playing goes out of band, the classic v1 way: an HTTP
//! `GET /admin.cgi?mode=updinfo&pass=…&song=…` to `admin_port` (the panel port).
//! sc_trans2's Weblet answers `Title Updated` with the DJ's own login — no server
//! admin password needed (Basic auth instead of `pass=` answers 404). The `song=`
//! value is percent-encoded with `%20` for spaces — sc_trans percent-decodes but
//! keeps a literal `+`, so form encoding aired "Kajian+Pagi" verbatim (live-proven
//! against TRANSCast 2.0.0.54).

use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

use crate::core::wire_safety::{header_value, strip_control_chars, MAX_HEADER_VALUE};
use crate::net::client::{
    admin_get, connect_any, read_crlf_line, resolve, AbortHandle, SourceClient,
    SourceClientError, TitleAck, TitleAckFn, HANDSHAKE_TIMEOUT_MS, SEND_TIMEOUT_MS,
};

/// After `OK2`, drain the remaining header block for at most this long.
const DRAIN_TIMEOUT_MS: u64 = 500;

pub struct ShoutcastV1Client {
    host: String,
    /// The port this client DIALS (panel port + 1 for v1 sources).
    port: u16,
    user: String,
    password: String,
    /// The panel port for out-of-band `admin.cgi` title updates.
    admin_port: u16,

    stream: Option<TcpStream>,
    abort: AbortHandle,
    /// Last now-playing title, pushed again after every successful login so a
    /// reconnect (or a title set while still connecting) still reaches the server.
    last_title: Option<String>,
    title_ack: Option<TitleAckFn>,
}

impl ShoutcastV1Client {
    pub fn new(host: String, port: u16, user: String, password: String, admin_port: u16) -> Self {
        Self {
            host,
            port,
            user,
            password,
            admin_port,
            stream: None,
            abort: AbortHandle::default(),
            last_title: None,
            title_ack: None,
        }
    }

    /// Pure: the v1 login line — `user:password`, or the bare password when there's no user.
    pub fn login_line(user: &str, password: &str) -> String {
        format!("{}\r\n", credential(user, password))
    }

    /// Pure: the admin.cgi title-update path; `pass` is the same credential as the login.
    pub fn admin_path(user: &str, password: &str, title: &str) -> String {
        format!(
            "/admin.cgi?mode=updinfo&pass={}&song={}",
            url_encode(&credential(user, password)),
            url_encode(title)
        )
    }
}

/// Canonical percent-encoding for a query-parameter value: spaces as `%20` — NEVER `+`.
/// sc_trans's Weblet percent-decodes but keeps a literal `+`, so a form-encoded title
/// aired as "Kajian+Pagi" instead of "Kajian Pagi" (live-proven 2026-10-31 against
/// TRANSCast 2.0.0.54). Harmless for handlers that do form-decode (`%20` is a space
/// everywhere), so this is safe for the Icecast/Liquidsoap `/admin/metadata` path too.
pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in strip_control_chars(s).bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Pure: `user:password`, or the bare password when the user is blank.
fn credential(user: &str, password: &str) -> String {
    let u = strip_control_chars(user);
    let p = strip_control_chars(password);
    if u.trim().is_empty() {
        p
    } else {
        format!("{u}:{p}")
    }
}

/// The admin.cgi push itself. Reports the Weblet's real verdict through `ack`:
/// `true` only on HTTP 200 + `Title Updated` (live-proven: a wrong password
/// answers 404, no DJ session answers 404). Shared by the live push and the
/// off-thread post-login replay.
fn push_title_via_admin(
    host: &str,
    admin_port: u16,
    user: &str,
    password: &str,
    title: &str,
    ack: Option<&TitleAckFn>,
) {
    let reply = admin_get(
        host,
        admin_port,
        &ShoutcastV1Client::admin_path(user, password, title),
        &[],
    );
    let ok = reply.code == 200 && reply.body.to_lowercase().contains("title updated");
    if let Some(ack) = ack {
        ack(TitleAck { title: title.to_string(), ok });
    }
}

impl SourceClient for ShoutcastV1Client {
    fn clone_box(&self) -> Box<dyn SourceClient> {
        Box::new(ShoutcastV1Client {
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            password: self.password.clone(),
            admin_port: self.admin_port,
            stream: None,
            abort: AbortHandle::default(),
            last_title: None,
            title_ack: None,
        })
    }
    fn connect(
        &mut self,
        bitrate_kbps: u32,
        name: &str,
        genre: &str,
    ) -> Result<(), SourceClientError> {
        // Drop any previous socket (the transport reconnect loop calls this on us).
        self.close_socket();

        let addrs = resolve(&self.host, self.port)?;
        let mut s = connect_any(&addrs)?;
        self.abort.arm(&s);
        s.set_nodelay(true)?;
        s.set_read_timeout(Some(Duration::from_millis(HANDSHAKE_TIMEOUT_MS)))?;
        s.set_write_timeout(Some(Duration::from_millis(HANDSHAKE_TIMEOUT_MS)))?;

        let result = self.handshake(&mut s, bitrate_kbps, name, genre);
        match result {
            Ok(()) => {
                // Streaming: writes bounded like the other clients so a stalled
                // server cannot pin the client mutex forever — that would
                // deadlock stop_and_shutdown (Android relies on the socket
                // close in close(); the shared-core teardown waits on us).
                s.set_read_timeout(None)?;
                s.set_write_timeout(Some(Duration::from_millis(SEND_TIMEOUT_MS)))?;
                self.stream = Some(s);
                // A fresh login has no title yet: re-announce the current one
                // (off-thread: a slow admin port must not hold the connect).
                if let Some(t) = self.last_title.clone() {
                    let host = self.host.clone();
                    let admin_port = self.admin_port;
                    let (user, password) = (self.user.clone(), self.password.clone());
                    let ack = self.title_ack.clone();
                    std::thread::spawn(move || {
                        push_title_via_admin(&host, admin_port, &user, &password, &t, ack.as_ref());
                    });
                }
                Ok(())
            }
            Err(e) => {
                let _ = s.shutdown(std::net::Shutdown::Both);
                Err(e)
            }
        }
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), SourceClientError> {
        let s = self.stream.as_mut().ok_or(SourceClientError::Closed)?;
        s.write_all(frame)?;
        s.flush()?;
        Ok(())
    }

    /// Out-of-band title update; the verdict is reported via the ack callback
    /// (see [push_title_via_admin]). Not logged in -> no verdict, replayed after
    /// the next successful handshake.
    ///
    /// The admin GET runs OFF-THREAD: this is called from the transport worker
    /// between audio frames, and a slow or dead Weblet must never stall the
    /// audio (up to ~9s of connect+read timeouts otherwise).
    fn update_now_playing(&mut self, title: &str) -> Result<(), SourceClientError> {
        let safe = header_value(title, MAX_HEADER_VALUE);
        self.last_title = Some(safe.clone());
        if self.stream.is_some() {
            let host = self.host.clone();
            let admin_port = self.admin_port;
            let (user, password) = (self.user.clone(), self.password.clone());
            let ack = self.title_ack.clone();
            let t = safe.clone();
            std::thread::spawn(move || {
                push_title_via_admin(&host, admin_port, &user, &password, &t, ack.as_ref());
            });
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
        self.close_socket();
    }
}

impl ShoutcastV1Client {
    /// The v1 login + `icy-*` header exchange. On failure the caller shuts the socket.
    fn handshake(
        &self,
        s: &mut TcpStream,
        bitrate_kbps: u32,
        name: &str,
        genre: &str,
    ) -> Result<(), SourceClientError> {
        // CR/LF/NUL stripped: a credential can't smuggle a second line into the login.
        s.write_all(Self::login_line(&self.user, &self.password).as_bytes())?;
        s.flush()?;

        let status = read_crlf_line(s)?;
        if !status.starts_with("OK") {
            // "invalid password" is a real v1 DJ port refusing the login; anything
            // else (an HTTP status line, EOF) means this isn't a v1 port at all.
            // The "auth NAK" prefix lets the UI and the fallback tell the two apart.
            let lower = status.to_lowercase();
            let reason = if lower.contains("invalid password") {
                format!("auth NAK: {status}")
            } else if status.is_empty() {
                "v1 NAK: <no reply>".to_string()
            } else {
                format!("v1 NAK: {status}")
            };
            return Err(SourceClientError::Protocol(reason));
        }

        // Drain the rest of the OK2 header block (icy-caps:…) up to its blank line.
        // Short cap: a server that ends after the status line must not stall the
        // start for the full handshake timeout.
        let _ = s.set_read_timeout(Some(Duration::from_millis(DRAIN_TIMEOUT_MS)));
        loop {
            match read_crlf_line(s) {
                Ok(line) if line.is_empty() => break,
                Ok(_) => continue,
                Err(_) => break, // timeout/EOF: header block ended without a blank line
            }
        }

        let headers = format!(
            "icy-name:{}\r\nicy-genre:{}\r\nicy-url:http://streamy.app\r\nicy-pub:1\r\nicy-br:{}\r\ncontent-type:audio/mpeg\r\n\r\n",
            header_value(name, MAX_HEADER_VALUE),
            header_value(genre, MAX_HEADER_VALUE),
            bitrate_kbps,
        );
        s.write_all(headers.as_bytes())?;
        s.flush()?;
        Ok(())
    }

    fn close_socket(&mut self) {
        if let Some(mut s) = self.stream.take() {
            let _ = s.flush();
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

impl Drop for ShoutcastV1Client {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_line_is_bare_password_without_user() {
        assert_eq!(ShoutcastV1Client::login_line("", "pw"), "pw\r\n");
        assert_eq!(ShoutcastV1Client::login_line("dj", "pw"), "dj:pw\r\n");
    }

    #[test]
    fn admin_path_encodes_spaces_as_percent20_never_plus() {
        // sc_trans percent-decodes but keeps a literal '+': "Kajian+Pagi" aired
        // verbatim (live-proven against TRANSCast 2.0.0.54).
        let p = ShoutcastV1Client::admin_path("dj", "p w", "Kajian Pagi");
        assert!(p.contains("pass=dj%3Ap%20w"), "got: {p}");
        assert!(p.contains("song=Kajian%20Pagi"), "got: {p}");
        assert!(!p.contains('+'));
    }

    #[test]
    fn credential_strips_control_chars() {
        assert_eq!(credential("dj\r\nx", "pw"), "djx:pw");
        assert_eq!(credential("", "pw"), "pw");
    }
}
