<script lang="ts">
  import { t } from "../../i18n";
  import { api } from "../api";

  let trackTitle = $state("");
  let isSending = $state(false);
  let statusMsg = $state<string | null>(null);

  async function handleSend() {
    if (!trackTitle.trim()) return;
    isSending = true;
    try {
      await api.updateNowPlaying(trackTitle.trim());
      statusMsg = t("toastNowPlayingSent");
      setTimeout(() => {
        statusMsg = null;
      }, 3000);
    } catch (e) {
      console.error(e);
      statusMsg = String(e);
    } finally {
      isSending = false;
      // Title sticks on the row after update button pressed!
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }
</script>

<div class="card now-playing-card">
  <div class="card-title-row">
    <div class="title-with-icon">
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M12 2v20M17 5v14M7 8v8M2 11v2M22 11v2"/>
      </svg>
      <h3 class="card-title">{t("nowPlayingTitle")}</h3>
    </div>
    {#if statusMsg}
      <span class="status-chip">{statusMsg}</span>
    {/if}
  </div>

  <div class="single-row-entry">
    <div class="input-wrap">
      <label for="track-title" class="sr-only">{t("nowPlayingTrack")}</label>
      <input
        id="track-title"
        type="text"
        class="text-input"
        placeholder={t("nowPlayingPlaceholderTrack")}
        bind:value={trackTitle}
        onkeydown={handleKeydown}
      />
    </div>

    <button
      class="btn-send"
      disabled={!trackTitle.trim() || isSending}
      onclick={handleSend}
    >
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M21.5 2v6h-6M21.34 15.57a10 10 0 1 1-.57-8.38l5.67-5.67"/>
      </svg>
      <span>{t("nowPlayingSend")}</span>
    </button>
  </div>
</div>

<style>
  .now-playing-card {
    gap: 12px;
  }

  .card-title-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .title-with-icon {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--color-text-main);
  }

  .card-title {
    font-size: 14px;
    font-weight: 700;
  }

  .status-chip {
    font-size: 11px;
    font-weight: 600;
    color: var(--color-live-text);
    background-color: var(--color-live-soft);
    padding: 2px 8px;
    border-radius: 9999px;
  }

  .single-row-entry {
    display: flex;
    gap: 10px;
    align-items: center;
  }

  .input-wrap {
    flex: 1;
    display: flex;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  .text-input {
    width: 100%;
    padding: 9px 12px;
    border-radius: 8px;
    background-color: var(--color-bg-base);
    border: 1px solid var(--color-border);
    color: var(--color-text-main);
    font-size: 13px;
    outline: none;
    transition: border-color 0.15s ease;
  }

  .text-input:focus {
    border-color: var(--color-primary);
  }

  .btn-send {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 9px 16px;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 600;
    background-color: var(--color-primary);
    color: #ffffff;
    border: none;
    cursor: pointer;
    transition: opacity 0.15s ease, background-color 0.15s ease;
    white-space: nowrap;
  }

  .btn-send:hover:not(:disabled) {
    opacity: 0.9;
  }

  .btn-send:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
</style>
