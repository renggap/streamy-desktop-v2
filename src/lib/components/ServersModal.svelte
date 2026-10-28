<script lang="ts">
  import { untrack } from "svelte";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import ProfileEditor from "./ProfileEditor.svelte";
  import { emptyProfile, store } from "../store.svelte";
  import type { Profile } from "../../types";
  import { t } from "../../i18n/index.svelte";

  let { onclose, startNew = false }: { onclose: () => void; startNew?: boolean } = $props();

  // null = list view; otherwise the profile being edited (id 0 = new).
  let editing = $state<Profile | null>(untrack(() => startNew) ? emptyProfile() : null);
  let editor = $state<ReturnType<typeof ProfileEditor>>();
  let saving = $state(false);
  let confirming = $state<number | "all" | null>(null);

  async function save(p: Profile) {
    saving = true;
    const id = await store.saveProfile(p);
    saving = false;
    if (id == null) return;
    // A brand-new server is almost always the one the user wants to use next.
    if (p.id === 0 && !store.isStreaming) await store.selectProfile(id);
    editing = null;
    if (startNew) onclose();
  }

  async function remove(id: number) {
    confirming = null;
    await store.deleteProfile(id);
  }

  async function removeAll() {
    confirming = null;
    await store.clearProfiles();
  }

  const title = $derived(editing ? (editing.id ? t("editServerTitle") : t("newServerTitle")) : t("serversTitle"));
  const selectedId = $derived(store.settings.selected_profile_id);
</script>

<Modal {title} {onclose} onback={editing && !startNew ? () => (editing = null) : undefined}>
  {#if editing}
    {#key editing}
      <ProfileEditor bind:this={editor} profile={editing} onsubmit={save} />
    {/key}
  {:else}
    {#if store.profiles.length === 0}
      <p class="hint empty">{t("serversEmpty")}</p>
    {/if}
    <ul class="list">
      {#each store.profiles as p (p.id)}
        <li class:selected={p.id === selectedId}>
          <div class="grow">
            <div class="row">
              <strong class="truncate">{p.name}</strong>
              <span class="tag">{p.mode === "DJ" ? (p.dj_protocol === "ICECAST" ? "AzuraCast DJ" : "DJ") : "Source"}</span>
            </div>
            <div class="hint mono truncate">
              {p.host}:{p.mode === "DJ" && p.dj_port ? p.dj_port : p.port} · {p.bitrate_kbps} kbps
            </div>
          </div>
          {#if confirming === p.id}
            <div class="confirm">
              <span class="hint">{t("confirmDelete", { name: p.name })}</span>
              <div class="row">
                <button class="btn btn-sm btn-danger" onclick={() => remove(p.id)}>{t("btnDelete")}</button>
                <button class="btn btn-sm" onclick={() => (confirming = null)}>{t("btnCancel")}</button>
              </div>
            </div>
          {:else}
            {#if p.id === selectedId}
              <span class="in-use"><Icon name="check" size={14} />{t("inUse")}</span>
            {:else}
              <button class="btn btn-sm" onclick={() => store.selectProfile(p.id)} disabled={store.isStreaming}>{t("btnUse")}</button>
            {/if}
            <button class="btn btn-sm btn-ghost btn-icon" onclick={() => (editing = p)} aria-label={t("btnEdit")} title={t("btnEdit")}>
              <Icon name="pencil" size={14} />
            </button>
            <button
              class="btn btn-sm btn-ghost btn-icon"
              onclick={() => (confirming = p.id)}
              disabled={store.isStreaming && p.id === selectedId}
              aria-label={t("btnDelete")}
              title={t("btnDelete")}
            >
              <Icon name="trash" size={14} />
            </button>
          {/if}
        </li>
      {/each}
    </ul>
    {#if store.profiles.length > 1}
      {#if confirming === "all"}
        <div class="banner banner-error">
          <div class="grow">{t("confirmClearAll")}</div>
          <button class="btn btn-sm btn-danger" onclick={removeAll}>{t("btnDelete")}</button>
          <button class="btn btn-sm" onclick={() => (confirming = null)}>{t("btnCancel")}</button>
        </div>
      {:else}
        <button class="btn btn-ghost btn-sm danger-link" onclick={() => (confirming = "all")} disabled={store.isStreaming}>
          {t("btnDeleteAll")}
        </button>
      {/if}
    {/if}
  {/if}

  {#snippet footer()}
    {#if editing}
      <button class="btn" onclick={() => (startNew ? onclose() : (editing = null))}>{t("btnCancel")}</button>
      <button class="btn btn-primary" onclick={() => editor?.submit()} disabled={saving}>
        {#if saving}<span class="spinner"></span>{/if}
        {t("btnSave")}
      </button>
    {:else}
      <button class="btn btn-primary" onclick={() => (editing = emptyProfile())}><Icon name="plus" />{t("btnNewServer")}</button>
    {/if}
  {/snippet}
</Modal>

<style>
  .empty {
    text-align: center;
    padding: 12px;
  }
  .list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  li {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 10px 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--card-2);
    flex-wrap: wrap;
  }
  li.selected {
    border-color: var(--primary);
  }
  .tag {
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--border);
    color: var(--text-muted);
    flex: none;
  }
  .in-use {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    font-weight: 700;
    color: var(--primary-text);
    padding: 0 6px;
  }
  .confirm {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
  }
  .danger-link {
    align-self: flex-start;
    color: var(--danger-text);
  }
</style>
