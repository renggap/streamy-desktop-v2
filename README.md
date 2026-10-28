# Streamy Desktop v2

Windows desktop app (Tauri 2 + Rust + Svelte 5) that streams mic audio to SHOUTcast v2 (DNAS2) and Icecast servers over Ultravox 2.1 or ICE/1.0, plus a standalone MP3 recorder. Faithfully ported from Streamy Android.

## Hard Rules & Principles

- **LAME is LGPL.** Vendored under `crates/streamy-lame/vendor/lame`. Kept as a separate dynamically-loaded/shared library (`lame_enc.dll` on Windows, `.so` for Linux dev). Preserves `crates/streamy-lame/lame_notice.txt` and relinkability instructions in `scripts/README-relink.md`.
- **Protocol behavior is pinned by golden vectors and live tests.** Ultravox handshake, XTEA cipher, ADTS-wrapped AAC, and in-stream `0x902` XML metadata are pinned byte-for-byte by golden vectors (`UvoxXteaTest`).
- **Teardown & idempotency discipline.** Single idempotent teardown paths, race guards, bounded joins (500 ms) on network closes, no leaked sockets, threads, or encoder handles.
- **Secrets stay in Credential Manager / env only.** Passwords never logged in plaintext. Windows Credential Manager stores passwords per profile ID. Logs redact Basic auth tokens and password fields.

---

## Architecture & Layout

```
streamy-desktop-v2/
  Cargo.toml                       Cargo workspace root
  crates/
    streamy-core/                  Platform-neutral core (builds & tests on Linux and Windows)
      src/core/                    XTEA, Ultravox framing, ADTS, WireSafety, AudioConfigRules,
                                   DjProtocol, backoff ladder, LevelTap, TeardownState
      src/net/                     UvoxClient, IcecastSourceClient, StreamTransport
      src/audio/                   AudioSource trait, pipeline, rubato resample, stream sinks
      src/session/                 stream_session, record_session
      tests/                       Golden vector unit tests, mock servers, session lifecycle
    streamy-lame/                  Vendored LAME 3.100 (encoder-only) + safe Rust FFI wrapper
  src-tauri/                       Windows desktop shell
    src/audio_cpal.rs              WASAPI AudioSource via cpal with rubato resampling
    src/power.rs                   SetThreadExecutionState sleep inhibitor
    src/secrets.rs                 Windows Credential Manager via keyring
    src/storage/                   SQLite profiles (rusqlite) & atomic JSON preferences
    src/commands.rs                Tauri IPC command handlers
    src/events.rs                  Tauri UI event emitters
    src/tray.rs                    System tray menu & window minimize-to-tray
    src/logging.rs                 Tracing rolling file logger with credential redaction
  src/                             Svelte 5 + Vite + TypeScript UI (Hallmark console palette)
    src/i18n/                      Strict 1:1 translation dictionaries (en, id)
    src/lib/store.svelte.ts        Single app store: status polling + events, actions, toasts
    src/lib/errors.ts              Maps backend error codes/messages to friendly, translated text
    src/lib/components/            Svelte 5 components (on-air panel, meter, servers, settings)
  scripts/                         LAME build & relinking scripts (PowerShell, .mjs, bash)
  docs/                            Design documentation & porting roadmap
```

---

## Building & Verification

### Prerequisites
- **Rust:** Stable 1.80+ (`x86_64-pc-windows-msvc` on Windows; standard toolchain on Linux)
- **Node.js:** v24+
- **Linux dev packages:** `libasound2-dev libwebkit2gtk-4.1-dev build-essential`

### 1. Run Workspace Tests (Core + LAME + Desktop)
```bash
cargo test --workspace
```
Runs 60 automated unit, golden vector, mock server, and concurrency tests.

### 2. Verify i18n Key Parity (English / Indonesian)
```bash
npm run test:i18n
```
Asserts 100% key parity between English and Indonesian dictionaries with no empty translations.

### 3. Verify Frontend Typecheck & Build
```bash
npm run check
npm run build
```
Runs `svelte-check` with TypeScript validation and Vite production bundling.

### 4. Build Windows NSIS Installer (on Windows)
```powershell
./scripts/build-lame-dll.ps1
npm run tauri build
```
Generates a per-user NSIS installer in `src-tauri/target/release/bundle/nsis/` bundling the app, `lame_enc.dll`, and WebView2 bootstrapper.

---

## LGPL Relinking & Replacement

In compliance with the GNU LGPL v2.1 for LAME MP3 Encoder:
- The LAME encoder is compiled as a standalone dynamic library (`lame_enc.dll`).
- End-users can compile their own custom or modified LAME DLL using `scripts/build-lame-dll.ps1` and drop it into the application installation directory.
- Detailed relinking instructions are provided in `scripts/README-relink.md` and accessible directly in the app's **Settings > License & Relinking (LGPL)** tab.

---

## Manual QA Verification Matrix (Windows)

| Test Case | Procedure | Expected Result |
| :--- | :--- | :--- |
| **1. Mic check** | Pick the input device in the Microphone card, click **Test mic**, speak. | Meter shows live level on a −60…0 dB scale with peak hold; CLIP lights above −0.5 dBFS. Gain slider (−30…+12 dB) and Mute take effect immediately. |
| **2. Broadcast to DNAS 2** | Add a *SHOUTcast source* server, click **Go live**. | Status pill goes Off air → Connecting → On air; time and bytes sent count up. A wrong password shows a plain-language error with Retry and Details. |
| **3. AzuraCast DJ** | Add a *DJ / streamer* server with *AzuraCast / Icecast*, DJ user, password, DJ port 8005, mount `/`. | Goes live; listeners hear the stream. |
| **4. Now playing** | Type a title in Now playing, press Enter (before or while live). | Before going live: queued and sent on connect. While live: "Title updated for listeners." |
| **5. Server kill & reconnect** | Stop the server while live. | Pill shows **Reconnecting (attempt n)** and a hint; resumes On air when the server returns. |
| **6. Stop mid-backoff** | Click **Cancel** while reconnecting. | Stops at once and returns to Off air; no further attempts. |
| **7. Record** | Click **Record** idle (mic-only file) and while live (records the broadcast), or tick *Record every broadcast automatically*. | MP3 lands in the recordings folder; timer and size update; Open folder works. |
| **8. Close to tray** | Close the window while live or recording. | Window hides; broadcast continues; tray tooltip shows the state; left-click restores. Closing while idle quits. |
| **9. Sleep inhibition** | Broadcast for 30 minutes with sleep enabled. | The PC stays awake until the broadcast and recording both stop. |
| **10. Credentials** | Save a server with a password, restart. | Password lives in Credential Manager; the editor shows "Saved · leave empty to keep"; deleting the server removes it. |
| **11. Language** | Settings › General › Bahasa Indonesia. | Every card and dialog switches language instantly, no reload. |

---

## License

- Streamy Desktop: [MIT License](LICENSE)
- LAME MP3 Encoder: [GNU LGPL v2.1](crates/streamy-lame/lame_notice.txt)
