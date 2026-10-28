<script lang="ts">
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { formatDuration } from "../format";
  import { t } from "../../i18n/index.svelte";

  const p = $derived(store.player);
  const current = $derived(store.currentTrack);
  const loaded = $derived(p.state !== "Stopped" && current !== null);
  const duration = $derived(p.duration_secs ?? current?.duration_secs ?? 0);
  const total = $derived(store.tracks.reduce((sum, tr) => sum + (tr.duration_secs ?? 0), 0));

  // While the user drags the seek bar, show their position instead of the player's.
  let scrub = $state<number | null>(null);
  const shown = $derived(scrub ?? store.position);
  let confirmClear = $state(false);

  function commitSeek() {
    if (scrub !== null) void store.seek(scrub);
    scrub = null;
  }

  function onMonitor(e: Event) {
    void store.saveSettings({ monitor_playlist: (e.currentTarget as HTMLInputElement).checked }, true);
  }
</script>

<section class="card">
  <div class="card-head">
    <div>
      <h2 class="card-title">{t("playlistTitle")}</h2>
      <p class="card-sub">
        {store.tracks.length
          ? t("trackCount", { count: store.tracks.length, duration: formatDuration(total) })
          : t("playlistFormats")}
      </p>
    </div>
    <div class="row">
      {#if store.tracks.length > 0}
        {#if confirmClear}
          <span class="hint">{t("confirmClearPlaylist")}</span>
          <button class="btn btn-sm btn-danger" onclick={() => { confirmClear = false; void store.clearPlaylist(); }}>{t("btnClearPlaylist")}</button>
          <button class="btn btn-sm" onclick={() => (confirmClear = false)}>{t("btnCancel")}</button>
        {:else}
          <button class="btn btn-sm btn-ghost" onclick={() => (confirmClear = true)}>{t("btnClearPlaylist")}</button>
        {/if}
      {/if}
      <button class="btn btn-sm btn-dark" onclick={() => store.pickFiles()}>
        <Icon name="file-plus" size={14} />{t("btnAddFiles")}
      </button>
    </div>
  </div>

  <!-- Transport -->
  <div class="rounded-2xl border border-gray-200 bg-gray-50 p-4">
    <div class="flex items-center gap-3">
      <span class="grid size-11 flex-none place-items-center rounded-xl {p.state === 'Playing' ? 'bg-lime-400 text-gray-900' : 'bg-gray-200 text-gray-500'}">
        <Icon name="music" size={20} />
      </span>
      <div class="min-w-0 flex-1">
        <div class="truncate font-semibold" title={current?.title}>{loaded ? current?.title : t("nothingPlaying")}</div>
        <div class="text-xs text-gray-500">
          {#if p.state === "Paused"}{t("statePaused")}{:else if p.state === "Playing"}{t("statePlaying")}{:else}{t("stateStopped")}{/if}
        </div>
      </div>
      <div class="flex items-center gap-1">
        <button class="btn btn-ghost btn-icon" onclick={() => store.previous()} disabled={!store.tracks.length} aria-label={t("btnPrevious")} title={t("btnPrevious")}>
          <Icon name="skip-back" />
        </button>
        <button
          class="grid size-11 cursor-pointer place-items-center rounded-full bg-gray-900 text-lime-400 shadow-sm transition hover:bg-gray-800 disabled:cursor-not-allowed disabled:opacity-40"
          onclick={() => store.togglePlay()}
          disabled={!store.tracks.length}
          aria-label={p.state === "Playing" ? t("btnPause") : t("btnPlay")}
          title={p.state === "Playing" ? t("btnPause") : t("btnPlay")}
        >
          <Icon name={p.state === "Playing" ? "pause" : "play"} size={18} />
        </button>
        <button class="btn btn-ghost btn-icon" onclick={() => store.next()} disabled={!store.tracks.length} aria-label={t("btnNext")} title={t("btnNext")}>
          <Icon name="skip-forward" />
        </button>
        <button class="btn btn-ghost btn-icon" onclick={() => store.stopPlayer()} disabled={p.state === "Stopped"} aria-label={t("btnStopPlayer")} title={t("btnStopPlayer")}>
          <Icon name="square" />
        </button>
      </div>
    </div>

    <div class="mt-3 flex items-center gap-3">
      <span class="w-12 text-right font-mono text-xs text-gray-500 tabular-nums">{formatDuration(shown)}</span>
      <input
        type="range"
        class="flex-1"
        min="0"
        max={Math.max(duration, 0.1)}
        step="0.1"
        value={loaded ? shown : 0}
        disabled={!loaded || !duration}
        oninput={(e) => (scrub = Number(e.currentTarget.value))}
        onchange={commitSeek}
        aria-label={t("seekLabel")}
        aria-valuetext={formatDuration(shown)}
      />
      <span class="w-12 font-mono text-xs text-gray-500 tabular-nums">{duration ? formatDuration(duration) : "--:--"}</span>
    </div>

    <div class="mt-3 flex flex-wrap items-center gap-x-5 gap-y-2">
      <label class="flex min-w-40 flex-1 items-center gap-2 text-gray-500" title={t("volumeLabel")}>
        <Icon name="volume" />
        <input
          type="range"
          min="0"
          max="1"
          step="0.01"
          value={p.volume}
          oninput={(e) => store.setPlayerVolume(Number(e.currentTarget.value))}
          aria-label={t("volumeLabel")}
        />
        <span class="w-9 text-right font-mono text-xs tabular-nums">{Math.round(p.volume * 100)}%</span>
      </label>
      <label class="switch">
        <input type="checkbox" checked={p.auto_advance} onchange={(e) => store.setPlayerOptions(e.currentTarget.checked, p.repeat)} />
        <span></span>{t("optAutoAdvance")}
      </label>
      <label class="switch">
        <input type="checkbox" checked={p.repeat} onchange={(e) => store.setPlayerOptions(p.auto_advance, e.currentTarget.checked)} />
        <span></span>{t("optRepeat")}
      </label>
      <label class="switch">
        <input type="checkbox" checked={store.settings.monitor_playlist} onchange={onMonitor} />
        <span></span>{t("optMonitor")}
      </label>
    </div>

    {#if store.input !== "playlist" && p.state === "Playing"}
      <div class="banner banner-warn mt-3">
        <Icon name="info" /><span class="grow">{t("playlistNotOnAir")}</span>
        <button class="btn btn-sm btn-dark" onclick={() => store.setInput("playlist")}>{t("btnPutOnAir")}</button>
      </div>
    {/if}
  </div>

  <!-- Queue -->
  {#if store.tracks.length === 0}
    <button
      class="flex cursor-pointer flex-col items-center gap-2 rounded-2xl border-2 border-dashed border-gray-200 px-4 py-10 text-center text-gray-500 transition hover:border-lime-400 hover:bg-lime-50"
      onclick={() => store.pickFiles()}
    >
      <span class="grid size-12 place-items-center rounded-xl bg-gray-100 text-gray-600"><Icon name="upload" size={22} /></span>
      <span class="font-medium text-gray-800">{t("playlistEmpty")}</span>
      <span class="text-xs">{t("playlistFormats")}</span>
    </button>
  {:else}
    <ol class="flex max-h-[420px] flex-col gap-1 overflow-y-auto pr-1">
      {#each store.tracks as track, i (track.id)}
        {@const isCurrent = track.id === p.current_id && p.state !== "Stopped"}
        <li
          class="group flex items-center gap-3 rounded-xl px-3 py-2 transition
            {isCurrent ? 'bg-gray-900 text-white' : 'hover:bg-gray-50'}"
          ondblclick={() => store.play(track.id)}
        >
          <button
            class="grid size-8 flex-none cursor-pointer place-items-center rounded-lg text-xs font-medium tabular-nums
              {isCurrent ? 'bg-lime-400 text-gray-900' : 'bg-gray-100 text-gray-500 group-hover:bg-gray-900 group-hover:text-lime-400'}"
            onclick={() => (isCurrent && p.state === "Playing" ? store.togglePlay() : store.play(track.id))}
            aria-label={`${t("btnPlay")}: ${track.title}`}
            title={t("btnPlay")}
          >
            {#if isCurrent && p.state === "Playing"}
              <Icon name="pause" size={12} />
            {:else}
              <span class="group-hover:hidden">{i + 1}</span>
              <span class="hidden group-hover:inline"><Icon name="play" size={12} /></span>
            {/if}
          </button>
          <div class="min-w-0 flex-1">
            <div class="truncate text-[13px] font-medium" title={track.path}>{track.title}</div>
          </div>
          <span class="font-mono text-xs tabular-nums {isCurrent ? 'text-gray-300' : 'text-gray-500'}">
            {track.duration_secs ? formatDuration(track.duration_secs) : "--:--"}
          </span>
          <div class="flex items-center opacity-0 transition group-focus-within:opacity-100 group-hover:opacity-100">
            <button
              class="grid size-7 cursor-pointer place-items-center rounded-lg hover:bg-gray-500/20 disabled:invisible"
              onclick={() => store.moveTrack(track.id, i - 1)}
              disabled={i === 0}
              aria-label={t("btnMoveUp")}
              title={t("btnMoveUp")}
            ><Icon name="arrow-up" size={14} /></button>
            <button
              class="grid size-7 cursor-pointer place-items-center rounded-lg hover:bg-gray-500/20 disabled:invisible"
              onclick={() => store.moveTrack(track.id, i + 1)}
              disabled={i === store.tracks.length - 1}
              aria-label={t("btnMoveDown")}
              title={t("btnMoveDown")}
            ><Icon name="arrow-down" size={14} /></button>
            <button
              class="grid size-7 cursor-pointer place-items-center rounded-lg hover:bg-red-500/20 hover:text-red-600"
              onclick={() => store.removeTrack(track.id)}
              aria-label={t("btnRemove")}
              title={t("btnRemove")}
            ><Icon name="x" size={14} /></button>
          </div>
        </li>
      {/each}
    </ol>
    <p class="hint">{t("dropHint")}</p>
  {/if}
</section>
