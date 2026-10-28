<script lang="ts">
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { friendlyError } from "../errors";
  import { formatBytes, formatDuration } from "../format";
  import { t } from "../../i18n/index.svelte";

  let { onmanage, onadd }: { onmanage: () => void; onadd: () => void } = $props();

  const stream = $derived(store.status.stream);
  const stats = $derived(store.status.stats);
  const profile = $derived(store.selectedProfile);
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

<section class="card onair" class:is-live={stream.type === "Live"}>
  {#if store.profiles.length === 0}
    <div class="empty">
      <Icon name="server" size={28} />
      <p>{t("needProfileHint")}</p>
      <button class="btn btn-primary" onclick={onadd}><Icon name="plus" />{t("btnAddServer")}</button>
    </div>
  {:else}
    <div class="row">
      <label class="field grow">
        <span>{t("profileLabel")}</span>
        <select class="select" value={profile?.id ?? ""} onchange={onselect} disabled={store.isStreaming}>
          {#if !profile}
            <option value="">—</option>
          {/if}
          {#each store.profiles as p (p.id)}
            <option value={p.id}>{p.name} · {p.host}:{p.mode === "DJ" && p.dj_port ? p.dj_port : p.port}</option>
          {/each}
        </select>
      </label>
      <button class="btn manage" onclick={onmanage} title={t("btnManageServers")}>
        <Icon name="server" /><span class="label">{t("btnManageServers")}</span>
      </button>
    </div>

    {#if store.isStreaming}
      <button class="btn btn-danger btn-lg" onclick={() => store.stopStream()} disabled={store.pending === "stop"}>
        {#if store.pending === "stop"}<span class="spinner"></span>{:else}<Icon name="square" />{/if}
        {busy ? t("btnCancel") : t("btnStop")}
      </button>
    {:else}
      <button
        class="btn btn-primary btn-lg"
        onclick={() => store.startStream()}
        disabled={!profile || store.pending !== null}
      >
        {#if store.pending === "start"}<span class="spinner"></span>{:else}<Icon name="radio" size={20} />{/if}
        {t("btnGoLive")}
      </button>
    {/if}

    {#if stream.type === "Connecting" && profile}
      <p class="hint row"><span class="spinner"></span>{t("connectingHint", { host: profile.host })}</p>
    {:else if stream.type === "Reconnecting"}
      <div class="banner banner-warn"><Icon name="alert" /><span>{t("reconnectingHint")}</span></div>
    {/if}

    {#if error}
      <div class="banner banner-error" role="alert">
        <Icon name="alert" />
        <div class="grow">
          <div>{error.message}</div>
          {#if error.detail}
            <details class="detail">
              <summary>{t("errDetails")}</summary>
              <code>{error.detail}</code>
            </details>
          {/if}
          <div class="row actions">
            <button class="btn btn-sm" onclick={() => store.startStream()} disabled={!profile || store.pending !== null}>
              <Icon name="refresh" size={14} />{t("btnRetry")}
            </button>
            <button class="btn btn-sm btn-ghost" onclick={() => (store.errorDismissed = true)}>{t("btnDismiss")}</button>
          </div>
        </div>
      </div>
    {/if}

    <dl class="stats" class:dim={!store.isStreaming}>
      <div>
        <dt>{t("statOnAir")}</dt>
        <dd class="mono">{formatDuration(stats.live_secs)}</dd>
      </div>
      <div>
        <dt>{t("statSent")}</dt>
        <dd class="mono">{formatBytes(stats.bytes_sent)}</dd>
      </div>
      <div>
        <dt>{t("statQuality")}</dt>
        <dd>{quality}</dd>
      </div>
      <div>
        <dt>{t("statDropped")}</dt>
        <dd class="mono" class:warn={stats.dropped_frames > 0}>{stats.dropped_frames}</dd>
      </div>
    </dl>
    {#if store.isStreaming && stats.dropped_frames > 0}
      <p class="hint">{t("droppedHint")}</p>
    {/if}
  {/if}
</section>

<style>
  .onair.is-live {
    border-color: var(--live);
    box-shadow: 0 0 0 1px var(--live), var(--shadow);
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 10px;
    padding: 18px 8px;
    color: var(--text-muted);
  }
  .manage {
    align-self: flex-end;
  }
  @media (max-width: 520px) {
    .manage .label {
      display: none;
    }
  }
  .actions {
    margin-top: 8px;
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 8px;
  }
  @media (max-width: 520px) {
    .stats {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
  .stats > div {
    background: var(--card-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 8px 10px;
    min-width: 0;
  }
  .stats.dim dd {
    color: var(--text-dim);
  }
  dt {
    font-size: 11px;
    color: var(--text-muted);
    font-weight: 600;
  }
  dd {
    font-size: 15px;
    font-weight: 700;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  dd:not(.mono) {
    font-size: 12px;
    line-height: 22px;
  }
  dd.warn {
    color: var(--warn);
  }
</style>
