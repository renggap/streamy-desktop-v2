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

export interface AppStatus {
  stream: StreamState;
  stats: StreamStats;
  recording: RecordingStatus;
  mic_test: boolean;
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
}

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
