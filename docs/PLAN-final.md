# Streamy Desktop v2 — Windows (Tauri 2) — FINAL PLAN

**Status:** ready for approval (4 small confirmations at the bottom)
**Date:** 2026-10-26
**Supersedes:** `PLAN.md` (2026-10-25)
**Source of truth:** `/home/streamy/` (Android v1.10, Kotlin/Compose/NDK) — read-only, never modified.
**Destination:** `/home/streamy-desktop-v2/` — brand-new project.

## Decisions locked in

| Topic | Decision |
|---|---|
| Target OS | **Windows only** — Windows 10 1809+ / 11, x64. No Linux/macOS builds. |
| LAME (LGPL) | **Bundled shared library** `libmp3lame.dll` built from the vendored source, shipped next to the exe (mirrors Android's shared `.so` model). |
| Server passwords | **Windows Credential Manager** (`keyring` crate). SQLite row stores only a credential reference. |
| UI language | **English + Bahasa Indonesia**, switchable in Settings; default from OS locale. |
| Frontend | Svelte 5 + Vite + TypeScript. |
| AAC | No encoder (Android ships it UI-disabled). ADTS + class-8 wire code ported and unit-tested for later. |
| Profiles storage | SQLite (rusqlite, bundled), same columns as `ProfileEntity`. |
| Concurrent recorder | Second cpal input stream (WASAPI shared mode allows multiple capture clients); fan-out from one capture is the fallback. |
| Identity | "Streamy Desktop", v2.0.0, identifier `com.streamy.desktop`. |

## Goal

A faithful fork of Streamy Android as a Windows desktop app (Rust core + Svelte 5 UI). Every
user-visible function of the Android app works 100%: live SOURCE/DJ streaming over Ultravox 2.1 /
ICE/1.0, LAME MP3 across every bitrate/rate/channel combo, standalone recording (also concurrent
with a stream), exact auto-reconnect behavior, admin-free now-playing, VU meter, server profiles,
mic gain + input-device selection, and the Hallmark "late-night tally-light" UI.

## Architecture change vs. the first plan: Cargo workspace split

The dev machine is Linux; the product is Windows-only. To keep ~90% of the work testable here,
the code is split so only a thin shell is Windows-specific:

```
/home/streamy-desktop-v2/
  PLAN.md  README.md  LICENSE  .gitignore
  Cargo.toml                       workspace
  crates/
    streamy-core/                  PLATFORM-NEUTRAL, builds + tests on Linux AND Windows
      src/core/                    xtea, uvox framing, adts, wire_safety, audio_config_rules,
                                   dj_protocol, backoff, level (RMS + decimation), teardown_state
      src/net/                     uvox.rs, icecast.rs, transport.rs
      src/audio/                   AudioSource trait, pipeline.rs, resample.rs, sink.rs
      src/session/                 stream_session.rs, record_session.rs
      tests/                       unit + env-gated live tests
    streamy-lame/                  vendored LAME 3.100 (encoder-only config.h) + FFI
      vendor/lame/ + lame_notice.txt
      build.rs                     Windows: builds libmp3lame.dll + import lib (via cc/cmake, .def export file)
                                   Linux:   builds shared lib for dev/tests only
  src-tauri/                       WINDOWS SHELL (only crate that needs Windows to build)
    src/audio_cpal.rs              cpal (WASAPI) impl of AudioSource
    src/power.rs                   SetThreadExecutionState sleep inhibitor
    src/secrets.rs                 Credential Manager via keyring
    src/storage/                   SQLite profiles, JSON prefs (atomic write)
    src/commands.rs  events.rs  tray.rs  logging.rs
  src/                             Svelte 5 UI + i18n (en, id)
  scripts/                         build-lame-dll.ps1, relink instructions, dev helpers
  docs/                            copied protocol finding + this plan
```

`AudioSource` is a trait (frames of i16 PCM at the configured rate). Production uses cpal/WASAPI;
tests use a sine/WAV feeder. This makes the full pipeline, reconnect ladder and recorder testable
with no sound card (needed for CI and for this Linux box).

## Function parity checklist — all must work 100%

1. **SOURCE mode (Ultravox 2.1 → DNAS2):** handshake exactly as pinned in `UvoxClient`:
   0x1009 cipher (server key, incl. empty) → 0x1001 XTEA auth
   (`2.1:<sid>:<xtea(uid)>:<xtea(password)>`, UID blank) → 0x1040 mime → 0x1002 setup →
   0x1003 buffer → 0x100..0x103 ICY config → 0x008 max-payload (16377) → 0x004 standby.
   MP3 data frames class 7 / type 0. Close sends Terminate (0x005), bounded 500 ms join.
2. **DJ mode:** Ultravox to sc_trans `djport2` with `djlogin` in the 0x1001 UID field, OR ICE/1.0
   (`SOURCE <mount> ICE/1.0` + HTTP Basic, numeric 2xx parse). Per profile via `djProtocol`
   (ULTRAVOX | ICECAST); `djPort` falls back to `port`; `djMount` defaults to `/`.
3. **LAME MP3** (same params as `lame_jni.cpp`: quality 5, `out_samplerate` pinned to the
   configured rate, fail loud on mismatch): bitrates per Android `AudioConfig` (**step 1 reads the
   Android source and fixes the list — README says 16/24/48/64/96/128/192, old plan added 32; a
   test asserts the desktop list equals Android's**), rates 16/22.05/44.1/48 kHz, mono/stereo.
   `AudioConfigRules` identical: MP3 16/24 kbps clamped to MPEG-2 (≤ 24 kHz); `validate`/`clamp`
   semantics preserved.
4. **Standalone recorder:** mic → LAME → `streamy-<yyyyMMdd-HHmmss>-<ms>.mp3`; runs without a
   stream and concurrently with one (stream-record = direct MP3 pass-through sink armed at frame
   boundaries). **Output folder:** default `%USERPROFILE%\Music\Streamy\`, changeable in Settings
   (folder picker); filenames Windows-safe.
5. **Auto-reconnect:** bounded queue (cap 64, drop-newest + drop counting), backlog cleared on
   reconnect, backoff 1/2/5/10/20 s ±20% jitter clamped [500, 24000] ms, give up after 10 →
   Failed, auth NAK (`ProtocolException`) non-retryable, Stop aborts immediately even mid-backoff,
   no phantom states after stop.
6. **Now-playing:** Ultravox class 3 / 0x902 XML (`<TIT2>`), control-char-stripped, 512 cap. ICE
   path: ICY `StreamTitle='...'` every 8192 audio bytes + best-effort `/admin/metadata` GET.
7. **VU meter:** pre-encoder tap, RMS dBFS (floor −90) + 96-point waveform decimation, ~30 fps,
   reset on stop.
8. **State machine → UI:** Idle / Connecting / Live(droppedFrames) / Reconnecting(attempt) /
   Error(reason); per-session drop baseline; backpressure folded into Live at ~4 Hz;
   `resolveTeardownState` (preserve Error on forced teardown, Idle on user stop).
9. **Profiles:** name, host, port, sid, password, mount, codec, bitrateKbps, sampleRateHz, mono,
   mode, djUser, djPort, djProtocol, djMount (legacy adminUser/adminPassword kept, unused).
   CRUD + select; audio dropdowns tune the selected profile in place; `needsRestartToApply` while
   Live. **Passwords live in Credential Manager**; deleting a profile / clear-all removes its entries.
10. **Mic settings:** software gain 0..2 (live push to the running pipeline), input-device
    enumeration + selection by name ("System default" option), persisted.
11. **Settings:** app name/version, language (en/id), input device, recordings folder, clear-all
    with confirm, LAME LGPL notice (surfaced in-app), "Open log folder".
12. **Background operation:** close = minimize to tray; stream + recording keep running; tray:
    Show / Stop stream / Quit. Windows may hide new tray icons in the overflow — first-close shows
    a one-time hint.
13. **Lifecycle discipline:** single idempotent teardown per session, start-during-shutdown and
    double-start guards, bounded joins (500 ms), no leaked sockets/threads/encoder handles,
    encode-after-close refused safely.

### Windows-specific additions (not in Android, required for parity of *behavior*)

- **Sleep inhibitor** (`power.rs`): `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`
  while Live/Connecting/Reconnecting or recording; released on teardown. Equivalent of the
  Android wake lock + WifiLock.
- **Single instance:** `tauri-plugin-single-instance` — second launch focuses the existing window
  (prevents two apps fighting over mic, DB and tray).
- **Microphone privacy:** WASAPI capture fails when "Let desktop apps access your microphone" is
  off → map to a `mic_denied` message with a hint to open Settings → Privacy → Microphone.
- **Audio format handling (WASAPI shared mode only offers the mix format):** convert f32/i16,
  downmix/upmix channels (behavior copied from Android `AudioCapture`; step 3 reads it), resample
  with `rubato` when the device rate ≠ configured rate, so the encoder always sees exactly the
  configured rate.
- **Logging:** `tracing` to `%APPDATA%\com.streamy.desktop\logs\` (rolling). Passwords/credentials
  are never logged (`WireSafety`-style redaction test).
- **Optional (default OFF, beyond Android):** "Resume stream on launch" (analog of `START_STICKY`
  recovery) and "Launch at login" (`tauri-plugin-autostart`).
- **i18n:** events carry message **codes**, never prose; UI maps codes to en/id strings. English is
  the base; Indonesian strings are drafted and listed in one file for your review.

**Out of scope for 2.0:** AAC encoder, SHOUTcast v1, decks/EQ/mixer, Linux/macOS, auto-update,
Microsoft Store packaging.

## Approach — milestones with gates (stop and verify at each gate)

**M0 — Scaffold.** Workspace + Tauri 2 + Vite + Svelte 5 + TS, `git init`, `.gitignore`
(`local.properties`, `.env*`, `*.jks`, `target/`), LICENSE (LGPL notice for LAME; author license
for the rest), README with the AGENTS.md hard rules (protocol goldens, teardown discipline,
secrets in env only). *Gate:* `cargo test -p streamy-core` green on Linux; `tauri build` skeleton
green on Windows CI.

**M1 — Pure core + goldens** (vectors copied byte-identical from `app/src/test`): `UvoxXtea`
(`encipher("test","foobar") = "0797c1d44e31a1f3"`), `message()` framing, `adtsHeader`,
`WireSafety`, `AudioConfigRules`, `DjProtocol.parse`, `LevelTap`, backoff ladder, `shouldGiveUp`,
`isNonRetryable`, `resolveTeardownState`. *Gate:* all 1:1 ported tests green.

**M2 — LAME.** Vendor `app/src/main/cpp/lame` + `include/lame.h`; build `libmp3lame.dll` (+ import
lib, `.def` listing only the `lame_*` symbols used); Rust FFI mirroring `lame_jni.cpp` (mono
`lame_encode_buffer` vs interleaved stereo, flush, `lame_get_out_samplerate` pin, idempotent
close). Installer bundles the DLL; `scripts/build-lame-dll.ps1` + relink instructions satisfy LGPL.
*Gate:* encode/flush/out-rate test for EVERY legal config (bitrate × rate × mono/stereo) + MP3
frame-header sanity decode against `silence.mp3` / `lame_readback.c`.

**M3 — Net layer + live handshake.** `UvoxClient`, `IcecastSourceClient`, `StreamTransport`
(dedicated task + bounded channel, `try_send` drop-newest, injectable backoff). *Gate (live,
env-gated):* real DNAS2 — full handshake, 60 s MP3 stream, `STREAMSTATUS=1`, correct
`BITRATE`/`CONTENT`, 0x902 `SONGTITLE` mid-stream. ICE/1.0 against a local Icecast/Liquidsoap
(runs on this Linux box; net crate is platform-neutral).

**M4 — Sessions + pipeline** (still platform-neutral, sine/WAV `AudioSource`):
`stream_session`, `record_session`, TransportState → StreamState mapping with drop baseline and
250 ms throttle, armRecord/disarmRecord at frame boundaries, mic gain at session scope.
*Gate:* automated "kill server → exact backoff ladder → restart → auto-resume", stop-during-backoff,
20× start/stop with no leaked threads/handles, recorder output ffprobe-playable.

**M5 — Windows shell.** cpal/WASAPI capture, power inhibitor, Credential Manager, SQLite + prefs,
tray + close-to-tray, single instance, IPC commands/events, logging. *Gate (on Windows):* live
mic → DNAS2 for 10 min, zero drops; sleep-inhibit verified (screen off, machine stays up);
concurrent stream + record; device selection persists.

**M6 — UI + i18n.** Home (masthead, state pill, hero Start/Stop incl. stop-reconnect during
Connecting/Reconnecting/Error, profile chip, now-playing + UPDATE, gain slider, both record
controls, VU canvas with phosphor afterglow), audio dropdowns with clamp rules, Profiles +
EditProfile (all fields, DJ group, protocol selector), Settings. Hallmark palette as plain CSS.
en/id switch. *Gate:* `npm run check` + build; side-by-side visual pass vs the Android app.

**M7 — Packaging + docs.** `tauri build` on GitHub Actions `windows-latest`: **NSIS per-user
installer** (no admin; embeds the WebView2 bootstrapper), DLL + `lame_notice.txt` bundled, README
(build, test, live-test setup). Code signing: see open item 2.

## Validation

- **Automated, Linux + Windows CI:** all unit tests; new tests for LAME every-config, ICE metadata
  block layout, drop-newest accounting, credential redaction in logs, i18n key parity (every en key
  has an id key), bitrate list equals Android's.
- **Live, env-gated `STREAMY_TEST_*` (never committed; copied by hand from
  `/home/streamy/local.properties`; run manually, not in CI):** DNAS2 handshake/stream/metadata.
- **Windows E2E (manual script on a real Windows machine/VM):** virtual mic (VB-Audio Virtual Cable)
  and real mic; start → Live → 10 min zero drops; server kill → ladder → resume; Stop mid-backoff;
  concurrent stream + record; profile CRUD + persistence across restart; credentials visible in
  Credential Manager and removed on delete; `needsRestartToApply`; close-to-tray keeps streaming;
  sleep-inhibit; mic-privacy-off error message; language switch en/id.
- **Acceptance gate:** the parity checklist fully green on Windows = "100% working".

## Remaining confirmations (defaults apply if you say nothing)

1. **Windows test machine:** do you have a Windows PC/VM for the M5/E2E gates? *Default:* yes, you
   run the manual script and report; CI only builds.
2. **Code signing:** unsigned installer triggers SmartScreen warnings. *Default:* ship unsigned for
   2.0.0, signing (OV cert / Azure Trusted Signing) as a follow-up.
3. **Workspace split** (`streamy-core` / `streamy-lame` / `src-tauri`) so most work is testable on
   this Linux box. *Default:* approved.
4. **Indonesian copy:** I draft id strings from the Android/web wording; you review one file at M6.
