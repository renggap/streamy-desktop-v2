<script lang="ts">
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { formatDuration, versionTag } from "../format";
  import { t } from "../../i18n/index.svelte";
  import type { Page } from "../../types";
  let { page, onnavigate }: { page: Page; onnavigate: (p: Page) => void } = $props();

  const NAV: Array<{ id: Page; icon: "layout" | "server" | "settings"; label: () => string }> = [
    { id: "studio", icon: "layout", label: () => t("navStudio") },
    { id: "servers", icon: "server", label: () => t("navServers") },
    { id: "settings", icon: "settings", label: () => t("navSettings") },
  ];

  const update = $derived.by(() => {
    const u = store.update;
    switch (u.state) {
      case "downloading":
        return t("updateDownloading", { version: versionTag(u.version), percent: u.percent == null ? "" : ` · ${u.percent}%` });
      case "ready":
        return t("updateReady", { version: versionTag(u.version) });
      case "installing":
        return t("updateInstalling", { version: versionTag(u.version) });
      default:
        return null;
    }
  });

  const status = $derived.by(() => {
    const s = store.status.stream;
    switch (s.type) {
      case "Live":
        return { tone: "live", label: t("statusLive") };
      case "Connecting":
        return { tone: "busy", label: t("statusConnecting") };
      case "Reconnecting":
        return { tone: "busy", label: t("statusReconnecting", { attempt: s.payload.attempt }) };
      case "Error":
        return { tone: "error", label: t("statusError") };
      default:
        return { tone: "idle", label: t("statusIdle") };
    }
  });
</script>

<aside class="flex w-60 flex-none flex-col bg-gray-900 text-gray-300">
  <div class="flex items-center gap-3 px-5 pt-6 pb-8">
    <span class="grid size-10 place-items-center rounded-xl bg-lime-400 text-gray-900">
      <Icon name="radio" size={20} />
    </span>
    <div class="min-w-0">
      <div class="text-lg leading-tight font-semibold text-white">{t("appName")}</div>
      <div class="truncate text-[11px] text-gray-400">{t("appSubtitle")}</div>
    </div>
  </div>

  <nav class="flex flex-col gap-1 px-3">
    {#each NAV as item (item.id)}
      <button
        class="flex cursor-pointer items-center gap-3 rounded-xl px-3.5 py-2.5 text-left text-[13px] font-medium transition
          {page === item.id ? 'bg-lime-400 text-gray-900' : 'text-gray-400 hover:bg-gray-800 hover:text-white'}"
        aria-current={page === item.id ? "page" : undefined}
        onclick={() => onnavigate(item.id)}
      >
        <Icon name={item.icon} size={18} />
        {item.label()}
      </button>
    {/each}
  </nav>

  <div class="mt-auto p-3">
    <div class="rounded-2xl bg-gray-800 p-4">
      <div class="flex items-center gap-2 text-xs font-semibold" role="status">
        <span
          class="pulse-dot {status.tone === 'live'
            ? 'text-red-500'
            : status.tone === 'busy'
              ? 'text-amber-400'
              : status.tone === 'error'
                ? 'text-red-400 animate-none'
                : 'text-gray-500 animate-none'}"
        ></span>
        <span class="truncate {status.tone === 'live' ? 'text-white' : 'text-gray-300'}">{status.label}</span>
      </div>
      {#if store.isStreaming}
        <div class="mt-2 font-mono text-xl font-semibold text-white tabular-nums">
          {formatDuration(store.status.stats.live_secs)}
        </div>
      {/if}
      <div class="mt-2 flex flex-wrap gap-1.5">
        <span class="badge bg-gray-700 text-gray-200">
          <Icon name={store.input === "playlist" ? "music" : "mic"} size={12} />
          {store.input === "playlist" ? t("sourcePlaylist") : t("sourceMic")}
        </span>
        {#if store.isRecording}
          <span class="badge bg-red-600 text-white"><span class="pulse-dot"></span>REC</span>
        {/if}
      </div>
    </div>
    <div class="mt-3 flex items-center gap-2 px-1.5 text-[11px]">
      {#if store.info}
        <span class="font-mono text-gray-500" title={store.info.version}>{versionTag(store.info.version)}</span>
      {/if}
      {#if update}
        <span class="flex min-w-0 items-center gap-1 text-lime-400" title={t("updateHint")} role="status">
          <Icon name={store.update.state === "installing" ? "refresh" : "download"} size={12} />
          <span class="truncate">{update}</span>
        </span>
      {/if}
    </div>
  </div>
</aside>
