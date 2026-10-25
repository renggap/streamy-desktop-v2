<script lang="ts">
  import type { StreamState } from "../../types";
  import { t } from "../../i18n";
  import StateBadge from "./StateBadge.svelte";

  interface Props {
    streamState: StreamState;
    hasActiveProfile: boolean;
    onStartStream: () => void;
    onStopStream: () => void;
  }

  let { streamState, hasActiveProfile, onStartStream, onStopStream }: Props = $props();

  const isLive = $derived(
    streamState.status === "streaming" ||
    streamState.status === "connecting" ||
    streamState.status === "reconnecting"
  );

  const isStreaming = $derived(streamState.status === "streaming");

  function formatDuration(totalSeconds: number): string {
    const h = Math.floor(totalSeconds / 3600);
    const m = Math.floor((totalSeconds % 3600) / 60);
    const s = totalSeconds % 60;
    const pad = (n: number) => n.toString().padStart(2, "0");
    if (h > 0) {
      return `${pad(h)}:${pad(m)}:${pad(s)}`;
    }
    return `${pad(m)}:${pad(s)}`;
  }

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
  }
</script>

<div class="hero-control card">
  <div class="hero-header">
    <div class="profile-info">
      <span class="eyebrow">{t("profileSelect")}</span>
      <span class="active-name">{streamState.profile_name || t("noProfileSelected")}</span>
    </div>
    <StateBadge status={streamState.status} reconnectAttempt={streamState.reconnect_attempt} />
  </div>

  <div class="hero-main">
    <button
      class="btn-hero {isLive ? 'btn-stop' : 'btn-start'}"
      disabled={!hasActiveProfile && !isLive}
      onclick={() => {
        if (isLive) {
          onStopStream();
        } else {
          onStartStream();
        }
      }}
    >
      {#if isLive}
        <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
          <rect x="6" y="6" width="12" height="12" rx="2" />
        </svg>
        <span>{t("btnStopStream")}</span>
      {:else}
        <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
          <polygon points="6 4 20 12 6 20 6 4" />
        </svg>
        <span>{t("btnStartStream")}</span>
      {/if}
    </button>
  </div>

  <!-- Telemetry stats during / after streaming -->
  <div class="telemetry-grid">
    <div class="telemetry-item">
      <span class="tele-label">{t("duration")}</span>
      <span class="tele-val font-mono">{formatDuration(streamState.duration_secs)}</span>
    </div>
    <div class="telemetry-item">
      <span class="tele-label">{t("dataSent")}</span>
      <span class="tele-val font-mono">{formatBytes(streamState.bytes_sent)}</span>
    </div>
    <div class="telemetry-item">
      <span class="tele-label">{t("fieldBitrate")}</span>
      <span class="tele-val font-mono">{streamState.current_bitrate_kbps > 0 ? `${streamState.current_bitrate_kbps} kbps` : "--"}</span>
    </div>
  </div>

  <!-- Drop / Backpressure alert banners -->
  {#if streamState.dropped_frames > 0}
    <div class="warning-banner drop-warn">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/>
        <line x1="12" y1="9" x2="12" y2="13"/>
        <line x1="12" y1="17" x2="12.01" y2="17"/>
      </svg>
      <span>{t("fpsDropped", { count: streamState.dropped_frames })}</span>
    </div>
  {/if}

  {#if streamState.backpressure_pct > 0}
    <div class="warning-banner backpressure-warn">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="12" cy="12" r="10"/>
        <line x1="12" y1="8" x2="12" y2="12"/>
        <line x1="12" y1="16" x2="12.01" y2="16"/>
      </svg>
      <span>{t("backpressureWarn", { pct: streamState.backpressure_pct })}</span>
    </div>
  {/if}

  {#if streamState.last_error}
    <div class="error-banner">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="12" cy="12" r="10"/>
        <line x1="15" y1="9" x2="9" y2="15"/>
        <line x1="9" y1="9" x2="15" y2="15"/>
      </svg>
      <span>{streamState.last_error}</span>
    </div>
  {/if}
</div>

<style>
  .hero-control {
    gap: 16px;
  }

  .hero-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .profile-info {
    display: flex;
    flex-direction: column;
  }

  .eyebrow {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--color-text-dim);
    letter-spacing: 0.05em;
  }

  .active-name {
    font-size: 16px;
    font-weight: 700;
    color: var(--color-text-main);
  }

  .hero-main {
    display: flex;
  }

  .btn-hero {
    width: 100%;
    padding: 16px;
    border-radius: 10px;
    border: none;
    font-size: 18px;
    font-weight: 700;
    letter-spacing: 0.01em;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    transition: all 0.15s ease;
    box-shadow: 0 4px 10px rgba(0, 0, 0, 0.1);
  }

  .btn-start {
    background-color: var(--color-primary);
    color: white;
  }
  .btn-start:hover:not(:disabled) {
    background-color: var(--color-primary-hover);
  }

  .btn-stop {
    background-color: var(--color-danger);
    color: white;
  }
  .btn-stop:hover:not(:disabled) {
    background-color: var(--color-danger-hover);
  }

  .btn-hero:disabled {
    opacity: 0.5;
    cursor: not-allowed;
    box-shadow: none;
  }

  .telemetry-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
    background-color: var(--color-muted-bg);
    border-radius: 8px;
    padding: 10px 14px;
  }

  .telemetry-item {
    display: flex;
    flex-direction: column;
  }

  .tele-label {
    font-size: 11px;
    color: var(--color-text-muted);
    font-weight: 600;
    text-transform: uppercase;
  }

  .tele-val {
    font-size: 14px;
    font-weight: 600;
    color: var(--color-text-main);
  }

  .font-mono {
    font-family: var(--font-mono);
  }

  .warning-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 500;
  }

  .drop-warn {
    background-color: var(--color-warn-soft);
    color: var(--color-warn-text);
    border: 1px solid rgba(217, 119, 6, 0.2);
  }

  .backpressure-warn {
    background-color: #fef08a;
    color: #854d0e;
    border: 1px solid #facc15;
  }

  .error-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 500;
    background-color: var(--color-danger-soft);
    color: var(--color-danger-text);
    border: 1px solid rgba(220, 38, 38, 0.2);
  }
</style>
