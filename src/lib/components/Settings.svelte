<script lang="ts">
  import { onMount } from "svelte";
  import { t, getLanguage, setLanguage, type Language } from "../../i18n";
  import { api } from "../api";
  import type { AppSettings, AudioDeviceDesc } from "../../types";

  interface Props {
    settings: AppSettings;
    onSaveSettings: (settings: AppSettings) => Promise<void>;
    onClose: () => void;
  }

  let { settings, onSaveSettings, onClose }: Props = $props();

  let activeTab = $state<"general" | "audio" | "legal">("general");

  // Local copy of settings for editing
  let micDevice = $state<string | null>(null);
  let micGain = $state<number>(1.0);
  let recordWithStream = $state<boolean>(false);
  let autoReconnect = $state<boolean>(true);
  let startMinimized = $state<boolean>(false);
  let autostart = $state<boolean>(false);
  let language = $state<Language>("en");

  let devices = $state<AudioDeviceDesc[]>([]);
  let lameNotice = $state<string>("");
  let isSaving = $state(false);

  onMount(async () => {
    micDevice = settings.mic_device || null;
    micGain = settings.mic_gain ?? 1.0;
    recordWithStream = settings.record_with_stream ?? false;
    autoReconnect = settings.auto_reconnect ?? true;
    startMinimized = settings.start_minimized ?? false;
    autostart = settings.autostart ?? false;
    language = settings.language || getLanguage();

    try {
      devices = await api.listInputDevices();
      lameNotice = await api.getLameNotice();
    } catch (e) {
      console.warn("Error fetching settings assets:", e);
    }
  });

  async function handleSave() {
    isSaving = true;
    const updated: AppSettings = {
      ...settings,
      mic_device: micDevice,
      mic_gain: micGain,
      record_with_stream: recordWithStream,
      auto_reconnect: autoReconnect,
      start_minimized: startMinimized,
      autostart: autostart,
      language: language,
    };

    setLanguage(language);

    try {
      await onSaveSettings(updated);
      onClose();
    } catch (e) {
      console.error(e);
    } finally {
      isSaving = false;
    }
  }

  async function openLogs() {
    try {
      await api.openLogFolder();
    } catch (e) {
      console.error(e);
    }
  }

  async function openRecordings() {
    try {
      await api.openRecordingsFolder();
    } catch (e) {
      console.error(e);
    }
  }

  // ponytail: no toast plumbing here; folders open visibly on success, errors logged. Add prop callback if needed.
</script>

<div
  class="modal-backdrop"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={(e) => { if (e.target === e.currentTarget) onClose(); }}
  onkeydown={(e) => { if (e.key === "Escape") onClose(); }}
