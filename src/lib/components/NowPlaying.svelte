<script lang="ts">
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { t } from "../../i18n/index.svelte";

  let title = $state("");
  let sending = $state(false);

  const current = $derived(store.status.stats.now_playing);
  const live = $derived(store.streamType === "Live");

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const text = title.trim();
    if (!text || sending) return;
    sending = true;
    if (await store.updateNowPlaying(text)) title = "";
    sending = false;
  }
</script>

<section class="card">
  <div>
    <h2 class="card-title">{t("nowPlayingTitle")}</h2>
    <p class="card-sub">{store.input === "playlist" && store.settings.send_track_titles ? t("nowPlayingAuto") : t("nowPlayingSub")}</p>
  </div>
  <form class="row" onsubmit={submit}>
    <input
      class="input grow"
      bind:value={title}
      maxlength="200"
      placeholder={t("nowPlayingPlaceholder")}
      aria-label={t("nowPlayingTitle")}
    />
    <button class="btn btn-dark" type="submit" disabled={!title.trim() || sending}>
      {#if sending}<span class="spinner"></span>{:else}<Icon name="send" size={14} />{/if}
      {t("btnSendTitle")}
    </button>
  </form>
  {#if current}
    <p class="hint flex items-center gap-1.5 truncate" title={current}>
      <Icon name="music" size={12} />
      {live ? t("nowPlayingCurrent", { title: current }) : t("nowPlayingQueued", { title: current })}
    </p>
  {/if}
</section>
