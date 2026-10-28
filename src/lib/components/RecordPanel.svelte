<script lang="ts">
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { fileName, formatBytes, formatDuration } from "../format";
  import { t } from "../../i18n/index.svelte";

  const rec = $derived(store.status.recording);
</script>

<section class="card" class:is-rec={rec.active}>
  <div class="card-head">
    <h2 class="card-title">{t("recordTitle")}</h2>
    <button class="btn btn-ghost btn-sm" onclick={() => store.openRecordings()}>
      <Icon name="folder" size={14} />{t("btnOpenFolder")}
    </button>
  </div>

  <div class="row main">
    <button
      class="btn {rec.active ? 'btn-danger' : ''}"
      onclick={() => store.toggleRecording()}
      disabled={store.pending !== null}
    >
      {#if store.pending === "record"}
        <span class="spinner"></span>
      {:else if rec.active}
        <Icon name="square" size={14} />
      {:else}
        <span class="red"><Icon name="record" size={14} /></span>
      {/if}
      {rec.active ? t("btnStopRecord") : t("btnRecord")}
    </button>

    {#if rec.active}
      <div class="grow status">
        <div class="row">
          <span class="dot"></span>
          <strong>{rec.with_stream ? t("recordingOfBroadcast") : t("recordingStandalone")}</strong>
        </div>
        <div class="hint mono">{formatDuration(rec.secs)} · {formatBytes(rec.bytes)}</div>
      </div>
    {:else}
      <p class="hint grow">{t("recordIdleHint")}</p>
    {/if}
  </div>

  {#if rec.active && rec.path}
    <p class="hint truncate mono" title={rec.path}>{fileName(rec.path)}</p>
  {:else if store.lastRecordingPath}
    <p class="hint truncate" title={store.lastRecordingPath}>{t("lastRecording", { file: fileName(store.lastRecordingPath) })}</p>
  {/if}

  <label class="check">
    <input
      type="checkbox"
      checked={store.settings.record_with_stream}
      onchange={(e) => store.saveSettings({ record_with_stream: e.currentTarget.checked }, true)}
    />
    <span>{t("recordWithStreamLabel")}</span>
  </label>
</section>

<style>
  .is-rec {
    border-color: var(--live);
  }
  .main {
    align-items: center;
    gap: 12px;
  }
  .red {
    color: var(--live);
    display: flex;
  }
  .status strong {
    font-size: 13px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--live);
    animation: pulse 1.4s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.3;
    }
  }
</style>
