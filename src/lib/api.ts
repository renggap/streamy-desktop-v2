import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppSettings, AudioDeviceDesc, Profile, RecordState, StreamState } from "../types";

export const api = {
  // Stream
  getStreamState: (): Promise<StreamState> => invoke("get_stream_state"),
  startStream: (profileId: number): Promise<void> => invoke("start_stream", { profileId }),
  stopStream: (): Promise<void> => invoke("stop_stream"),
  updateNowPlaying: (title: string): Promise<void> =>
    invoke("update_now_playing", { title }),

  // Recording
  getRecordState: (): Promise<RecordState> => invoke("get_record_state"),
  startRecording: (): Promise<void> => invoke("start_recording"),
  stopRecording: (): Promise<void> => invoke("stop_recording"),
  toggleStreamRecording: (enabled: boolean): Promise<void> =>
    invoke("toggle_stream_recording", { enabled }),

  // Audio & Mic
  setMicGain: (gain: number): Promise<void> => invoke("set_mic_gain", { gain }),
  listInputDevices: (): Promise<AudioDeviceDesc[]> => invoke("list_input_devices"),

  // Settings
  getSettings: (): Promise<AppSettings> => invoke("get_settings"),
  saveSettings: (settings: AppSettings): Promise<void> => invoke("save_settings", { settings }),

  // Profiles
  listProfiles: (): Promise<Profile[]> => invoke("list_profiles"),
  getProfile: (id: number): Promise<Profile> => invoke("get_profile", { id }),
  saveProfile: (profile: Profile): Promise<number> => invoke("save_profile", { profile }),
  deleteProfile: (id: number): Promise<void> => invoke("delete_profile", { id }),
  clearAllProfiles: (): Promise<void> => invoke("clear_all_profiles"),

  // Legal & Folders
  getLameNotice: (): Promise<string> => invoke("get_lame_notice"),
  openLogFolder: (): Promise<void> => invoke("open_log_folder"),
  openRecordingsFolder: (): Promise<void> => invoke("open_recordings_folder"),

  // Event Listeners
  onStreamState: (callback: (state: StreamState) => void): Promise<UnlistenFn> =>
    listen<StreamState>("streamy://stream-state", (event) => callback(event.payload)),

  onRecordState: (callback: (state: RecordState) => void): Promise<UnlistenFn> =>
    listen<RecordState>("streamy://record-state", (event) => callback(event.payload)),

  onLevelTap: (callback: (level: { rms_dbfs: number; peak_dbfs: number; clipping: boolean }) => void): Promise<UnlistenFn> =>
    listen<{ rms_dbfs: number; peak_dbfs: number; clipping: boolean }>("streamy://level-tap", (event) =>
      callback(event.payload)
    ),
};
