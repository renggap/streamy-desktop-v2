// Types matching the Rust backend and Streamy Desktop domain.

export type DjProtocol = "SHOUTCAST" | "ICECAST";
export type BroadcastMode = "DJ" | "AUTOMATION";

export interface Profile {
  id?: number;
  name: string;
  server: string;
  port: number;
  mount?: string | null;
  mode: BroadcastMode;
  protocol: DjProtocol;
  bitrate: number;
  samplerate: number;
  channels: number;
  is_public: boolean;
  stream_name?: string | null;
  stream_genre?: string | null;
  stream_desc?: string | null;
  stream_url?: string | null;
  stream_aim?: string | null;
  stream_icq?: string | null;
  stream_irc?: string | null;
  has_password?: boolean;
  password?: string;
}

export type StreamStatus = "idle" | "connecting" | "streaming" | "reconnecting" | "teardown";

export interface StreamState {
  status: StreamStatus;
  profile_id?: number | null;
  profile_name?: string | null;
  bytes_sent: number;
  duration_secs: number;
  current_bitrate_kbps: number;
  reconnect_attempt: number;
  reconnect_delay_ms: number;
  backpressure_pct: number;
  dropped_frames: number;
  last_error?: string | null;
}

export type RecordStatus = "idle" | "recording" | "error";

export interface RecordState {
  status: RecordStatus;
  duration_secs: number;
  bytes_written: number;
  output_path?: string | null;
  last_error?: string | null;
}

export interface AppSettings {
  active_profile_id?: number | null;
  mic_device?: string | null;
  mic_gain: number;
  recordings_folder?: string | null;
  record_with_stream: boolean;
  auto_reconnect: boolean;
  start_minimized: boolean;
  autostart: boolean;
  language: "en" | "id";
}

export interface AudioDeviceDesc {
  name: string;
  is_default: boolean;
}

export interface LevelTapEvent {
  rms_dbfs: number;
  peak_dbfs: number;
  clipping: boolean;
}
