import { api, errorText } from "./api";
import { friendlyError } from "./errors";
import { fileName } from "./format";
import { setLanguage, t, type TranslationKey } from "../i18n/index.svelte";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type {
  AddResult,
  AppInfo,
  AppSettings,
  AppStatus,
  AudioDeviceDesc,
  InputKind,
  PlayerSnapshot,
  PlayerStatus,
  Profile,
  Track,
} from "../types";

const POLL_MS = 1000;
const SILENCE_DB = -90;

export type ToastKind = "info" | "success" | "error";
export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
  detail: string | null;
}

const DEFAULT_SETTINGS: AppSettings = {
  selected_profile_id: null,
  mic_gain: 1,
  input_device_name: null,
  recordings_folder: null,
  language: "en",
  launch_at_login: false,
  start_minimized: false,
  auto_reconnect: true,
  record_with_stream: false,
  input_source: "mic",
  monitor_playlist: true,
  send_track_titles: true,
  player_volume: 1,
  auto_advance: true,
  repeat: false,
};

const IDLE_PLAYER: PlayerStatus = {
  state: "Stopped",
  current_id: null,
  position_secs: 0,
  duration_secs: null,
  volume: 1,
  auto_advance: true,
  repeat: false,
};

const IDLE_STATUS: AppStatus = {
  stream: { type: "Idle" },
  stats: {
    bytes_sent: 0,
    live_secs: 0,
    session_secs: 0,
    dropped_frames: 0,
    recording: false,
    recording_path: null,
    recording_bytes: 0,
    recording_secs: 0,
    now_playing: null,
  },
  recording: { active: false, with_stream: false, path: null, secs: 0, bytes: 0, error: null },
  mic_test: false,
  input: "mic",
  mic_available: true,
  player: IDLE_PLAYER,
};

export function emptyProfile(): Profile {
  return {
    id: 0,
    name: "",
    host: "",
    port: 8000,
    sid: 1,
    password: "",
    mount: "",
    codec: "MP3",
    bitrate_kbps: 64,
    sample_rate_hz: 44100,
    mono: true,
    mode: "SOURCE",
    dj_user: null,
    dj_password: null,
    dj_port: null,
    dj_protocol: "ULTRAVOX",
    dj_mount: null,
    stream_name: "",
    genre: "",
    station_url: "",
    is_public: true,
    has_password: false,
    has_dj_password: false,
  };
}

class AppStore {
  ready = $state(false);
  status = $state<AppStatus>(IDLE_STATUS);
  settings = $state<AppSettings>({ ...DEFAULT_SETTINGS });
  profiles = $state<Profile[]>([]);
  devices = $state<AudioDeviceDesc[]>([]);
  info = $state<AppInfo | null>(null);
  level = $state({ rms: SILENCE_DB, peak: SILENCE_DB });
  /** Which action is in flight, to disable buttons and show spinners. */
  pending = $state<null | "start" | "stop" | "record" | "mictest">(null);
  /** User dismissed the current stream error banner. */
  errorDismissed = $state(false);
  lastRecordingPath = $state<string | null>(null);
  toasts = $state<Toast[]>([]);
  tracks = $state<Track[]>([]);
  player = $state<PlayerStatus>(IDLE_PLAYER);
  /** When `player.position_secs` was reported, to interpolate between polls. */
  #playerAt = 0;
  /** Ticks while playing so the position display advances smoothly. */
  clock = $state(Date.now());
  /** Files are being dragged over the window. */
  dragging = $state(false);

  #toastSeq = 0;
  #pollTimer: ReturnType<typeof setInterval> | null = null;
  #unlisten: Array<() => void> = [];
  #lastStreamType = "Idle";

