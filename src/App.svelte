<script lang="ts">
  import { onMount } from "svelte";
  import { store } from "./lib/store.svelte";
  import { t } from "./i18n/index.svelte";
  import TopBar from "./lib/components/TopBar.svelte";
  import OnAirPanel from "./lib/components/OnAirPanel.svelte";
  import AudioPanel from "./lib/components/AudioPanel.svelte";
  import NowPlaying from "./lib/components/NowPlaying.svelte";
  import RecordPanel from "./lib/components/RecordPanel.svelte";
  import ServersModal from "./lib/components/ServersModal.svelte";
  import SettingsModal from "./lib/components/SettingsModal.svelte";
  import Toasts from "./lib/components/Toasts.svelte";

  let modal = $state<null | "servers" | "new-server" | "settings">(null);

  onMount(() => {
    void store.init();
    return () => store.destroy();
  });
</script>

<div class="app">
  <TopBar onsettings={() => (modal = "settings")} />

  {#if !store.ready}
    <div class="loading"><span class="spinner"></span>{t("loading")}</div>
  {:else}
    <main>
      <div class="col">
        <OnAirPanel onmanage={() => (modal = "servers")} onadd={() => (modal = "new-server")} />
        <NowPlaying />
      </div>
      <div class="col">
        <AudioPanel />
        <RecordPanel />
      </div>
    </main>
  {/if}
</div>

{#if modal === "servers" || modal === "new-server"}
  <ServersModal startNew={modal === "new-server"} onclose={() => (modal = null)} />
{:else if modal === "settings"}
  <SettingsModal onclose={() => (modal = null)} />
{/if}

<Toasts />

<style>
  .app {
    height: 100vh;
    display: flex;
    flex-direction: column;
  }
  main {
    flex: 1;
    overflow-y: auto;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    align-content: start;
    gap: 14px;
    padding: 14px 16px 80px;
    max-width: 1100px;
    width: 100%;
    margin: 0 auto;
  }
  @media (max-width: 760px) {
    main {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .col {
    display: flex;
    flex-direction: column;
    gap: 14px;
    min-width: 0;
  }
  .loading {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    color: var(--text-muted);
  }
</style>
