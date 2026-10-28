import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AddResult,
  AppInfo,
  AppSettings,
  AppStatus,
  AudioDeviceDesc,
  InputKind,
  LevelPayload,
  PlayerErrorPayload,
  PlayerSnapshot,
  Profile,
  RecordState,
  StreamState,
} from "../types";

// Event names: keep in sync with src-tauri/src/events.rs
const EVENT_STREAM_STATE = "stream-state-changed";
const EVENT_RECORD_STATE = "record-state-changed";
const EVENT_LEVELS = "levels-changed";
const EVENT_STREAM_RECORDING = "stream-recording-changed";
const EVENT_PLAYER_CHANGED = "player-changed";
const EVENT_PLAYER_ERROR = "player-error";

export const api = {
  getStatus: (): Promise<AppStatus> => invoke("get_status"),

  // Broadcast. Resolves to a non-fatal warning (e.g. auto-record failed) or null.
  startStream: (profileId: number | null): Promise<string | null> =>
    invoke("start_stream", { profileId }),
  stopStream: (): Promise<void> => invoke("stop_stream"),
  /** true = sent live, false = queued until on air */
  updateNowPlaying: (title: string): Promise<boolean> => invoke("update_now_playing", { title }),

  // Recording: records the broadcast while on air, else a standalone mic recording.
  startRecording: (): Promise<string> => invoke("start_recording"),
  stopRecording: (): Promise<void> => invoke("stop_recording"),

  // Audio
  setMicGain: (gain: number): Promise<number> => invoke("set_mic_gain", { gain }),
  listInputDevices: (): Promise<AudioDeviceDesc[]> => invoke("list_input_devices"),
  startMicTest: (): Promise<void> => invoke("start_mic_test"),
  stopMicTest: (): Promise<void> => invoke("stop_mic_test"),

  // Input switch + playlist
  setInputSource: (source: InputKind): Promise<void> => invoke("set_input_source", { source }),
  getPlayer: (): Promise<PlayerSnapshot> => invoke("get_player"),
  /** Native file picker; null when cancelled. */
  pickFiles: (): Promise<AddResult | null> => invoke("playlist_pick_files"),
  addFiles: (paths: string[]): Promise<AddResult> => invoke("playlist_add", { paths }),
  removeTrack: (id: number): Promise<void> => invoke("playlist_remove", { id }),
  moveTrack: (id: number, to: number): Promise<void> => invoke("playlist_move", { id, to }),
  clearPlaylist: (): Promise<void> => invoke("playlist_clear"),
  play: (id: number | null = null): Promise<void> => invoke("player_play", { id }),
  pause: (): Promise<void> => invoke("player_pause"),
  stopPlayer: (): Promise<void> => invoke("player_stop"),
  next: (): Promise<void> => invoke("player_next"),
  previous: (): Promise<void> => invoke("player_previous"),
  seek: (secs: number): Promise<void> => invoke("player_seek", { secs }),
  setPlayerVolume: (volume: number): Promise<number> => invoke("player_set_volume", { volume }),
  setPlayerOptions: (autoAdvance: boolean, repeat: boolean): Promise<void> =>
    invoke("player_set_options", { autoAdvance, repeat }),

  // Settings
  getSettings: (): Promise<AppSettings> => invoke("get_settings"),
  saveSettings: (settings: AppSettings): Promise<AppSettings> => invoke("save_settings", { settings }),

  // Profiles
  listProfiles: (): Promise<Profile[]> => invoke("list_profiles"),
  saveProfile: (profile: Profile): Promise<number> => invoke("save_profile", { profile }),
  deleteProfile: (id: number): Promise<void> => invoke("delete_profile", { id }),
  clearAllProfiles: (): Promise<void> => invoke("clear_all_profiles"),

  // Misc
  getLameNotice: (): Promise<string> => invoke("get_lame_notice"),
  getAppInfo: (): Promise<AppInfo> => invoke("get_app_info"),
  openLogFolder: (): Promise<void> => invoke("open_log_folder"),
  openRecordingsFolder: (): Promise<void> => invoke("open_recordings_folder"),

  // Events
  onStreamState: (cb: (s: StreamState) => void): Promise<UnlistenFn> =>
    listen<StreamState>(EVENT_STREAM_STATE, (e) => cb(e.payload)),
  onRecordState: (cb: (s: RecordState) => void): Promise<UnlistenFn> =>
    listen<RecordState>(EVENT_RECORD_STATE, (e) => cb(e.payload)),
  onStreamRecording: (cb: (on: boolean) => void): Promise<UnlistenFn> =>
    listen<boolean>(EVENT_STREAM_RECORDING, (e) => cb(e.payload)),
  onPlayer: (cb: (p: PlayerSnapshot) => void): Promise<UnlistenFn> =>
    listen<PlayerSnapshot>(EVENT_PLAYER_CHANGED, (e) => cb(e.payload)),
  onPlayerError: (cb: (p: PlayerErrorPayload) => void): Promise<UnlistenFn> =>
    listen<PlayerErrorPayload>(EVENT_PLAYER_ERROR, (e) => cb(e.payload)),
  onLevels: (cb: (l: LevelPayload) => void): Promise<UnlistenFn> =>
    listen<LevelPayload>(EVENT_LEVELS, (e) => cb(e.payload)),
};

/** Tauri rejects with a plain string from our commands; normalise anything else. */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}
