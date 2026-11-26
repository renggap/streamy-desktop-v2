<script lang="ts">
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import type { SavedTitle } from "../../types";
  import { t } from "../../i18n/index.svelte";

  let title = $state("");
  let sending = $state(false);

  const current = $derived(store.status.stats.now_playing);
  const live = $derived(store.streamType === "Live");
  const saved = $derived(store.settings.saved_titles);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!title.trim() || sending) return;
    // Title text stays after sending: the user often sends the same title again.
    sending = true;
    await store.sendTitle(title);
    sending = false;
  }

  function useSaved(s: SavedTitle) {
    title = s.text;
    sending = true;
    void store.sendTitle(s.text).finally(() => (sending = false));
  }

  // Inline rename: the row's text becomes an input until Enter (save) or Escape (cancel).
  let editing = $state<number | null>(null);
  let draft = $state("");
  function startEdit(i: number, s: SavedTitle) {
    editing = i;
    draft = s.text;
  }
  function commitEdit(oldText: string) {
    editing = null;
    void store.editTitle(oldText, draft);
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

  <!-- Saved titles: one click sends; x removes. -->
  <div class="rounded-xl border border-gray-200 bg-gray-50 p-3">
    <div class="flex items-center justify-between gap-2">
      <span class="text-xs font-medium text-gray-700">{t("savedTitlesTitle")}</span>
      <span class="text-[11px] text-gray-400">{saved.length}/10</span>
    </div>
    {#if saved.length === 0}
      <p class="hint mt-1">{t("savedTitlesEmpty")}</p>
    {:else}
      <ul class="mt-2 flex flex-col gap-1">
        {#each saved as s, i (i)}
          <li class="group flex items-center gap-1 rounded-lg px-1 py-0.5 hover:bg-white">
            {#if editing === i}
              <input
                class="input min-w-0 flex-1 py-1 text-[13px]"
                bind:value={draft}
                maxlength="200"
                aria-label={t("btnEditTitle")}
                onkeydown={(e) => {
                  if (e.key === "Enter") commitEdit(s.text);
                  if (e.key === "Escape") editing = null;
                }}
                onblur={() => editing === i && commitEdit(s.text)}
              />
              <button
                class="grid size-7 flex-none cursor-pointer place-items-center rounded-lg text-gray-400 hover:bg-gray-900 hover:text-white"
                onmousedown={(e) => e.preventDefault()}
                onclick={() => commitEdit(s.text)}
                aria-label={t("btnSaveTitle")}
                title={t("btnSaveTitle")}
              ><Icon name="check" size={14} /></button>
            {:else}
              <button
                class="min-w-0 flex-1 cursor-pointer truncate py-1 text-left text-[13px] text-gray-800 disabled:opacity-60"
                onclick={() => useSaved(s)}
                disabled={sending}
                title={t("btnUseTitle")}
              >{s.text}</button>
              <button
                class="grid size-7 flex-none cursor-pointer place-items-center rounded-lg
                  {s.pinned ? 'bg-lime-300 text-gray-900 hover:bg-gray-900 hover:text-lime-300' : 'text-gray-400 hover:bg-gray-900 hover:text-white'}"
                onclick={() => store.togglePin(s.text)}
                aria-pressed={s.pinned}
                aria-label={s.pinned ? t("btnUnpinTitle") : t("btnPinTitle")}
                title={s.pinned ? t("btnUnpinTitle") : t("btnPinTitle")}
              ><Icon name="pin" size={14} /></button>
              <button
                class="grid size-7 flex-none cursor-pointer place-items-center rounded-lg text-gray-400 hover:bg-gray-900 hover:text-white"
                onclick={() => startEdit(i, s)}
                aria-label={`${t("btnEditTitle")}: ${s.text}`}
                title={t("btnEditTitle")}
              ><Icon name="pencil" size={14} /></button>
            {/if}
            <button
              class="grid size-7 flex-none cursor-pointer place-items-center rounded-lg text-gray-400 hover:bg-red-500/10 hover:text-red-600"
              onclick={() => store.removeTitle(s.text)}
              aria-label={`${t("btnRemoveTitle")}: ${s.text}`}
              title={t("btnRemoveTitle")}
            ><Icon name="x" size={14} /></button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</section>
