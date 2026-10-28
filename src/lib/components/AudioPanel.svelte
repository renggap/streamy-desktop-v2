<script lang="ts">
  import Icon from "./Icon.svelte";
  import Meter from "./Meter.svelte";
  import { store } from "../store.svelte";
  import { dbToGain, formatDb, gainToDb } from "../format";
  import { t } from "../../i18n/index.svelte";
  import type { NoiseSuppression } from "../../types";

  const NOISE_LEVELS: Array<{ id: NoiseSuppression; key: "noiseOff" | "noiseLow" | "noiseMedium" | "noiseHigh" }> = [
    { id: "off", key: "noiseOff" },
    { id: "low", key: "noiseLow" },
    { id: "medium", key: "noiseMedium" },
    { id: "high", key: "noiseHigh" },
  ];

  const MIN_DB = -30;
  const MAX_DB = 12;

  const defaultDevice = $derived(store.devices.find((d) => d.is_default)?.name ?? null);
  const selected = $derived(store.settings.input_device_name ?? "");
  // A saved device that is unplugged right now still shows, so the choice is visible.
  const missing = $derived(selected !== "" && !store.devices.some((d) => d.name === selected));
  const deviceLocked = $derived(store.isStreaming || store.isRecording);

  const gain = $derived(store.settings.mic_gain);
  const muted = $derived(gain <= 0);
  const sliderDb = $derived(muted ? MIN_DB : Math.max(MIN_DB, Math.min(MAX_DB, gainToDb(gain))));
  let lastAudibleGain = 1;

  function ondevice(e: Event) {
    const v = (e.currentTarget as HTMLSelectElement).value;
    void store.saveSettings({ input_device_name: v || null }, true);
  }

  function onslide(e: Event) {
    const db = Number((e.currentTarget as HTMLInputElement).value);
    // Snap to unity near 0 dB: the most common target.
    const g = Math.abs(db) < 0.25 ? 1 : dbToGain(db);
    lastAudibleGain = g;
    store.setGain(g);
  }

  function toggleMute() {
    if (muted) {
      store.setGain(lastAudibleGain > 0 ? lastAudibleGain : 1);
    } else {
      lastAudibleGain = gain;
      store.setGain(0);
    }
  }
</script>

<section class="card">
  <div class="card-head">
    <div>
      <h2 class="card-title">{t("audioTitle")}</h2>
      <p class="card-sub">{store.input === "mic" ? t("micOnAirHint") : t("micOffAirHint")}</p>
    </div>
    <button
      class="btn btn-sm"
      class:btn-dark={store.status.mic_test}
      onclick={() => store.toggleMicTest()}
      disabled={store.isStreaming || store.isRecording || store.pending !== null}
    >
      <Icon name="mic" size={14} />
      {store.status.mic_test ? t("btnMicTestStop") : t("btnMicTest")}
    </button>
  </div>

  <div class="row">
    <label class="field grow">
      <span>{t("deviceLabel")}</span>
      <select class="select" value={selected} onchange={ondevice} disabled={deviceLocked}>
        <option value="">{defaultDevice ? t("deviceDefaultNamed", { name: defaultDevice }) : t("deviceDefault")}</option>
        {#if missing}
          <option value={selected}>{selected} (?)</option>
        {/if}
        {#each store.devices as d (d.name)}
          <option value={d.name}>{d.name}</option>
        {/each}
      </select>
    </label>
    <button class="btn btn-icon refresh" onclick={() => store.loadDevices()} aria-label={t("deviceRefresh")} title={t("deviceRefresh")}>
      <Icon name="refresh" />
    </button>
  </div>

  <div>
    <Meter rms={store.level.rms} peak={store.level.peak} active={store.meterActive} />
    <p class="hint">{store.status.mic_test ? t("micTestHint") : store.meterActive ? "" : t("meterIdleHint")}</p>
  </div>

  <div class="gain">
    <span class="label">{t("gainLabel")}</span>
    <input
      type="range"
      min={MIN_DB}
      max={MAX_DB}
      step="0.5"
      value={sliderDb}
      oninput={onslide}
      disabled={muted}
      aria-label={t("gainLabel")}
      aria-valuetext={muted ? t("muted") : formatDb(gainToDb(gain))}
    />
    <span class="value mono">{muted ? t("muted") : formatDb(gainToDb(gain))}</span>
    <button
      class="btn btn-sm"
      class:btn-danger={muted}
      onclick={toggleMute}
      aria-pressed={muted}
      title={muted ? t("btnUnmute") : t("btnMute")}
    >
      <Icon name={muted ? "mic-off" : "mic"} size={14} />
      {muted ? t("btnUnmute") : t("btnMute")}
    </button>
  </div>

  <div class="field">
    <span>{t("noiseLabel")}</span>
    <div class="segmented" role="group" aria-label={t("noiseLabel")}>
      {#each NOISE_LEVELS as n (n.id)}
        <button
          type="button"
          aria-pressed={store.settings.noise_suppression === n.id}
          onclick={() => store.saveSettings({ noise_suppression: n.id }, true)}
        >{t(n.key)}</button>
      {/each}
    </div>
    <p class="hint">{t("noiseHint")}</p>
  </div>
</section>

<style>
  .refresh {
    align-self: flex-end;
  }
  .hint:empty {
    display: none;
  }
  .hint {
    margin-top: 4px;
  }
  .gain {
    display: grid;
    grid-template-columns: auto 1fr auto auto;
    align-items: center;
    gap: 10px;
  }
  .label {
    font-size: 12px;
    font-weight: 500;
    color: var(--color-gray-700);
  }
  .value {
    min-width: 68px;
    text-align: right;
    font-size: 12px;
  }
</style>
