<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { t, getLanguage, setLanguage, subscribeLanguage } from "./i18n";
  import { api } from "./lib/api";
  import type { AppSettings, Profile, RecordState, RecordStatus, StreamState, StreamStatus } from "./types";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  import Masthead from "./lib/components/Masthead.svelte";
  import HeroControl from "./lib/components/HeroControl.svelte";
  import NowPlaying from "./lib/components/NowPlaying.svelte";
  import VuMeter from "./lib/components/VuMeter.svelte";
  import MicSlider from "./lib/components/MicSlider.svelte";
  import RecordControl from "./lib/components/RecordControl.svelte";
  import ServerProfiles from "./lib/components/ServerProfiles.svelte";
  import EditProfile from "./lib/components/EditProfile.svelte";
  import SettingsModal from "./lib/components/Settings.svelte";

  // App State
  let streamState = $state<StreamState>({
    status: "idle",
    profile_id: null,
    profile_name: null,
    bytes_sent: 0,
    duration_secs: 0,
    current_bitrate_kbps: 0,
    reconnect_attempt: 0,
    reconnect_delay_ms: 0,
    backpressure_pct: 0,
    dropped_frames: 0,
    last_error: null,
  });

  let recordState = $state<RecordState>({
    status: "idle",
    duration_secs: 0,
    bytes_written: 0,
    output_path: null,
    last_error: null,
  });

  let settings = $state<AppSettings>({
    active_profile_id: null,
    mic_device: null,
    mic_gain: 1.0,
    recordings_folder: null,
    record_with_stream: false,
    auto_reconnect: true,
    start_minimized: false,
    autostart: false,
    language: "en",
  });

  let profiles = $state<Profile[]>([]);
  let editingProfile = $state<Profile | null>(null);
  let showEditModal = $state(false);
  let showSettingsModal = $state(false);
  let globalToast = $state<string | null>(null);
  let toastTimer: any = null;

  let unlistenStream: UnlistenFn | null = null;
  let unlistenRecord: UnlistenFn | null = null;
  let unsubscribeLang: (() => void) | null = null;

  // Reactivity tick for i18n changes
  let langTick = $state(0);

  function showToast(msg: string) {
    globalToast = msg;
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      globalToast = null;
    }, 4000);
  }

  let streamTimer: any = null;
  let recordTimer: any = null;

  function updateStreamState(raw: any) {
    if (!raw) return;
    const type = raw.type || (typeof raw.status === "string" ? raw.status : "Idle");
    let status: StreamStatus = "idle";
    let dropped = streamState.dropped_frames;
    let attempt = 0;
    let error: string | null = null;

    if (type === "Live" || type === "streaming") {
      status = "streaming";
      if (raw.payload && typeof raw.payload.dropped_frames === "number") {
        dropped = raw.payload.dropped_frames;
      }
    } else if (type === "Connecting" || type === "connecting") {
      status = "connecting";
    } else if (type === "Reconnecting" || type === "reconnecting") {
      status = "reconnecting";
      attempt = raw.payload?.attempt ?? (raw.reconnect_attempt || 1);
    } else if (type === "Error" || type === "error") {
      status = "idle";
      error = typeof raw.payload?.reason === "string" ? raw.payload.reason : (raw.last_error || "Stream error");
    } else {
      status = "idle";
    }

    const isNowStreaming = status === "streaming";
    const wasStreaming = streamState.status === "streaming";
    const activeBitrate = activeProfile?.bitrate_kbps || activeProfile?.bitrate || 128;

    streamState = {
      ...streamState,
      status,
      reconnect_attempt: attempt,
      dropped_frames: dropped,
      last_error: error,
      profile_name: activeProfile?.name ?? streamState.profile_name,
      current_bitrate_kbps: isNowStreaming ? activeBitrate : 0,
    };

    if (isNowStreaming && !wasStreaming) {
      streamState.duration_secs = 0;
      streamState.bytes_sent = 0;
      if (streamTimer) clearInterval(streamTimer);
      streamTimer = setInterval(() => {
        streamState.duration_secs = (streamState.duration_secs || 0) + 1;
        streamState.bytes_sent = (streamState.bytes_sent || 0) + Math.round((activeBitrate * 1000) / 8);
      }, 1000);
    } else if (!isNowStreaming && wasStreaming) {
      if (streamTimer) clearInterval(streamTimer);
      streamTimer = null;
    }
  }

  function updateRecordState(raw: any) {
    if (!raw) return;
    const type = raw.type || (typeof raw.status === "string" ? raw.status : "Idle");
    let status: RecordStatus = "idle";
    let outPath: string | null = null;
    let error: string | null = null;

    if (type === "Recording" || type === "recording") {
      status = "recording";
      outPath = raw.payload?.path ?? raw.output_path ?? null;
    } else if (type === "Error" || type === "error") {
      status = "error";
      error = typeof raw.payload?.reason === "string" ? raw.payload.reason : (raw.last_error || "Recording error");
    } else {
      status = "idle";
    }

    const isNowRecording = status === "recording";
    const wasRecording = recordState.status === "recording";

    recordState = {
      ...recordState,
      status,
      output_path: outPath,
      last_error: error,
    };

    if (isNowRecording && !wasRecording) {
      recordState.duration_secs = 0;
      recordState.bytes_written = 0;
      if (recordTimer) clearInterval(recordTimer);
      recordTimer = setInterval(() => {
        recordState.duration_secs = (recordState.duration_secs || 0) + 1;
        const kbps = activeProfile?.bitrate_kbps || activeProfile?.bitrate || 128;
        recordState.bytes_written = (recordState.bytes_written || 0) + Math.round((kbps * 1000) / 8);
      }, 1000);
    } else if (!isNowRecording && wasRecording) {
      if (recordTimer) clearInterval(recordTimer);
      recordTimer = null;
    }
  }

  onMount(async () => {
    unsubscribeLang = subscribeLanguage(() => {
      langTick += 1;
    });

    try {
      const s = await api.getSettings();
      settings = s;
      if (s.language) setLanguage(s.language);
    } catch (e) {
      console.warn("Failed to load initial settings:", e);
    }

    try {
      profiles = await api.listProfiles();
    } catch (e) {
      console.warn("Failed to load profiles:", e);
    }

    try {
      const initStream = await api.getStreamState();
      updateStreamState(initStream);
      const initRecord = await api.getRecordState();
      updateRecordState(initRecord);
    } catch (e) {
      console.warn("Failed to load session states:", e);
    }

    try {
      unlistenStream = await api.onStreamState((state) => {
        updateStreamState(state);
        if (state && (state as any).last_error) {
          showToast((state as any).last_error);
        }
      });
      unlistenRecord = await api.onRecordState((state) => {
        updateRecordState(state);
        if (state && (state as any).last_error) {
          showToast((state as any).last_error);
        }
      });
    } catch (e) {
      console.warn("Could not register event listeners:", e);
    }
  });

  onDestroy(() => {
    if (unlistenStream) unlistenStream();
    if (unlistenRecord) unlistenRecord();
    if (unsubscribeLang) unsubscribeLang();
    if (toastTimer) clearTimeout(toastTimer);
    if (streamTimer) clearInterval(streamTimer);
    if (recordTimer) clearInterval(recordTimer);
  });

  const activeProfile = $derived(
    profiles.find((p) => p.id === settings.active_profile_id) || profiles[0] || null
  );

  async function handleStartStream() {
    const prof = activeProfile;
    if (!prof || prof.id === undefined) {
      showToast(t("errProfileRequired"));
      return;
    }
    try {
      await api.startStream(prof.id);
    } catch (e) {
      console.error(e);
      showToast(String(e));
    }
  }

  async function handleStopStream() {
    try {
      await api.stopStream();
    } catch (e) {
      console.error(e);
      showToast(String(e));
    }
  }

  async function handleStartRecord() {
    try {
      await api.startRecording();
    } catch (e) {
      console.error(e);
      showToast(String(e));
    }
  }

  async function handleStopRecord() {
    try {
      await api.stopRecording();
    } catch (e) {
      console.error(e);
      showToast(String(e));
    }
  }

  async function handleSelectProfile(id: number) {
    settings.active_profile_id = id;
    try {
      await api.saveSettings(settings);
    } catch (e) {
      console.error(e);
    }
  }

  async function handleSaveProfile(saved: Profile) {
    try {
      const id = await api.saveProfile(saved);
      profiles = await api.listProfiles();
      if (!settings.active_profile_id) {
        settings.active_profile_id = id;
        await api.saveSettings(settings);
      }
      showEditModal = false;
      editingProfile = null;
    } catch (e) {
      console.error(e);
      showToast(String(e));
    }
  }

  async function handleDeleteProfile(id: number) {
    try {
      await api.deleteProfile(id);
      profiles = await api.listProfiles();
      if (settings.active_profile_id === id) {
        settings.active_profile_id = profiles.length > 0 ? profiles[0].id : null;
        await api.saveSettings(settings);
      }
    } catch (e) {
      console.error(e);
      showToast(String(e));
    }
  }

  async function handleSaveSettings(updated: AppSettings) {
    settings = updated;
    try {
      await api.saveSettings(updated);
      showToast(t("savedSuccess"));
    } catch (e) {
      console.error(e);
      showToast(String(e));
    }
  }
