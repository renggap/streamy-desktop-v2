// Types mirroring the Rust backend (src-tauri/src/commands.rs, storage/*, streamy-core).
// Keep in sync: field names are the serde names.

export type BroadcastMode = "SOURCE" | "DJ";
export type DjProtocol = "ULTRAVOX" | "ICECAST";

export interface Profile {
  id: number;
  name: string;
  host: string;
  port: number;
  sid: number;
  /** Write-only: the backend never returns stored secrets. Blank = keep. */
  password: string;
  mount: string;
  codec: string;
  bitrate_kbps: number;
  sample_rate_hz: number;
  mono: boolean;
  mode: BroadcastMode;
  dj_user: string | null;
  /** Write-only, like `password`. */
  dj_password: string | null;
  dj_port: number | null;
  dj_protocol: DjProtocol;
  dj_mount: string | null;
  stream_name: string;
  genre: string;
  station_url: string;
  is_public: boolean;
  has_password: boolean;
  has_dj_password: boolean;
}

/** serde `#[serde(tag = "type", content = "payload")]` */
export type StreamState =
  | { type: "Idle" }
  | { type: "Connecting" }
  | { type: "Live"; payload: { dropped_frames: number } }
  | { type: "Reconnecting"; payload: { attempt: number } }
  | { type: "Error"; payload: { reason: string } };

export type RecordState =
  | { type: "Idle" }
  | { type: "Recording"; payload: { path: string } }
  | { type: "Error"; payload: { reason: string } };

export interface StreamStats {
  bytes_sent: number;
  live_secs: number;
  session_secs: number;
  dropped_frames: number;
  recording: boolean;
  recording_path: string | null;
  recording_bytes: number;
  recording_secs: number;
  now_playing: string | null;
}

export interface RecordingStatus {
  active: boolean;
  with_stream: boolean;
  path: string | null;
  secs: number;
  bytes: number;
  error: string | null;
}

export type InputKind = "mic" | "playlist";
export type PlayState = "Stopped" | "Playing" | "Paused";

export interface Track {
  id: number;
  path: string;
  title: string;
  duration_secs: number | null;
}

export interface PlayerStatus {
  state: PlayState;
  current_id: number | null;
  position_secs: number;
  duration_secs: number | null;
  volume: number;
  auto_advance: boolean;
  repeat: boolean;
}

export interface PlayerSnapshot {
  status: PlayerStatus;
  tracks: Track[];
}

export interface AddResult {
  added: Track[];
  /** [path, reason] */
  failed: [string, string][];
}

export interface PlayerErrorPayload {
  title: string;
  reason: string;
}

export interface AppStatus {
  stream: StreamState;
  stats: StreamStats;
  recording: RecordingStatus;
  mic_test: boolean;
  input: InputKind;
  /** False when the running broadcast/recording could not open the mic. */
  mic_available: boolean;
  player: PlayerStatus;
}

export interface AppSettings {
  selected_profile_id: number | null;
  mic_gain: number;
  input_device_name: string | null;
  recordings_folder: string | null;
  language: "en" | "id";
  launch_at_login: boolean;
  start_minimized: boolean;
  auto_reconnect: boolean;
  record_with_stream: boolean;
  input_source: InputKind;
  monitor_playlist: boolean;
  send_track_titles: boolean;
  player_volume: number;
  auto_advance: boolean;
  repeat: boolean;
  noise_suppression: NoiseSuppression;
  record_bitrate_kbps: number;
  record_mono: boolean;
}

export type NoiseSuppression = "off" | "low" | "medium" | "high";

/** Recording bitrates the backend accepts (MP3, 44.1 kHz). */
export const RECORD_BITRATES = [32, 48, 64, 96, 128, 192, 256, 320] as const;

export interface AudioDeviceDesc {
  name: string;
  is_default: boolean;
}

export interface LevelPayload {
  rms_db: number;
  peak_db: number;
}

export interface AppInfo {
  version: string;
  lame_version: string;
  log_dir: string;
  recordings_dir: string;
}

export type Page = "studio" | "servers" | "settings";

/** Background auto-update progress (src-tauri/src/updater.rs). */
export type UpdateStatus =
  | { state: "idle" }
  | { state: "downloading"; version: string; percent: number | null }
  | { state: "ready"; version: string }
  | { state: "installing"; version: string };
