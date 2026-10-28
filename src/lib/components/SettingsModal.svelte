<script lang="ts">
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import { store } from "../store.svelte";
  import { api, errorText } from "../api";
  import { t } from "../../i18n/index.svelte";
  import type { AppSettings } from "../../types";

  let { onclose }: { onclose: () => void } = $props();

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

<Modal title={t("settingsTitle")} {onclose}>
  <section>
    <h3>{t("sectionGeneral")}</h3>
    <label class="field">
      <span>{t("settingLanguage")}</span>
      <div class="segmented" role="group" aria-label={t("settingLanguage")}>
        <button type="button" aria-pressed={store.settings.language === "en"} onclick={() => set("language", "en")}>English</button>
        <button type="button" aria-pressed={store.settings.language === "id"} onclick={() => set("language", "id")}>Bahasa Indonesia</button>
      </div>
    </label>
  </section>

  <section>
    <h3>{t("sectionBroadcast")}</h3>
    <label class="check">
      <input type="checkbox" checked={store.settings.auto_reconnect} onchange={(e) => set("auto_reconnect", e.currentTarget.checked)} />
      <span>{t("settingAutoReconnect")}</span>
    </label>
    <label class="check">
      <input type="checkbox" checked={store.settings.record_with_stream} onchange={(e) => set("record_with_stream", e.currentTarget.checked)} />
      <span>{t("settingRecordWithStream")}</span>
    </label>
    <label class="field">
      <span>{t("settingRecordingsFolder")}</span>
      <div class="row">
        <input
          class="input grow mono"
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

  <section>
    <h3>{t("sectionStartup")}</h3>
    <label class="check">
      <input type="checkbox" checked={store.settings.launch_at_login} onchange={(e) => set("launch_at_login", e.currentTarget.checked)} />
      <span>{t("settingLaunchAtLogin")}</span>
    </label>
    <label class="check">
      <input type="checkbox" checked={store.settings.start_minimized} onchange={(e) => set("start_minimized", e.currentTarget.checked)} />
      <span>{t("settingStartMinimized")}</span>
    </label>
  </section>

  <section>
    <h3>{t("sectionAbout")}</h3>
    {#if store.info}
      <p class="hint">{t("aboutVersion", { version: store.info.version, lame: store.info.lame_version })}</p>
    {/if}
    <div class="row wrap">
      <button class="btn btn-sm" onclick={() => store.openLogs()}><Icon name="folder" size={14} />{t("btnOpenLogs")}</button>
      <button class="btn btn-sm" onclick={toggleLame} aria-expanded={lame !== null}><Icon name="info" size={14} />{t("btnLameNotice")}</button>
    </div>
    {#if lame !== null}
      <pre class="notice">{lame}</pre>
    {/if}
  </section>
</Modal>

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  h3 {
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .wrap {
    flex-wrap: wrap;
  }
  .notice {
    max-height: 200px;
    overflow: auto;
    padding: 10px;
    border-radius: var(--radius-sm);
    background: var(--card-2);
    border: 1px solid var(--border);
    font-family: var(--font-mono);
    font-size: 11px;
    white-space: pre-wrap;
    user-select: text;
  }
</style>
