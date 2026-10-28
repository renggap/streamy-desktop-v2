<script lang="ts">
  import { onMount } from "svelte";
  import { store } from "./lib/store.svelte";
  import { t } from "./i18n/index.svelte";
  import type { Page } from "./types";
  import Icon from "./lib/components/Icon.svelte";
  import Sidebar from "./lib/components/Sidebar.svelte";
  import OnAirHero from "./lib/components/OnAirHero.svelte";
  import SourceSwitch from "./lib/components/SourceSwitch.svelte";
  import PlaylistPanel from "./lib/components/PlaylistPanel.svelte";
  import AudioPanel from "./lib/components/AudioPanel.svelte";
  import NowPlaying from "./lib/components/NowPlaying.svelte";
  import RecordPanel from "./lib/components/RecordPanel.svelte";
  import ServersPage from "./lib/components/ServersPage.svelte";
  import SettingsPage from "./lib/components/SettingsPage.svelte";
  import Toasts from "./lib/components/Toasts.svelte";

  let page = $state<Page>("studio");
  /** Opened from "Add server": go straight to the editor, then back to the studio. */
  let newServer = $state(false);

  const TITLES: Record<Page, () => string> = {
    studio: () => t("navStudio"),
    servers: () => t("navServers"),
    settings: () => t("navSettings"),
  };

  function go(p: Page, startNew = false) {
    newServer = startNew;
    page = p;
  }

  onMount(() => {
    void store.init();
    return () => store.destroy();
  });
</script>

<div class="flex h-screen">
  <Sidebar {page} onnavigate={(p) => go(p)} />

  <main class="flex min-w-0 flex-1 flex-col">
    <header class="flex items-center justify-between gap-4 border-b border-gray-200 bg-white px-8 py-4">
      <div>
        <h1 class="text-xl font-semibold">{TITLES[page]()}</h1>
        {#if page === "studio"}
          <p class="text-xs text-gray-500">{t("studioSub")}</p>
        {/if}
      </div>
      {#if store.isRecording}
        <span class="badge bg-red-50 text-red-700"><span class="pulse-dot"></span>REC</span>
      {/if}
    </header>

    <div class="flex-1 overflow-y-auto px-8 py-6">
      {#if !store.ready}
        <div class="flex h-full items-center justify-center gap-2.5 text-gray-500"><span class="spinner"></span>{t("loading")}</div>
      {:else if page === "studio"}
        <div class="mx-auto grid max-w-[1400px] gap-5 lg:grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)]">
          <div class="flex min-w-0 flex-col gap-5">
            <OnAirHero onservers={() => go("servers")} onaddserver={() => go("servers", true)} />
            <SourceSwitch />
            <PlaylistPanel />
          </div>
          <div class="flex min-w-0 flex-col gap-5">
            <AudioPanel />
            <NowPlaying />
            <RecordPanel />
          </div>
        </div>
      {:else if page === "servers"}
        <div class="mx-auto max-w-[1400px]">
          {#key newServer}
            <ServersPage startNew={newServer} ondone={() => go("studio")} />
          {/key}
        </div>
      {:else}
        <div class="mx-auto max-w-[1400px]"><SettingsPage /></div>
      {/if}
    </div>
  </main>
</div>

{#if store.dragging}
  <div class="pointer-events-none fixed inset-0 z-40 grid place-items-center bg-gray-900/60 p-8 backdrop-blur-sm">
    <div class="flex flex-col items-center gap-3 rounded-3xl border-2 border-dashed border-lime-400 bg-gray-900 px-12 py-10 text-white">
      <span class="grid size-14 place-items-center rounded-2xl bg-lime-400 text-gray-900"><Icon name="upload" size={26} /></span>
      <span class="text-lg font-semibold">{t("dropTitle")}</span>
      <span class="text-xs text-gray-400">{t("playlistFormats")}</span>
    </div>
  </div>
{/if}

<Toasts />