</script>

<div class="app-layout">
  <!-- Key trick: langTick dependency triggers full reactivity when language updates -->
  {#key langTick}
    <Masthead onOpenSettings={() => (showSettingsModal = true)} />

    {#if globalToast}
      <div class="toast-bar">
        <span>{globalToast}</span>
        <button class="toast-close" onclick={() => (globalToast = null)}>&times;</button>
      </div>
    {/if}

    <main class="console-grid">
      <!-- Left Column: Master Broadcast & Audio Control -->
      <section class="console-column">
        <HeroControl
          {streamState}
          hasActiveProfile={activeProfile !== null}
          onStartStream={handleStartStream}
          onStopStream={handleStopStream}
        />

        <NowPlaying />

        <div class="audio-strip">
          <VuMeter />
          <MicSlider
            initialGain={settings.mic_gain}
            selectedDevice={settings.mic_device}
            onDeviceChange={async (dev) => {
              settings.mic_device = dev || null;
              await api.saveSettings(settings);
            }}
          />
        </div>
      </section>

      <!-- Right Column: Recording & Server Profiles -->
      <section class="console-column">
        <RecordControl
          {recordState}
          recordWithStream={settings.record_with_stream}
          onToggleRecordWithStream={async (enabled) => {
            settings.record_with_stream = enabled;
            await api.toggleStreamRecording(enabled);
            await api.saveSettings(settings);
          }}
          onStartRecord={handleStartRecord}
          onStopRecord={handleStopRecord}
        />

        <ServerProfiles
          {profiles}
          activeProfileId={settings.active_profile_id ?? (activeProfile?.id ?? null)}
          isStreaming={streamState.status === "streaming"}
          onSelectProfile={handleSelectProfile}
          onEditProfile={(p) => {
            editingProfile = p;
            showEditModal = true;
          }}
          onDeleteProfile={handleDeleteProfile}
          onNewProfile={() => {
            editingProfile = null;
            showEditModal = true;
          }}
        />
      </section>
    </main>

    <!-- Modals -->
    {#if showEditModal}
      <EditProfile
        profile={editingProfile}
        onSave={handleSaveProfile}
        onCancel={() => {
          showEditModal = false;
          editingProfile = null;
        }}
      />
    {/if}

    {#if showSettingsModal}
      <SettingsModal
        {settings}
        onSaveSettings={handleSaveSettings}
        onClose={() => (showSettingsModal = false)}
      />
    {/if}
  {/key}
</div>

<style>
  .app-layout {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    background-color: var(--color-page);
  }

  .toast-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 16px;
    background-color: #0f172a;
    color: white;
    font-size: 13px;
    font-weight: 500;
  }

  .toast-close {
    background: transparent;
    border: none;
    color: #94a3b8;
    font-size: 18px;
    cursor: pointer;
  }

  .toast-close:hover {
    color: white;
  }

  .console-grid {
    flex: 1;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
    padding: 16px 20px;
    max-width: 1400px;
    margin: 0 auto;
    width: 100%;
  }

  @media (max-width: 900px) {
    .console-grid {
      grid-template-columns: 1fr;
    }
  }

  .console-column {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .audio-strip {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
</style>
