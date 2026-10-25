<script lang="ts">
  import type { StreamStatus } from "../../types";
  import { t } from "../../i18n";

  interface Props {
    status: StreamStatus;
    reconnectAttempt?: number;
  }

  let { status, reconnectAttempt = 0 }: Props = $props();

  const badgeClass = $derived(() => {
    switch (status) {
      case "streaming":
        return "badge-live";
      case "reconnecting":
      case "connecting":
      case "teardown":
        return "badge-warn";
      default:
        return "badge-idle";
    }
  });

  const label = $derived(() => {
    switch (status) {
      case "streaming":
        return t("statusStreaming");
      case "connecting":
        return t("statusConnecting");
      case "reconnecting":
        return t("statusReconnecting", { attempt: reconnectAttempt });
      case "teardown":
        return t("statusTeardown");
      default:
        return t("statusIdle");
    }
  });
</script>

<div class="state-badge {badgeClass()}">
  {#if status === "streaming"}
    <span class="pulse-dot"></span>
  {/if}
  <span>{label()}</span>
</div>

<style>
  .state-badge {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 14px;
    border-radius: 9999px;
    font-weight: 600;
    font-size: 13px;
    letter-spacing: 0.02em;
    text-transform: uppercase;
  }

  .badge-idle {
    background-color: var(--color-muted-bg);
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
  }

  .badge-live {
    background-color: var(--color-live-soft);
    color: var(--color-live-text);
    border: 1px solid rgba(22, 163, 74, 0.3);
  }

  .badge-warn {
    background-color: var(--color-warn-soft);
    color: var(--color-warn-text);
    border: 1px solid rgba(217, 119, 6, 0.3);
  }

  .pulse-dot {
    width: 8px;
    height: 8px;
    background-color: var(--color-live);
    border-radius: 50%;
    animation: pulse 1.5s infinite;
  }

  @keyframes pulse {
    0% {
      transform: scale(0.95);
      box-shadow: 0 0 0 0 rgba(22, 163, 74, 0.7);
    }
    70% {
      transform: scale(1);
      box-shadow: 0 0 0 6px rgba(22, 163, 74, 0);
    }
    100% {
      transform: scale(0.95);
      box-shadow: 0 0 0 0 rgba(22, 163, 74, 0);
    }
  }
</style>
