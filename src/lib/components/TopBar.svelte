<script lang="ts">
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { t } from "../../i18n/index.svelte";

  let { onsettings }: { onsettings: () => void } = $props();

  const pill = $derived.by(() => {
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

<header class="topbar">
  <div class="brand">
    <span class="logo"><Icon name="radio" size={18} /></span>
    <div>
      <div class="name">{t("appName")}</div>
      <div class="sub">{t("appSubtitle")}</div>
    </div>
  </div>
  <div class="row">
    {#if store.isRecording}
      <span class="pill rec" title={t("recordTitle")}><span class="dot"></span>REC</span>
    {/if}
    <span class="pill {pill.tone}" role="status"><span class="dot"></span>{pill.label}</span>
    <button class="btn btn-ghost btn-icon" onclick={onsettings} aria-label={t("settingsTitle")} title={t("settingsTitle")}>
      <Icon name="settings" size={18} />
    </button>
  </div>
</header>

<style>
  .topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 16px;
    background: var(--card);
    border-bottom: 1px solid var(--border);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .logo {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 8px;
    background: var(--primary);
    color: #fff;
    flex: none;
  }
  .name {
    font-weight: 800;
    font-size: 15px;
    line-height: 1.1;
  }
  .sub {
    font-size: 11px;
    color: var(--text-muted);
  }
  @media (max-width: 420px) {
    .sub {
      display: none;
    }
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 700;
    white-space: nowrap;
    background: var(--card-2);
    color: var(--text-muted);
    border: 1px solid var(--border);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: currentColor;
  }
  .pill.live,
  .pill.rec {
    background: var(--live);
    border-color: var(--live);
    color: #fff;
  }
  .pill.live .dot,
  .pill.rec .dot {
    animation: pulse 1.4s ease-in-out infinite;
  }
  .pill.busy {
    background: var(--warn-soft);
    border-color: transparent;
    color: var(--warn-text);
  }
  .pill.error {
    background: var(--danger-soft);
    border-color: transparent;
    color: var(--danger-text);
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
</style>
