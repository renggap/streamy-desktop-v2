<script lang="ts">
  import { untrack } from "svelte";
  import Icon from "./Icon.svelte";
  import ProfileEditor from "./ProfileEditor.svelte";
  import { emptyProfile, store } from "../store.svelte";
  import type { Profile } from "../../types";
  import { t } from "../../i18n/index.svelte";

  let { startNew = false, ondone }: { startNew?: boolean; ondone?: () => void } = $props();

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
    if (startNew) ondone?.();
  }

  function cancel() {
    editing = null;
    if (startNew) ondone?.();
  }

  async function remove(id: number) {
    confirming = null;
    await store.deleteProfile(id);
  }

  async function removeAll() {
    confirming = null;
    await store.clearProfiles();
  }

  const selectedId = $derived(store.settings.selected_profile_id);
</script>

{#if editing}
  <section class="card mx-auto w-full max-w-2xl">
    <div class="card-head">
      <div class="row">
        <button class="btn btn-ghost btn-icon" onclick={cancel} aria-label={t("btnBack")} title={t("btnBack")}>
          <Icon name="arrow-left" />
        </button>
        <h2 class="card-title">{editing.id ? t("editServerTitle") : t("newServerTitle")}</h2>
      </div>
    </div>
    {#key editing}
      <ProfileEditor bind:this={editor} profile={editing} onsubmit={save} />
    {/key}
    <div class="flex justify-end gap-2 border-t border-gray-100 pt-4">
      <button class="btn" onclick={cancel}>{t("btnCancel")}</button>
      <button class="btn btn-dark" onclick={() => editor?.submit()} disabled={saving}>
        {#if saving}<span class="spinner"></span>{/if}
        {t("btnSave")}
      </button>
    </div>
  </section>
{:else}
  <section class="card">
    <div class="card-head">
      <div>
        <h2 class="card-title">{t("serversTitle")}</h2>
        <p class="card-sub">{t("serversSub")}</p>
      </div>
      <button class="btn btn-dark" onclick={() => (editing = emptyProfile())}><Icon name="plus" />{t("btnNewServer")}</button>
    </div>

    {#if store.profiles.length === 0}
      <div class="flex flex-col items-center gap-2 rounded-2xl border-2 border-dashed border-gray-200 px-4 py-10 text-center text-gray-500">
        <span class="grid size-12 place-items-center rounded-xl bg-gray-100 text-gray-600"><Icon name="server" size={22} /></span>
        <span>{t("serversEmpty")}</span>
      </div>
    {/if}

    <ul class="grid gap-3 xl:grid-cols-2">
      {#each store.profiles as p (p.id)}
        {@const inUse = p.id === selectedId}
        <li class="flex flex-wrap items-center gap-3 rounded-2xl border p-4 {inUse ? 'border-gray-900 ring-1 ring-gray-900' : 'border-gray-200'}">
          <span class="grid size-10 flex-none place-items-center rounded-xl {inUse ? 'bg-lime-400 text-gray-900' : 'bg-gray-100 text-gray-500'}">
            <Icon name="server" />
          </span>
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <strong class="truncate font-semibold">{p.name}</strong>
              <span class="badge bg-gray-100 text-gray-600">{p.mode === "DJ" ? (p.dj_protocol === "ICECAST" ? "AzuraCast DJ" : "DJ") : "Source"}</span>
            </div>
            <div class="truncate font-mono text-xs text-gray-500">
              {p.host}:{p.mode === "DJ" && p.dj_port ? p.dj_port : p.port} · {p.bitrate_kbps} kbps
            </div>
          </div>
          {#if confirming === p.id}
            <div class="flex w-full flex-col gap-2 rounded-xl bg-red-50 p-3">
              <span class="text-xs text-red-800">{t("confirmDelete", { name: p.name })}</span>
              <div class="row">
                <button class="btn btn-sm btn-danger" onclick={() => remove(p.id)}>{t("btnDelete")}</button>
                <button class="btn btn-sm" onclick={() => (confirming = null)}>{t("btnCancel")}</button>
              </div>
            </div>
          {:else}
            <div class="flex items-center gap-1">
              {#if inUse}
                <span class="badge bg-gray-900 text-lime-400"><Icon name="check" size={12} />{t("inUse")}</span>
              {:else}
                <button class="btn btn-sm" onclick={() => store.selectProfile(p.id)} disabled={store.isStreaming}>{t("btnUse")}</button>
              {/if}
              <button class="btn btn-sm btn-ghost btn-icon" onclick={() => (editing = p)} aria-label={t("btnEdit")} title={t("btnEdit")}>
                <Icon name="pencil" size={14} />
              </button>
              <button
                class="btn btn-sm btn-ghost btn-icon hover:!text-red-600"
                onclick={() => (confirming = p.id)}
                disabled={store.isStreaming && inUse}
                aria-label={t("btnDelete")}
                title={t("btnDelete")}
              >
                <Icon name="trash" size={14} />
              </button>
            </div>
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
        <button class="btn btn-ghost btn-sm self-start text-red-700" onclick={() => (confirming = "all")} disabled={store.isStreaming}>
          {t("btnDeleteAll")}
        </button>
      {/if}
    {/if}
  </section>
{/if}
