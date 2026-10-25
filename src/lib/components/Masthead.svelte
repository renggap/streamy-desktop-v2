<script lang="ts">
  import { t, getLanguage, setLanguage, type Language } from "../../i18n";
  import { api } from "../api";

  interface Props {
    onOpenSettings: () => void;
  }

  let { onOpenSettings }: Props = $props();

  let lang = $state<Language>(getLanguage());

  function toggleLanguage() {
    const next: Language = lang === "en" ? "id" : "en";
    lang = next;
    setLanguage(next);
  }

  async function openLogs() {
    try {
      await api.openLogFolder();
    } catch (e) {
      console.error(e);
    }
  }
</script>

<header class="masthead">
  <div class="brand">
    <div class="logo-mark">
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
        <path d="M4.9 19.1C1 15.2 1 8.8 4.9 4.9"/>
        <path d="M7.8 16.2c-2.3-2.3-2.3-6.1 0-8.5"/>
        <circle cx="12" cy="12" r="2"/>
        <path d="M16.2 7.8c2.3 2.3 2.3 6.1 0 8.5"/>
        <path d="M19.1 4.9C23 8.8 23 15.2 19.1 19.1"/>
      </svg>
    </div>
    <div class="titles">
      <div class="title-row">
        <h1 class="app-title">{t("appName")}</h1>
        <span class="version-pill">v2.0</span>
      </div>
      <p class="app-subtitle">{t("appSubtitle")}</p>
    </div>
  </div>

  <div class="actions">
    <button class="btn-tool" onclick={toggleLanguage} title={t("settingLanguage")}>
      <span class="lang-flag">{lang === "en" ? "🇬🇧 EN" : "🇮🇩 ID"}</span>
    </button>

    <button class="btn-tool" onclick={openLogs} title={t("btnOpenLogs")}>
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/>
        <polyline points="14 2 14 8 20 8"/>
        <line x1="16" y1="13" x2="8" y2="13"/>
        <line x1="16" y1="17" x2="8" y2="17"/>
        <polyline points="10 9 9 9 8 9"/>
      </svg>
    </button>

    <button class="btn-tool" onclick={onOpenSettings} title={t("settingsTitle")}>
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="3"/>
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/>
      </svg>
    </button>
  </div>
</header>

<style>
  .masthead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 20px;
    background-color: var(--color-card);
    border-bottom: 1px solid var(--color-border);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .logo-mark {
    width: 38px;
    height: 38px;
    background: linear-gradient(135deg, var(--color-primary), var(--color-primary-hover));
    color: white;
    border-radius: 10px;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 2px 6px rgba(101, 163, 13, 0.35);
  }

  .titles {
    display: flex;
    flex-direction: column;
  }

  .title-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .app-title {
    font-size: 18px;
    font-weight: 700;
    color: var(--color-text-main);
    letter-spacing: -0.02em;
  }

  .version-pill {
    font-size: 11px;
    font-weight: 600;
    color: var(--color-primary-text);
    background-color: var(--color-primary-soft);
    padding: 2px 6px;
    border-radius: 4px;
  }

  .app-subtitle {
    font-size: 12px;
    color: var(--color-text-muted);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .btn-tool {
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 6px 10px;
    cursor: pointer;
    color: var(--color-text-muted);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    font-weight: 600;
    transition: all 0.15s ease;
  }

  .btn-tool:hover {
    background: var(--color-muted-bg);
    color: var(--color-text-main);
    border-color: var(--color-text-dim);
  }

  .lang-flag {
    letter-spacing: 0.05em;
  }
</style>
