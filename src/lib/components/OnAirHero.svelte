<script lang="ts">
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { friendlyError } from "../errors";
  import { formatBytes, formatDuration } from "../format";
  import { t } from "../../i18n/index.svelte";

  let { onservers, onaddserver }: { onservers: () => void; onaddserver: () => void } = $props();

  const stream = $derived(store.status.stream);
  const stats = $derived(store.status.stats);
  const profile = $derived(store.selectedProfile);
  const live = $derived(stream.type === "Live");
  const busy = $derived(stream.type === "Connecting" || stream.type === "Reconnecting");
  const error = $derived(
    stream.type === "Error" && !store.errorDismissed ? friendlyError(stream.payload.reason) : null,
  );
  const quality = $derived(
    profile
      ? t("qualityValue", {
          bitrate: profile.bitrate_kbps,
          rate: (profile.sample_rate_hz / 1000).toLocaleString(undefined, { maximumFractionDigits: 2 }),
          channels: profile.mono ? t("mono") : t("stereo"),
        })
      : "—",
  );

  function onselect(e: Event) {
    const v = (e.currentTarget as HTMLSelectElement).value;
    void store.selectProfile(v ? Number(v) : null);
  }
</script>

<section
  class="relative overflow-hidden rounded-2xl bg-gray-900 p-6 text-white shadow-sm ring-1
    {live ? 'ring-red-500/60' : 'ring-gray-900'}"