>
  <div class="modal-window card">
    <div class="modal-header">
      <h2 class="modal-title">{t("settingsTitle")}</h2>
      <button class="btn-close" onclick={onClose}>&times;</button>
    </div>

    <!-- Tabs Navigation -->
    <div class="tabs-bar">
      <button
        class="tab-btn {activeTab === 'general' ? 'active' : ''}"
        onclick={() => (activeTab = "general")}
      >
        General
      </button>
      <button
        class="tab-btn {activeTab === 'audio' ? 'active' : ''}"
        onclick={() => (activeTab = "audio")}
      >
        Audio & Mic
      </button>
      <button
        class="tab-btn {activeTab === 'legal' ? 'active' : ''}"
        onclick={() => (activeTab = "legal")}
      >
        {t("legalTitle")}
      </button>
    </div>

    <div class="modal-body">
      {#if activeTab === "general"}
        <div class="settings-group">
          <label class="setting-row">
            <input type="checkbox" bind:checked={autoReconnect} />
            <div class="setting-desc">
              <span class="setting-name">{t("settingAutoReconnect")}</span>
              <span class="setting-hint">Reconnects with exponential backoff and bounded jitter</span>
            </div>
          </label>

          <label class="setting-row">
            <input type="checkbox" bind:checked={recordWithStream} />
            <div class="setting-desc">
              <span class="setting-name">{t("settingRecordWithStream")}</span>
              <span class="setting-hint">Automatically saves a local MP3 recording during broadcast</span>
            </div>
          </label>

          <label class="setting-row">
            <input type="checkbox" bind:checked={startMinimized} />
            <div class="setting-desc">
              <span class="setting-name">{t("settingStartMinimized")}</span>
              <span class="setting-hint">Hides to system tray when launched</span>
            </div>
          </label>

          <label class="setting-row">
            <input type="checkbox" bind:checked={autostart} />
            <div class="setting-desc">
              <span class="setting-name">{t("settingAutostart")}</span>
              <span class="setting-hint">Register Streamy in Windows startup tasks</span>
            </div>
          </label>

          <div class="setting-row-select">
            <span class="setting-name">{t("settingLanguage")}</span>
            <select class="input-select" bind:value={language}>
              <option value="en">English (UK/US)</option>
              <option value="id">Bahasa Indonesia</option>
            </select>
          </div>

          <div class="actions-group">
            <button class="btn-action" onclick={openRecordings}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
              </svg>
              <span>{t("btnOpenRecordings")}</span>
            </button>

            <button class="btn-action" onclick={openLogs}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/>
                <polyline points="14 2 14 8 20 8"/>
                <line x1="16" y1="13" x2="8" y2="13"/>
                <line x1="16" y1="17" x2="8" y2="17"/>
              </svg>
              <span>{t("btnOpenLogs")}</span>
            </button>
          </div>
        </div>

      {:else if activeTab === "audio"}
        <div class="settings-group">
          <div class="setting-field">
            <label for="set-device">{t("settingMicDevice")}</label>
            <select id="set-device" class="input-select" bind:value={micDevice}>
              <option value={null}>Windows Default Audio Device</option>
              {#each devices as d}
                <option value={d.name}>{d.name} {d.is_default ? "(Default)" : ""}</option>
              {/each}
            </select>
          </div>

          <div class="setting-field">
            <div class="label-row">
              <label for="set-gain">{t("settingMicGain")}</label>
              <span class="font-mono">{Math.round(micGain * 100)}%</span>
            </div>
            <input
              id="set-gain"
              type="range"
              min="0"
              max="2"
              step="0.05"
              class="gain-slider"
              bind:value={micGain}
            />
          </div>
        </div>

      {:else if activeTab === "legal"}
        <div class="legal-group">
          <h3 class="legal-subtitle">{t("lameNoticeTitle")}</h3>
          <pre class="lame-notice-box font-mono">{lameNotice}</pre>
        </div>
      {/if}
    </div>

    <div class="modal-footer">
      <button class="btn-cancel" onclick={onClose}>{t("btnClose")}</button>
      <button class="btn-save" disabled={isSaving} onclick={handleSave}>
        {t("btnSave")}
      </button>
    </div>
  </div>
</div>

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background-color: rgba(15, 23, 42, 0.6);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    padding: 20px;
  }

  .modal-window {
    width: 100%;
    max-width: 600px;
    max-height: 85vh;
    background: white;
    border-radius: 12px;
    box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.2);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    border-bottom: 1px solid var(--color-border);
    padding-bottom: 12px;
  }

  .modal-title {
    font-size: 16px;
    font-weight: 700;
  }

  .btn-close {
    background: transparent;
    border: none;
    font-size: 20px;
    cursor: pointer;
    color: var(--color-text-muted);
  }

  .tabs-bar {
    display: flex;
    gap: 8px;
    border-bottom: 1px solid var(--color-border);
    padding-bottom: 8px;
    margin-top: 8px;
  }

  .tab-btn {
    padding: 6px 14px;
    border-radius: 6px;
    border: none;
    background: transparent;
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text-muted);
    cursor: pointer;
  }

  .tab-btn:hover {
    background-color: var(--color-muted-bg);
    color: var(--color-text-main);
  }

  .tab-btn.active {
    background-color: var(--color-primary-soft);
    color: var(--color-primary-text);
  }

  .modal-body {
    flex: 1;
    overflow-y: auto;
    padding: 16px 0;
  }

  .settings-group {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .setting-row {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    cursor: pointer;
  }

  .setting-row input {
    margin-top: 3px;
    accent-color: var(--color-primary);
  }

  .setting-desc {
    display: flex;
    flex-direction: column;
  }

  .setting-name {
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text-main);
  }

  .setting-hint {
    font-size: 11px;
    color: var(--color-text-muted);
  }

  .setting-row-select {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .setting-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .label-row {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    font-weight: 600;
    color: var(--color-text-muted);
  }

  .input-select {
    padding: 8px 12px;
    border: 1px solid var(--color-border);
    border-radius: 6px;
    background-color: var(--color-page);
    font-size: 13px;
    color: var(--color-text-main);
    outline: none;
  }

  .gain-slider {
    accent-color: var(--color-primary);
    cursor: pointer;
  }

  .actions-group {
    display: flex;
    gap: 10px;
    margin-top: 8px;
  }

  .btn-action {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    background-color: var(--color-page);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    color: var(--color-text-main);
  }

  .btn-action:hover {
    background-color: var(--color-muted-bg);
  }

  .legal-group {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .legal-subtitle {
    font-size: 13px;
    font-weight: 700;
  }

  .lame-notice-box {
    background-color: var(--color-page);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    padding: 12px;
    font-size: 11px;
    line-height: 1.4;
    max-height: 300px;
    overflow-y: auto;
    white-space: pre-wrap;
    color: var(--color-text-muted);
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    border-top: 1px solid var(--color-border);
    padding-top: 14px;
  }

  .btn-cancel {
    padding: 8px 16px;
    border: 1px solid var(--color-border);
    border-radius: 6px;
    background: transparent;
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text-muted);
    cursor: pointer;
  }

  .btn-save {
    padding: 8px 20px;
    border: none;
    border-radius: 6px;
    background-color: var(--color-primary);
    color: white;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .font-mono {
    font-family: var(--font-mono);
  }
</style>
