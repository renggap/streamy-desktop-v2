//! Tries the primary, and on any connect failure tries the fallback — used for DJ
//! profiles so one setting covers the SHOUTcast DJ port styles seen live:
//! Centova/sc_trans2 panels that advertise a v1 login (primary: ShoutcastV1Client
//! on the listed port + 1), servers that speak Ultravox on the listed port, and
//! raw sc_trans djport2 setups whose Ultravox DJ port sits at the listed port + 2
//! (the Centova convention: panel 8092, v1 on 8093, Ultravox DJ on 8094).
//!
//! Sticky: once a protocol connects, reconnects use it alone. Flapping between
//! protocols on a flaky link would mean two different DJ sessions taking over the
//! station in turn.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::net::client::{
    AbortHandle, SourceClient, SourceClientError, TitleAckFn,
};

/// Which child is live: 0 = unset, 1 = primary, 2 = fallback.
const NONE: usize = 0;
const PRIMARY: usize = 1;
const FALLBACK: usize = 2;

pub struct FallbackSourceClient {
    primary: Box<dyn SourceClient>,
    fallback: Box<dyn SourceClient>,
    /// One AbortHandle per child, cloned once at construction (both are cheap).
    primary_abort: AbortHandle,
    fallback_abort: AbortHandle,
    active: AtomicUsize,
    closed: AtomicBool,
}

impl FallbackSourceClient {
    pub fn new(primary: Box<dyn SourceClient>, fallback: Box<dyn SourceClient>) -> Self {
        let primary_abort = primary.abort_handle();
        let fallback_abort = fallback.abort_handle();
        Self {
            primary,
            fallback,
            primary_abort,
            fallback_abort,
            active: AtomicUsize::new(NONE),
            closed: AtomicBool::new(false),
        }
    }

    fn active_client(&mut self) -> Option<&mut dyn SourceClient> {
        match self.active.load(Ordering::SeqCst) {
            PRIMARY => Some(self.primary.as_mut()),
            FALLBACK => Some(self.fallback.as_mut()),
            _ => None,
        }
    }
}

impl SourceClient for FallbackSourceClient {
    fn clone_box(&self) -> Box<dyn SourceClient> {
        Box::new(FallbackSourceClient::new(
            self.primary.clone_box(),
            self.fallback.clone_box(),
        ))
    }
    fn connect(
        &mut self,
        bitrate_kbps: u32,
        name: &str,
        genre: &str,
    ) -> Result<(), SourceClientError> {
        if let Some(active) = self.active_client() {
            return active.connect(bitrate_kbps, name, genre);
        }
        let first = match self.primary.connect(bitrate_kbps, name, genre) {
            Ok(()) => {
                self.active.store(PRIMARY, Ordering::SeqCst);
                return Ok(());
            }
            Err(e) => e,
        };
        if self.closed.load(Ordering::SeqCst) {
            // Stop arrived mid-attempt: don't start a second handshake.
            return Err(first);
        }
        match self.fallback.connect(bitrate_kbps, name, genre) {
            Ok(()) => {
                self.active.store(FALLBACK, Ordering::SeqCst);
                Ok(())
            }
            Err(second) => Err(pick_error(first, second)),
        }
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), SourceClientError> {
        self.active_client()
            .ok_or(SourceClientError::Closed)?
            .send(frame)
    }

    /// Both children get the title: whichever connects keeps it for replay after
    /// its handshake. Each child reports its own verdict through its ack channel.
    fn update_now_playing(&mut self, title: &str) -> Result<(), SourceClientError> {
        self.primary.update_now_playing(title)?;
        self.fallback.update_now_playing(title)
    }

    fn set_title_ack(&mut self, ack: TitleAckFn) {
        self.primary.set_title_ack(ack.clone());
        self.fallback.set_title_ack(ack);
    }

    /// Merged abort: shuts both children's sockets, whichever is live.
    fn abort_handle(&self) -> AbortHandle {
        let primary = self.primary_abort.clone();
        let fallback = self.fallback_abort.clone();
        let mut inner = AbortHandle::default();
        inner.set_abort(Box::new(move || {
            primary.abort();
            fallback.abort();
        }));
        inner
    }

    fn close(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        // Either may be mid-handshake on the transport thread; each close()
        // aborts its own.
        self.primary.close();
        self.fallback.close();
    }
}

/// Which failure the user sees when both fail. A primary that got as far as
/// refusing the password is the real answer (right server, wrong login) — the
/// fallback's error then is just noise from speaking the wrong protocol.
/// Otherwise the fallback's error is the meaningful one. Pure → unit tested.
pub fn pick_error(primary: SourceClientError, fallback: SourceClientError) -> SourceClientError {
    let primary_is_auth = matches!(&primary, SourceClientError::Protocol(m) if m.starts_with("auth NAK"));
    if primary_is_auth {
        primary
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    /// Scripted client: connect() pops the next outcome.
    struct Script {
        outcomes: Mutex<Vec<Result<(), SourceClientError>>>,
    }

    impl SourceClient for Script {
        fn clone_box(&self) -> Box<dyn SourceClient> {
            unimplemented!("not needed in tests")
        }
        fn connect(&mut self, _b: u32, _n: &str, _g: &str) -> Result<(), SourceClientError> {
            self.outcomes.lock().remove(0)
        }
        fn send(&mut self, _f: &[u8]) -> Result<(), SourceClientError> {
            Ok(())
        }
        fn close(&mut self) {}
    }

    fn script(outcomes: Vec<Result<(), SourceClientError>>) -> Box<Script> {
        Box::new(Script { outcomes: Mutex::new(outcomes) })
    }

    #[test]
    fn primary_connect_wins_and_sticks() {
        let mut f = FallbackSourceClient::new(
            script(vec![Ok(()), Ok(())]),
            script(vec![Err(SourceClientError::Protocol("must not be reached".into()))]),
        );
        assert!(f.connect(64, "n", "g").is_ok());
        // Sticky: second connect goes to the primary alone.
        assert!(f.connect(64, "n", "g").is_ok());
    }

    #[test]
    fn primary_fails_fallback_connects_and_sticks() {
        let mut f = FallbackSourceClient::new(
            script(vec![Err(SourceClientError::Protocol("v1 NAK: junk".into())), Ok(())]),
            script(vec![Ok(()), Ok(())]),
        );
        assert!(f.connect(64, "n", "g").is_ok());
        // Sticky on the fallback now.
        assert!(f.connect(64, "n", "g").is_ok());
    }

    #[test]
    fn auth_nak_from_primary_is_the_reported_error() {
        let err = pick_error(
            SourceClientError::Protocol("auth NAK: invalid password".into()),
            SourceClientError::Protocol("uvox junk".into()),
        );
        assert!(err.to_string().contains("auth NAK"), "got: {err}");
    }

    #[test]
    fn non_auth_primary_error_falls_through_to_fallback_error() {
        let err = pick_error(
            SourceClientError::Protocol("v1 NAK: HTTP/1.1 400".into()),
            SourceClientError::Protocol("auth rejected: NAK".into()),
        );
        assert!(err.to_string().contains("auth rejected"), "got: {err}");
    }
}