>
  <!-- soft accent glow -->
  <div
    class="pointer-events-none absolute -top-24 -right-24 size-64 rounded-full blur-3xl
      {live ? 'bg-red-500/25' : 'bg-lime-400/15'}"
  ></div>

  {#if store.profiles.length === 0}
    <div class="relative flex flex-col items-start gap-4">
      <span class="grid size-12 place-items-center rounded-xl bg-gray-800 text-lime-400"><Icon name="server" size={24} /></span>
      <div>
        <h2 class="text-xl font-semibold">{t("heroSetupTitle")}</h2>
        <p class="mt-1 max-w-md text-sm text-gray-400">{t("needProfileHint")}</p>
      </div>
      <button class="btn btn-primary btn-lg" onclick={onaddserver}><Icon name="plus" />{t("btnAddServer")}</button>
    </div>
  {:else}
    <div class="relative flex flex-wrap items-start justify-between gap-4">
      <div class="min-w-0">
        <div class="flex items-center gap-2">
          {#if live}
            <span class="badge bg-red-600 text-white"><span class="pulse-dot"></span>{t("statusLive")}</span>
          {:else if busy}
            <span class="badge bg-amber-400/15 text-amber-300"><span class="spinner size-3"></span>{stream.type === "Reconnecting" ? t("statusReconnecting", { attempt: stream.payload.attempt }) : t("statusConnecting")}</span>
          {:else}
            <span class="badge bg-gray-800 text-gray-300">{t("statusIdle")}</span>
          {/if}
          <span class="badge bg-gray-800 text-gray-300">
            <Icon name={store.input === "playlist" ? "music" : "mic"} size={12} />
            {store.input === "playlist" ? t("sourcePlaylist") : t("sourceMic")}
          </span>
        </div>
        <div class="mt-3 font-mono text-5xl font-semibold tracking-tight tabular-nums {store.isStreaming ? 'text-white' : 'text-gray-600'}">
          {formatDuration(stats.live_secs)}
        </div>
      </div>

      <div class="flex w-full max-w-xs flex-col gap-2">
        <label class="flex flex-col gap-1.5">
          <span class="text-xs font-medium text-gray-400">{t("profileLabel")}</span>
          <div class="flex gap-2">
            <select
              class="min-h-10 w-full min-w-0 rounded-xl border border-gray-700 bg-gray-800 px-3 text-sm text-white focus:border-lime-400 focus:outline-none disabled:opacity-60"
              value={profile?.id ?? ""}
              onchange={onselect}
              disabled={store.isStreaming}
            >
              {#if !profile}<option value="">—</option>{/if}
              {#each store.profiles as p (p.id)}
                <option value={p.id}>{p.name}</option>
              {/each}
            </select>
            <button
              class="grid size-10 flex-none cursor-pointer place-items-center rounded-xl border border-gray-700 text-gray-300 hover:bg-gray-800 hover:text-white"
              onclick={onservers}
              title={t("btnManageServers")}
              aria-label={t("btnManageServers")}
            >
              <Icon name="server" />
            </button>
          </div>
        </label>
        {#if store.isStreaming}
          <button class="btn btn-danger btn-lg w-full" onclick={() => store.stopStream()} disabled={store.pending === "stop"}>
            {#if store.pending === "stop"}<span class="spinner"></span>{:else}<Icon name="square" />{/if}
            {busy ? t("btnCancel") : t("btnStop")}
          </button>
        {:else}
          <button class="btn btn-primary btn-lg w-full" onclick={() => store.startStream()} disabled={!profile || store.pending !== null}>
            {#if store.pending === "start"}<span class="spinner"></span>{:else}<Icon name="radio" size={20} />{/if}
            {t("btnGoLive")}
          </button>
        {/if}
      </div>
    </div>

    {#if stream.type === "Connecting" && profile}
      <p class="relative mt-4 text-xs text-gray-400">{t("connectingHint", { host: profile.host })}</p>
    {:else if stream.type === "Reconnecting"}
      <div class="relative mt-4 flex gap-2 rounded-xl bg-amber-400/10 px-3.5 py-2.5 text-[13px] text-amber-200">
        <Icon name="alert" /><span>{t("reconnectingHint")}</span>
      </div>
    {/if}

    {#if error}
      <div class="relative mt-4 flex gap-2.5 rounded-xl bg-red-500/15 px-3.5 py-3 text-[13px] text-red-100" role="alert">
        <Icon name="alert" />
        <div class="grow">
          <div>{error.message}</div>
          {#if error.detail}
            <details class="detail mt-1">
              <summary>{t("errDetails")}</summary>
              <code>{error.detail}</code>
            </details>
          {/if}
          <div class="mt-2 flex gap-2">
            <button class="btn btn-sm btn-primary" onclick={() => store.startStream()} disabled={!profile || store.pending !== null}>
              <Icon name="refresh" size={14} />{t("btnRetry")}
            </button>
            <button class="btn btn-sm border-transparent bg-transparent text-red-100 shadow-none hover:!bg-red-500/20" onclick={() => (store.errorDismissed = true)}>
              {t("btnDismiss")}
            </button>
          </div>
        </div>
      </div>
    {/if}

    <dl class="relative mt-6 grid grid-cols-2 gap-3 lg:grid-cols-4">
      {#each [
        { label: t("statSent"), value: formatBytes(stats.bytes_sent), mono: true },
        { label: t("statQuality"), value: quality, mono: false },
        { label: t("statDropped"), value: String(stats.dropped_frames), mono: true, warn: stats.dropped_frames > 0 },
        { label: t("statServer"), value: profile ? `${profile.host}:${profile.mode === "DJ" && profile.dj_port ? profile.dj_port : profile.port}` : "—", mono: true },
      ] as stat}
        <div class="min-w-0 rounded-xl bg-gray-800/80 px-4 py-3">
          <dt class="text-[11px] font-medium text-gray-400">{stat.label}</dt>
          <dd
            class="mt-0.5 truncate font-semibold {stat.mono ? 'font-mono text-[15px] tabular-nums' : 'text-xs leading-[22px]'}
              {stat.warn ? 'text-amber-300' : store.isStreaming ? 'text-white' : 'text-gray-400'}"
            title={stat.value}
          >
            {stat.value}
          </dd>
        </div>
      {/each}
    </dl>
    {#if store.isStreaming && stats.dropped_frames > 0}
      <p class="relative mt-3 text-xs text-amber-200">{t("droppedHint")}</p>
    {/if}
  {/if}
</section>
