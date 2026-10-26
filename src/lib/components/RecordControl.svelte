<script lang="ts">
  import type { RecordState } from "../../types";
  import { t } from "../../i18n";
  import { api } from "../api";

  interface Props {
    recordState: RecordState;
    recordWithStream: boolean;
    onToggleRecordWithStream: (enabled: boolean) => void;
    onStartRecord: () => void;
    onStopRecord: () => void;
  }

  let {
    recordState,
    recordWithStream,
    onToggleRecordWithStream,
    onStartRecord,
    onStopRecord,
  }: Props = $props();

  const isRecording = $derived(recordState.status === "recording");

  function formatDuration(totalSeconds: number | null | undefined): string {
    if (totalSeconds == null || isNaN(totalSeconds) || totalSeconds <= 0) {
      return "--";
    }
    const h = Math.floor(totalSeconds / 3600);
    const m = Math.floor((totalSeconds % 3600) / 60);
    const s = totalSeconds % 60;
    const pad = (n: number) => n.toString().padStart(2, "0");
    if (h > 0) {
      return `${pad(h)}:${pad(m)}:${pad(s)}`;
    }
    return `${pad(m)}:${pad(s)}`;
  }

  function formatBytes(bytes: number | null | undefined): string {
    if (bytes == null || isNaN(bytes) || bytes <= 0) {
      return "--";
    }
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
  }

  async function openFolder() {
    try {
      await api.openRecordingsFolder();
    } catch (e) {
      console.error(e);
    }
  }
</script>

<div class="card record-card">
  <div class="record-header">
    <div class="record-title-row">
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="12" cy="12" r="10"/>
        <circle cx="12" cy="12" r="4" fill="currentColor"/>
      </svg>
      <span class="record-title">{t("statusRecording")}</span>
      {#if isRecording}
        <span class="rec-dot"></span>
      {/if}
    </div>

    <button class="btn-folder" onclick={openFolder} title={t("btnOpenRecordings")}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
      </svg>
      <span>{t("btnOpenRecordings")}</span>
    </button>
  </div>

  <div class="record-body">
    <button
      class="btn-record {isRecording ? 'recording' : ''}"
      onclick={() => {
        if (isRecording) {
          onStopRecord();
        } else {
          onStartRecord();
        }
      }}
    >
      {#if isRecording}
        <span class="rec-square"></span>
        <span>{t("btnStopRecord")}</span>
      {:else}
        <span class="rec-circle"></span>
        <span>{t("btnStartRecord")}</span>
      {/if}
    </button>

    <div class="record-stats">
      <div class="stat-col">
        <span class="stat-label">{t("duration")}</span>
        <span class="stat-val font-mono">{formatDuration(recordState.duration_secs)}</span>
      </div>
      <div class="stat-col">
        <span class="stat-label">{t("dataSent")}</span>
        <span class="stat-val font-mono">{formatBytes(recordState.bytes_written)}</span>
      </div>
    </div>
  </div>

  <label class="auto-rec-toggle">
    <input
      type="checkbox"
      checked={recordWithStream}
      onchange={(e) => onToggleRecordWithStream(e.currentTarget.checked)}
    />
    <span>{t("settingRecordWithStream")}</span>
  </label>
</div>

<style>
  .record-card {
    gap: 12px;
  }

  .record-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .record-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 700;
    color: var(--color-text-main);
  }

  .rec-dot {
    width: 8px;
    height: 8px;
    background-color: var(--color-danger);
    border-radius: 50%;
    animation: blink 1s infinite;
  }

  @keyframes blink {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.2; }
  }

  .btn-folder {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--color-text-muted);
    background: transparent;
    border: 1px solid var(--color-border);
    padding: 4px 8px;
    border-radius: 6px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .btn-folder:hover {
    background-color: var(--color-muted-bg);
    color: var(--color-text-main);
  }

  .record-body {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .btn-record {
    flex: 1;
    padding: 10px 16px;
    border-radius: 8px;
    border: 1px solid var(--color-border);
    background-color: var(--color-page);
    color: var(--color-text-main);
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    transition: all 0.15s ease;
  }

  .btn-record:hover {
    background-color: var(--color-muted-bg);
    border-color: var(--color-danger);
  }

  .btn-record.recording {
    background-color: var(--color-danger-soft);
    color: var(--color-danger);
    border-color: var(--color-danger);
  }

  .rec-circle {
    width: 10px;
    height: 10px;
    background-color: var(--color-danger);
    border-radius: 50%;
  }

  .rec-square {
    width: 10px;
    height: 10px;
    background-color: var(--color-danger);
    border-radius: 2px;
  }

  .record-stats {
    flex: 1;
    display: flex;
    gap: 16px;
  }

  .stat-col {
    display: flex;
    flex-direction: column;
  }

  .stat-label {
    font-size: 10px;
    text-transform: uppercase;
    font-weight: 600;
    color: var(--color-text-muted);
  }

  .stat-val {
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text-main);
  }

  .auto-rec-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--color-text-muted);
    cursor: pointer;
  }

  .auto-rec-toggle input {
    accent-color: var(--color-primary);
  }

  .font-mono {
    font-family: var(--font-mono);
  }
</style>
