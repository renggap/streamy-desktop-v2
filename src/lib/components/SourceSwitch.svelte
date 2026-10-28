<script lang="ts">
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { t } from "../../i18n/index.svelte";
  import type { InputKind } from "../../types";

  const playing = $derived(store.player.state === "Playing");
  const options = $derived<
    Array<{ id: InputKind; icon: "mic" | "music"; title: string; desc: string; warn: string | null }>
  >([
    {
      id: "mic",
      icon: "mic",
      title: t("sourceMic"),
      desc: t("sourceMicDesc"),
      warn: store.status.mic_available ? null : t("micUnavailable"),
    },
    {
      id: "playlist",
      icon: "music",
      title: t("sourcePlaylist"),
      desc: store.currentTrack && store.player.state !== "Stopped" ? store.currentTrack.title : t("sourcePlaylistDesc"),
      warn: null,
    },
  ]);
</script>

<section class="card">
  <div class="card-head">
    <div>
      <h2 class="card-title">{t("sourceTitle")}</h2>
      <p class="card-sub">{t("sourceHint")}</p>
    </div>
  </div>
  <div class="grid grid-cols-2 gap-3" role="radiogroup" aria-label={t("sourceTitle")}>
    {#each options as o (o.id)}
      {@const active = store.input === o.id}
      <button
        role="radio"
        aria-checked={active}
        class="group relative flex cursor-pointer items-center gap-3 rounded-2xl border p-4 text-left transition
          {active ? 'border-gray-900 bg-gray-900 text-white shadow-md' : 'border-gray-200 bg-white hover:border-gray-300 hover:bg-gray-50'}"
        onclick={() => store.setInput(o.id)}
      >
        <span
          class="grid size-11 flex-none place-items-center rounded-xl
            {active ? 'bg-lime-400 text-gray-900' : 'bg-gray-100 text-gray-500 group-hover:text-gray-900'}"
        >
          <Icon name={o.icon} size={20} />
        </span>
        <span class="min-w-0 flex-1">
          <span class="flex items-center gap-2">
            <span class="font-semibold">{o.title}</span>
            {#if active}
              <span class="badge {store.isStreaming ? 'bg-red-600 text-white' : 'bg-lime-400 text-gray-900'}">
                {#if store.isStreaming}<span class="pulse-dot"></span>{t("statusLive")}{:else}{t("sourceSelected")}{/if}
              </span>
            {/if}
          </span>
          <span class="mt-0.5 block truncate text-xs {active ? 'text-gray-300' : 'text-gray-500'}">
            {#if o.id === "playlist" && playing}<Icon name="play" size={10} />{/if}
            {o.desc}
          </span>
          {#if o.warn}
            <span class="mt-1 block text-xs font-medium {active ? 'text-amber-300' : 'text-amber-600'}">{o.warn}</span>
          {/if}
        </span>
      </button>
    {/each}
  </div>
</section>
