<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import { t } from "../../i18n/index.svelte";

  let {
    title,
    onclose,
    children,
    footer,
    onback,
  }: {
    title: string;
    onclose: () => void;
    children: Snippet;
    footer?: Snippet;
    onback?: () => void;
  } = $props();

  let dialog: HTMLDivElement;

  $effect(() => {
    const prev = document.activeElement as HTMLElement | null;
    dialog?.focus();
    return () => prev?.focus?.();
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      onclose();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="backdrop" onmousedown={(e) => e.target === e.currentTarget && onclose()} {onkeydown}>
  <div class="modal" role="dialog" aria-modal="true" aria-label={title} tabindex="-1" bind:this={dialog}>
    <header>
      {#if onback}
        <button class="btn btn-ghost btn-icon" onclick={onback} aria-label={t("btnBack")}>
          <Icon name="arrow-left" />
        </button>
      {/if}
      <h2 class="grow truncate">{title}</h2>
      <button class="btn btn-ghost btn-icon" onclick={onclose} aria-label={t("btnClose")}>
        <Icon name="x" />
      </button>
    </header>
    <div class="body">
      {@render children()}
    </div>
    {#if footer}
      <footer>{@render footer()}</footer>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(2, 6, 23, 0.5);
    display: flex;
    align-items: flex-end;
    justify-content: center;
    z-index: 50;
    padding: 12px;
  }
  @media (min-height: 600px) {
    .backdrop {
      align-items: center;
    }
  }
  .modal {
    width: 100%;
    max-width: 560px;
    max-height: calc(100vh - 24px);
    display: flex;
    flex-direction: column;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: var(--shadow-lg);
    outline: none;
  }
  header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 10px 10px 16px;
    border-bottom: 1px solid var(--border);
  }
  h2 {
    font-size: 16px;
    font-weight: 700;
  }
  .body {
    padding: 16px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 16px;
    border-top: 1px solid var(--border);
  }
</style>