  selectedProfile = $derived(
    this.profiles.find((p) => p.id === this.settings.selected_profile_id) ?? null,
  );
  streamType = $derived(this.status.stream.type);
  isStreaming = $derived(this.streamType !== "Idle" && this.streamType !== "Error");
  isRecording = $derived(this.status.recording.active);
  meterActive = $derived(this.isStreaming || this.isRecording || this.status.mic_test);
  input = $derived<InputKind>(this.status.input);
  currentTrack = $derived(this.tracks.find((t) => t.id === this.player.current_id) ?? null);
  position = $derived.by(() => {
    const p = this.player;
    if (p.state !== "Playing") return p.position_secs;
    const pos = p.position_secs + Math.max(0, this.clock - this.#playerAt) / 1000;
    return p.duration_secs ? Math.min(pos, p.duration_secs) : pos;
  });

  async init() {
    try {
      this.#unlisten.push(
        await api.onStreamState(() => void this.refresh()),
        await api.onRecordState(() => void this.refresh()),
        await api.onStreamRecording(() => void this.refresh()),
        await api.onPlayer((p) => this.#onPlayer(p)),
        await api.onPlayerError((e) => {
          const f = friendlyError(e.reason);
          this.toast("error", t("toastTrackSkipped", { title: e.title }), f.detail ?? f.message);
        }),
        await getCurrentWebview().onDragDropEvent((e) => {
          const p = e.payload;
          if (p.type === "enter" || p.type === "over") this.dragging = true;
          else if (p.type === "leave") this.dragging = false;
          else if (p.type === "drop") {
            this.dragging = false;
            if (p.paths.length) void this.addPaths(p.paths);
          }
        }),
        await api.onLevels((l) => {
          this.level = { rms: l.rms_db, peak: l.peak_db };
        }),
      );
    } catch (e) {
      console.error("event subscription failed", e);
    }

    const [settings, profiles, info, player] = await Promise.allSettled([
      api.getSettings(),
      api.listProfiles(),
      api.getAppInfo(),
      api.getPlayer(),
    ]);
    if (player.status === "fulfilled") this.#onPlayer(player.value);
    if (settings.status === "fulfilled") this.settings = settings.value;
    if (profiles.status === "fulfilled") this.profiles = profiles.value;
    if (info.status === "fulfilled") this.info = info.value;
    setLanguage(this.settings.language);

    // Single existing server: select it so "Go live" just works.
    if (this.settings.selected_profile_id == null && this.profiles.length > 0) {
      void this.selectProfile(this.profiles[0].id);
    }

    await this.refresh();
    this.ready = true;
    void this.loadDevices();
    this.#pollTimer = setInterval(() => void this.refresh(), POLL_MS);
    this.#clockTimer = setInterval(() => {
      if (this.player.state === "Playing") this.clock = Date.now();
    }, 250);
  }

  #clockTimer: ReturnType<typeof setInterval> | null = null;

  destroy() {
    if (this.#pollTimer) clearInterval(this.#pollTimer);
    if (this.#clockTimer) clearInterval(this.#clockTimer);
    this.#unlisten.forEach((u) => u());
    this.#unlisten = [];
  }

  async refresh() {
    try {
      const s = await api.getStatus();
      this.#onStatus(s);
    } catch (e) {
      console.error("status refresh failed", e);
    }
  }

  #onStatus(s: AppStatus) {
    const prev = this.#lastStreamType;
    const next = s.stream.type;
    if (prev !== next) {
      if (next === "Live" && prev !== "Live") this.toast("success", t("toastLive"));
      if (next === "Error") this.errorDismissed = false;
    }
    this.#lastStreamType = next;
    if (s.recording.path) this.lastRecordingPath = s.recording.path;
    if (!s.recording.active && !s.mic_test && !(next !== "Idle" && next !== "Error")) {
      this.level = { rms: SILENCE_DB, peak: SILENCE_DB };
    }
    this.status = s;
    this.#setPlayer(s.player);
  }

  #setPlayer(p: PlayerStatus) {
    this.player = p;
    this.#playerAt = Date.now();
    this.clock = this.#playerAt;
  }

  #onPlayer(p: PlayerSnapshot) {
    this.tracks = p.tracks;
    this.#setPlayer(p.status);
  }

  // ---- toasts -----------------------------------------------------------

  toast(kind: ToastKind, message: string, detail: string | null = null) {
    const id = ++this.#toastSeq;
    this.toasts = [...this.toasts.slice(-3), { id, kind, message, detail }];
    setTimeout(() => this.dismissToast(id), kind === "error" ? 9000 : 3500);
  }

  toastError(e: unknown) {
    const f = friendlyError(errorText(e));
    this.toast("error", f.message, f.detail);
  }

  toastKey(kind: ToastKind, key: TranslationKey, params?: Record<string, string | number>) {
    this.toast(kind, t(key, params));
  }

  dismissToast(id: number) {
    this.toasts = this.toasts.filter((x) => x.id !== id);
  }

  // ---- broadcast ----------------------------------------------------------

  async startStream() {
    if (this.pending) return;
    this.pending = "start";
    this.errorDismissed = false;
    try {
      const warning = await api.startStream(this.settings.selected_profile_id);
      if (warning) {
        const f = friendlyError(warning);
        this.toast("error", t("toastAutoRecordFailed", { error: f.message }), f.detail);
      }
    } catch (e) {
      this.toastError(e);
    } finally {
      this.pending = null;
      await this.refresh();
    }
  }

  async stopStream() {
    if (this.pending === "stop") return;
    this.pending = "stop";
    try {
      await api.stopStream();
      this.toastKey("info", "toastStopped");
    } catch (e) {
      this.toastError(e);
    } finally {
      this.pending = null;
      await this.refresh();
    }
  }

  async updateNowPlaying(title: string): Promise<boolean> {
    try {
      const live = await api.updateNowPlaying(title);
      this.toastKey("success", live ? "toastTitleSent" : "toastTitleQueued");
      await this.refresh();
      return true;
    } catch (e) {
      this.toastError(e);
      return false;
    }
  }

  // ---- recording ----------------------------------------------------------

  async toggleRecording() {
    if (this.pending) return;
    this.pending = "record";
    try {
      if (this.isRecording) {
        await api.stopRecording();
        this.toastKey("success", "toastRecordStopped");
      } else {
        await api.startRecording();
        this.toastKey("success", "toastRecordStarted");
      }
    } catch (e) {
      this.toastError(e);
    } finally {
      this.pending = null;
      await this.refresh();
    }
  }

  // ---- audio --------------------------------------------------------------

  async loadDevices() {
    try {
      this.devices = await api.listInputDevices();
    } catch (e) {
      console.error("device list failed", e);
    }
  }

  #gainTimer: ReturnType<typeof setTimeout> | null = null;
  /** Applies immediately in the UI; the backend call is coalesced. */
  setGain(gain: number) {
    this.settings.mic_gain = gain;
    if (this.#gainTimer) clearTimeout(this.#gainTimer);
    this.#gainTimer = setTimeout(() => {
      api.setMicGain(gain).catch((e) => this.toastError(e));
    }, 80);
  }

  async toggleMicTest() {
    if (this.pending) return;
    this.pending = "mictest";
    try {
      if (this.status.mic_test) await api.stopMicTest();
      else await api.startMicTest();
    } catch (e) {
      this.toastError(e);
    } finally {
      this.pending = null;
      await this.refresh();
    }
  }

  // ---- input switch & playlist -----------------------------------------------

  async setInput(kind: InputKind) {
    if (kind === this.status.input) return;
    const prev = this.status.input;
    this.status.input = kind; // optimistic: switching is instant on the backend
    try {
      await api.setInputSource(kind);
      this.settings.input_source = kind;
    } catch (e) {
      this.status.input = prev;
      this.toastError(e);
    }
  }

  #reportAdd(r: AddResult) {
    if (r.added.length) this.toastKey("success", "toastTracksAdded", { count: r.added.length });
    if (r.failed.length) {
      const detail = r.failed.map(([path, why]) => `${fileName(path)}: ${why}`).join("\n");
      this.toast("error", t("toastTracksFailed", { count: r.failed.length }), detail);
    }
    if (!r.added.length && !r.failed.length) this.toastKey("info", "toastNoAudioFiles");
  }

  async pickFiles() {
    try {
      const r = await api.pickFiles();
      if (r) this.#reportAdd(r);
    } catch (e) {
      this.toastError(e);
    }
  }

  async addPaths(paths: string[]) {
    try {
      this.#reportAdd(await api.addFiles(paths));
    } catch (e) {
      this.toastError(e);
    }
  }

  async #player(action: () => Promise<unknown>) {
    try {
      await action();
    } catch (e) {
      this.toastError(e);
    }
    try {
      this.#onPlayer(await api.getPlayer());
    } catch {
      /* the next event or poll catches up */
    }
  }

  play(id: number | null = null) {
    return this.#player(() => api.play(id));
  }
  togglePlay() {
    return this.player.state === "Playing" ? this.#player(() => api.pause()) : this.play();
  }
  stopPlayer() {
    return this.#player(() => api.stopPlayer());
  }
  next() {
    return this.#player(() => api.next());
  }
  previous() {
    return this.#player(() => api.previous());
  }
  seek(secs: number) {
    return this.#player(() => api.seek(secs));
  }
  removeTrack(id: number) {
    return this.#player(() => api.removeTrack(id));
  }
  moveTrack(id: number, to: number) {
    return this.#player(() => api.moveTrack(id, to));
  }
  clearPlaylist() {
    return this.#player(() => api.clearPlaylist());
  }

  #volumeTimer: ReturnType<typeof setTimeout> | null = null;
  setPlayerVolume(v: number) {
    this.player.volume = v;
    this.settings.player_volume = v;
    if (this.#volumeTimer) clearTimeout(this.#volumeTimer);
    this.#volumeTimer = setTimeout(() => {
      api.setPlayerVolume(v).catch((e) => this.toastError(e));
    }, 80);
  }

  setPlayerOptions(autoAdvance: boolean, repeat: boolean) {
    this.settings.auto_advance = autoAdvance;
    this.settings.repeat = repeat;
    return this.#player(() => api.setPlayerOptions(autoAdvance, repeat));
  }

  // ---- settings & profiles ---------------------------------------------------

  async saveSettings(patch: Partial<AppSettings>, quiet = false): Promise<boolean> {
    const next = { ...this.settings, ...patch };
    try {
      this.settings = await api.saveSettings(next);
      setLanguage(this.settings.language);
      if (!quiet) this.toastKey("success", "toastSettingsSaved");
      return true;
    } catch (e) {
      this.toastError(e);
      // Reload: part of the save (everything but autostart) may have applied.
      try {
        this.settings = await api.getSettings();
      } catch {
        /* keep local */
      }
      return false;
    }
  }

  async selectProfile(id: number | null) {
    await this.saveSettings({ selected_profile_id: id }, true);
  }

  async reloadProfiles() {
    try {
      this.profiles = await api.listProfiles();
      this.settings = await api.getSettings();
    } catch (e) {
      this.toastError(e);
    }
  }

  async saveProfile(p: Profile): Promise<number | null> {
    try {
      const id = await api.saveProfile(p);
      await this.reloadProfiles();
      this.toastKey("success", "toastServerSaved");
      return id;
    } catch (e) {
      this.toastError(e);
      return null;
    }
  }

  async deleteProfile(id: number) {
    try {
      await api.deleteProfile(id);
      await this.reloadProfiles();
      this.toastKey("info", "toastServerDeleted");
    } catch (e) {
      this.toastError(e);
    }
  }

  async clearProfiles() {
    try {
      await api.clearAllProfiles();
      await this.reloadProfiles();
    } catch (e) {
      this.toastError(e);
    }
  }

  async openRecordings() {
    try {
      await api.openRecordingsFolder();
    } catch (e) {
      this.toastError(e);
    }
  }

  async openLogs() {
    try {
      await api.openLogFolder();
    } catch (e) {
      this.toastError(e);
    }
  }
}

export const store = new AppStore();
