<script lang="ts">
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { api, errorText } from "../api";
  import { t } from "../../i18n/index.svelte";
  import type { AppSettings } from "../../types";

  let folder = $state(store.settings.recordings_folder ?? "");
  let lame = $state<string | null>(null);

  function set<K extends keyof AppSettings>(key: K, value: AppSettings[K]) {
    void store.saveSettings({ [key]: value } as Partial<AppSettings>);
  }

  function saveFolder() {
    const v = folder.trim() || null;
    if (v !== (store.settings.recordings_folder ?? null)) set("recordings_folder", v);
  }

  async function toggleLame() {
    if (lame !== null) {
      lame = null;
      return;
    }
    try {
      lame = await api.getLameNotice();
    } catch (e) {
      lame = errorText(e);
    }
  }
</script>

<div class="grid gap-5 xl:grid-cols-2">
  <section class="card">
    <h2 class="card-title">{t("sectionGeneral")}</h2>
    <div class="field">
      <span>{t("settingLanguage")}</span>
      <div class="segmented" role="group" aria-label={t("settingLanguage")}>
        <button type="button" aria-pressed={store.settings.language === "en"} onclick={() => set("language", "en")}>English</button>
        <button type="button" aria-pressed={store.settings.language === "id"} onclick={() => set("language", "id")}>Bahasa Indonesia</button>
      </div>
    </div>
    <h2 class="card-title mt-2">{t("sectionStartup")}</h2>
    <label class="switch">
      <input type="checkbox" checked={store.settings.launch_at_login} onchange={(e) => set("launch_at_login", e.currentTarget.checked)} />
      <span></span>{t("settingLaunchAtLogin")}
    </label>
    <label class="switch">
      <input type="checkbox" checked={store.settings.start_minimized} onchange={(e) => set("start_minimized", e.currentTarget.checked)} />
      <span></span>{t("settingStartMinimized")}
    </label>
  </section>

  <section class="card">
    <h2 class="card-title">{t("sectionBroadcast")}</h2>
    <label class="switch">
      <input type="checkbox" checked={store.settings.auto_reconnect} onchange={(e) => set("auto_reconnect", e.currentTarget.checked)} />
      <span></span>{t("settingAutoReconnect")}
    </label>
    <label class="switch">
      <input type="checkbox" checked={store.settings.record_with_stream} onchange={(e) => set("record_with_stream", e.currentTarget.checked)} />
      <span></span>{t("settingRecordWithStream")}
    </label>
    <label class="field">
      <span>{t("settingRecordingsFolder")}</span>
      <div class="row">
        <input
          class="input grow font-mono"
          bind:value={folder}
          onblur={saveFolder}
          onkeydown={(e) => e.key === "Enter" && saveFolder()}
          placeholder={t("settingRecordingsFolderPlaceholder", { path: store.info?.recordings_dir ?? "" })}
          spellcheck="false"
        />
        <button class="btn btn-icon" onclick={() => store.openRecordings()} aria-label={t("btnOpenFolder")} title={t("btnOpenFolder")}>
          <Icon name="folder" />
        </button>
      </div>
    </label>
    <p class="hint">{t("closeHint")}</p>
  </section>

  <section class="card">
    <h2 class="card-title">{t("sectionPlaylist")}</h2>
    <label class="switch">
      <input type="checkbox" checked={store.settings.send_track_titles} onchange={(e) => set("send_track_titles", e.currentTarget.checked)} />
      <span></span>{t("optSendTitles")}
    </label>
    <label class="switch">
      <input type="checkbox" checked={store.settings.monitor_playlist} onchange={(e) => set("monitor_playlist", e.currentTarget.checked)} />
      <span></span>{t("optMonitor")}
    </label>
    <p class="hint">{t("playlistSettingsHint")}</p>
  </section>

  <section class="card">
    <h2 class="card-title">{t("sectionAbout")}</h2>
    {#if store.info}
      <p class="hint">{t("aboutVersion", { version: store.info.version, lame: store.info.lame_version })}</p>
    {/if}
    <div class="row flex-wrap">
      <button class="btn btn-sm" onclick={() => store.openLogs()}><Icon name="folder" size={14} />{t("btnOpenLogs")}</button>
      <button class="btn btn-sm" onclick={toggleLame} aria-expanded={lame !== null}><Icon name="info" size={14} />{t("btnLameNotice")}</button>
    </div>
    {#if lame !== null}
      <pre class="max-h-52 overflow-auto rounded-xl border border-gray-200 bg-gray-50 p-3 font-mono text-[11px] whitespace-pre-wrap select-text">{lame}</pre>
    {/if}
  </section>
</div>
