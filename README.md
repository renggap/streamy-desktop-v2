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
    src/lib/components/            Svelte 5 runes components (VU meter, hero, profiles, etc.)
  scripts/                         LAME build & relinking scripts (PowerShell, .mjs, bash)
  docs/                            Design documentation & porting roadmap
```

---

## Building & Verification

### Prerequisites
- **Rust:** Stable 1.80+ (`x86_64-pc-windows-msvc` on Windows; standard toolchain on Linux)
- **Node.js:** v20+ / v22+
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
| **1. Virtual & Real Mic** | Select input device in Microphone card. Speak into mic. | VU meter displays live RMS dBFS level, peak-hold tick, green/yellow/red gradient. |
| **2. Broadcast to DNAS2** | Select SHOUTcast v2 profile, click **Start Broadcast**. | Status changes Ready → Connecting → On Air with pulsing green dot. Duration and bytes sent increment. |
| **3. Mid-Stream Song Title** | Enter track title & artist in Now Playing card, click **Update Title**. | Displays "Title Updated!" banner. In-stream `0x902` XML metadata pushed over Ultravox. |
| **4. Server Kill & Reconnect** | Stop/kill the remote streaming server while broadcasting. | Status changes to **Reconnecting (1)** with backoff ladder (1s, 2s, 5s...). Resumes On Air when server restarts. |
| **5. Stop Mid-Backoff** | Click **Stop Broadcast** during reconnect backoff. | Broadcast immediately halts and reverts to Ready. No reconnection attempts occur. |
| **6. Concurrent Stream + Record** | Enable **Auto-record whenever broadcast starts** or click **Record**. | Both broadcast and standalone recording run simultaneously without audio stutter or frame drops. |
| **7. Close to System Tray** | Click window close button (X). | Window hides to system tray; broadcast continues uninterrupted. Double-clicking tray icon restores window. |
| **8. Sleep Inhibition** | Broadcast for 30 minutes with display sleep enabled. | System remains awake; broadcast does not sleep or drop connection. |
| **9. Credential Storage** | Save profile with password; restart app. | Profile password saved securely in Windows Credential Manager. Deleting profile deletes credential. |
| **10. Language Switch** | Click flag icon in masthead (🇬🇧 EN / 🇮🇩 ID). | Instant UI language switch between English and Indonesian across all cards and dialogs. |

---

## License

- Streamy Desktop: [MIT License](LICENSE)
- LAME MP3 Encoder: [GNU LGPL v2.1](crates/streamy-lame/lame_notice.txt)
