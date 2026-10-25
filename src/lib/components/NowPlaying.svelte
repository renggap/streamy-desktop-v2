<script lang="ts">
  import { t } from "../../i18n";
  import { api } from "../api";

  let trackTitle = $state("");
  let artistName = $state("");
  let isSending = $state(false);
  let statusMsg = $state<string | null>(null);

  async function handleSend() {
    if (!trackTitle.trim()) return;
    isSending = true;
    try {
      await api.updateNowPlaying(trackTitle.trim(), artistName.trim() || undefined);
      statusMsg = t("nowPlayingSent");
      setTimeout(() => {
        statusMsg = null;
      }, 2500);
    } catch (e) {
      console.error(e);
      statusMsg = String(e);
    } finally {
      isSending = false;
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
        <path d="M9 18V5l12-2v13"/>
        <circle cx="6" cy="18" r="3"/>
        <circle cx="18" cy="16" r="3"/>
      </svg>
      <h3 class="card-title">{t("nowPlayingTitle")}</h3>
    </div>
    {#if statusMsg}
      <span class="status-chip">{statusMsg}</span>
    {/if}
  </div>

  <div class="inputs-row">
    <div class="input-col flex-2">
      <label for="track-title" class="input-label">{t("nowPlayingTrack")}</label>
      <input
        id="track-title"
        type="text"
        class="text-input"
        placeholder={t("nowPlayingPlaceholderTrack")}
        bind:value={trackTitle}
        onkeydown={handleKeydown}
      />
    </div>

    <div class="input-col flex-2">
      <label for="artist-name" class="input-label">{t("nowPlayingArtist")}</label>
      <input
        id="artist-name"
        type="text"
        class="text-input"
        placeholder={t("nowPlayingPlaceholderArtist")}
        bind:value={artistName}
        onkeydown={handleKeydown}
      />
    </div>

    <div class="input-col flex-btn">
      <span class="input-label" aria-hidden="true">&nbsp;</span>
      <button
        class="btn-send"
        disabled={!trackTitle.trim() || isSending}
        onclick={handleSend}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="22" y1="2" x2="11" y2="13"/>
          <polygon points="22 2 15 22 11 13 2 9 22 2"/>
        </svg>
        <span>{t("nowPlayingSend")}</span>
      </button>
    </div>
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

  .inputs-row {
    display: flex;
    gap: 10px;
    align-items: flex-end;
  }

  .input-col {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .flex-2 {
    flex: 2;
  }

  .flex-btn {
    flex: 1;
    min-width: 120px;
  }

  .input-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
  }

  .text-input {
    padding: 8px 12px;
    border: 1px solid var(--color-border);
    border-radius: 6px;
    font-size: 13px;
    color: var(--color-text-main);
    background-color: var(--color-page);
    outline: none;
    transition: border 0.15s ease;
  }

  .text-input:focus {
    border-color: var(--color-primary);
    background-color: white;
  }

  .btn-send {
    padding: 8px 14px;
    background-color: var(--color-page);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text-main);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    transition: all 0.15s ease;
  }

  .btn-send:hover:not(:disabled) {
    background-color: var(--color-primary-soft);
    color: var(--color-primary-text);
    border-color: var(--color-primary);
  }

  .btn-send:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
