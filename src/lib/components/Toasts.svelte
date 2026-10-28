<script lang="ts">
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { t } from "../../i18n/index.svelte";
</script>

<div class="toasts" aria-live="polite">
  {#each store.toasts as toast (toast.id)}
    <div class="toast {toast.kind}" role={toast.kind === "error" ? "alert" : "status"}>
      <span class="icon">
        <Icon name={toast.kind === "error" ? "alert" : toast.kind === "success" ? "check" : "info"} />
      </span>
      <div class="grow">
        <div>{toast.message}</div>
        {#if toast.detail}
          <details class="detail">
            <summary>{t("errDetails")}</summary>
            <code>{toast.detail}</code>
          </details>
        {/if}
      </div>
      <button class="btn btn-ghost btn-sm" onclick={() => store.dismissToast(toast.id)} aria-label={t("btnDismiss")}>
        <Icon name="x" size={14} />
      </button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    left: 12px;
    right: 16px;
    bottom: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 100;
    pointer-events: none;
    align-items: flex-end;
  }
  .toast {
    pointer-events: auto;
    width: 100%;
    max-width: 420px;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 8px 10px 12px;
    border-radius: 10px;
    background: var(--color-gray-900);
    color: var(--color-white);
    border-radius: 14px;
    box-shadow: 0 20px 40px -12px rgb(0 0 0 / 0.35);
    font-size: 13px;
  }
  .toast .btn {
    color: inherit;
  }
  .icon {
    margin-top: 2px;
    display: flex;
  }
  .success .icon {
    color: var(--color-lime-400);
  }
  .error .icon {
    color: var(--meter-red);
  }
</style>
