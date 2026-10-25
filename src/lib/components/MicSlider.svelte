<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "../../i18n";
  import { api } from "../api";
  import type { AudioDeviceDesc } from "../../types";

  interface Props {
    initialGain: number;
    selectedDevice?: string | null;
    onDeviceChange: (device: string) => void;
  }

  let { initialGain = 1.0, selectedDevice = null, onDeviceChange }: Props = $props();

  let gain = $state(1.0);
  let prevGain = $state(1.0);
  let isMuted = $state(false);
  let devices = $state<AudioDeviceDesc[]>([]);

  onMount(async () => {
    gain = initialGain;
    prevGain = initialGain > 0 ? initialGain : 1.0;
    isMuted = initialGain === 0;

    try {
      devices = await api.listInputDevices();
    } catch (e) {
      console.warn("Could not list input devices:", e);
    }
  });

  async function handleGainChange(newGain: number) {
    gain = newGain;
    isMuted = newGain === 0;
    try {
      await api.setMicGain(newGain);
    } catch (e) {
      console.error(e);
    }
  }

  async function toggleMute() {
    if (isMuted) {
      const restore = prevGain > 0 ? prevGain : 1.0;
      await handleGainChange(restore);
    } else {
      prevGain = gain;
      await handleGainChange(0);
    }
  }
</script>

<div class="card mic-card">
  <div class="mic-header">
    <div class="mic-title-row">
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"/>
        <path d="M19 10v2a7 7 0 0 1-14 0v-2"/>
        <line x1="12" y1="19" x2="12" y2="23"/>
        <line x1="8" y1="23" x2="16" y2="23"/>
      </svg>
      <span class="mic-title">{t("micControlTitle")}</span>
    </div>

    <!-- Device select dropdown -->
    {#if devices.length > 0}
      <select
        class="device-select"
        value={selectedDevice || ""}
        onchange={(e) => onDeviceChange(e.currentTarget.value)}
      >
        <option value="">Default Device</option>
        {#each devices as d}
          <option value={d.name}>{d.name} {d.is_default ? "(Default)" : ""}</option>
        {/each}
      </select>
    {/if}
  </div>

  <div class="slider-row">
    <button
      class="btn-mute {isMuted ? 'muted' : ''}"
      onclick={toggleMute}
      title={isMuted ? "Unmute" : "Mute"}
    >
      {#if isMuted}
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="1" y1="1" x2="23" y2="23"/>
          <path d="M9 9v3a3 3 0 0 0 5.12 2.12M15 9.34V4a3 3 0 0 0-5.94-.6"/>
          <path d="M17 16.95A7 7 0 0 1 5 12v-2m14 0v2a7 7 0 0 1-.11 1.23"/>
          <line x1="12" y1="19" x2="12" y2="23"/>
          <line x1="8" y1="23" x2="16" y2="23"/>
        </svg>
      {:else}
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"/>
          <path d="M19 10v2a7 7 0 0 1-14 0v-2"/>
          <line x1="12" y1="19" x2="12" y2="23"/>
          <line x1="8" y1="23" x2="16" y2="23"/>
        </svg>
      {/if}
    </button>

    <div class="range-wrapper">
      <input
        type="range"
        min="0"
        max="2"
        step="0.05"
        class="gain-slider"
        value={gain}
        oninput={(e) => handleGainChange(parseFloat(e.currentTarget.value))}
      />
      <div class="gain-marks">
        <span>0%</span>
        <span>100%</span>
        <span>200%</span>
      </div>
    </div>

    <span class="gain-readout font-mono">
      {isMuted ? t("micMuted") : `${Math.round(gain * 100)}%`}
    </span>
  </div>
</div>

<style>
  .mic-card {
    gap: 12px;
  }

  .mic-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
  }

  .mic-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 700;
    color: var(--color-text-main);
  }

  .device-select {
    padding: 4px 8px;
    font-size: 12px;
    border-radius: 6px;
    border: 1px solid var(--color-border);
    background-color: var(--color-page);
    color: var(--color-text-main);
    max-width: 220px;
    text-overflow: ellipsis;
    outline: none;
  }

  .slider-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .btn-mute {
    width: 36px;
    height: 36px;
    border-radius: 8px;
    background-color: var(--color-page);
    border: 1px solid var(--color-border);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--color-text-muted);
    transition: all 0.15s ease;
  }

  .btn-mute:hover {
    background-color: var(--color-muted-bg);
    color: var(--color-text-main);
  }

  .btn-mute.muted {
    background-color: var(--color-danger-soft);
    color: var(--color-danger);
    border-color: var(--color-danger);
  }

  .range-wrapper {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .gain-slider {
    width: 100%;
    accent-color: var(--color-primary);
    cursor: pointer;
  }

  .gain-marks {
    display: flex;
    justify-content: space-between;
    font-size: 10px;
    color: var(--color-text-dim);
    font-family: var(--font-mono);
  }

  .gain-readout {
    min-width: 50px;
    text-align: right;
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text-main);
  }

  .font-mono {
    font-family: var(--font-mono);
  }
</style>
