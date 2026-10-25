<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { t } from "../../i18n";
  import { api } from "../api";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  let rmsDb = $state(-90);
  let peakDb = $state(-90);
  let isClipping = $state(false);
  let clipTimeout: any = null;

  let displayRms = $state(-90);
  let displayPeak = $state(-90);

  let unlisten: UnlistenFn | null = null;
  let animId: number | null = null;

  // Convert dBFS (-90 to 0) to percentage (0% to 100%)
  function dbToPct(db: number): number {
    if (db <= -90) return 0;
    if (db >= 0) return 100;
    return ((db + 90) / 90) * 100;
  }

  onMount(async () => {
    try {
      unlisten = await api.onLevelTap((data) => {
        rmsDb = data.rms_dbfs;
        peakDb = data.peak_dbfs;
        if (data.clipping) {
          isClipping = true;
          if (clipTimeout) clearTimeout(clipTimeout);
          clipTimeout = setTimeout(() => {
            isClipping = false;
          }, 800);
        }
      });
    } catch (e) {
      console.warn("Level tap listen not supported or mocked:", e);
    }

    // Ballistics loop for smooth meter action
    const loop = () => {
      // Fast attack, smooth decay
      if (rmsDb > displayRms) {
        displayRms += (rmsDb - displayRms) * 0.4;
      } else {
        displayRms += (rmsDb - displayRms) * 0.12;
      }

      if (peakDb > displayPeak) {
        displayPeak = peakDb;
      } else {
        displayPeak -= 0.5; // slow peak drop
      }

      animId = requestAnimationFrame(loop);
    };
    animId = requestAnimationFrame(loop);
  });

  onDestroy(() => {
    if (unlisten) unlisten();
    if (animId) cancelAnimationFrame(animId);
    if (clipTimeout) clearTimeout(clipTimeout);
  });

  const rmsPct = $derived(dbToPct(displayRms));
  const peakPct = $derived(dbToPct(displayPeak));
</script>

<div class="card vu-card">
  <div class="vu-header">
    <div class="vu-title-row">
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M12 2v20M17 5v14M7 8v8M2 11v2M22 11v2"/>
      </svg>
      <span class="vu-title">{t("audioInputLevels")}</span>
    </div>
    <div class="vu-status-tags">
      {#if isClipping}
        <span class="clip-tag">{t("clippingWarn")}</span>
      {/if}
      <span class="db-readout font-mono">
        {displayRms > -85 ? `${displayRms.toFixed(1)} dB` : "-inf"}
      </span>
    </div>
  </div>

  <div class="meter-container">
    <div class="meter-bar-track">
      <!-- Fill gradient bar -->
      <div
        class="meter-bar-fill"
        style="width: {rmsPct}%;"
      ></div>
      <!-- Peak hold tick mark -->
      {#if peakPct > 0}
        <div
          class="meter-peak-tick"
          style="left: calc({peakPct}% - 2px);"
        ></div>
      {/if}
    </div>

    <!-- Scale ticks -->
    <div class="scale-ticks">
      <span>-60</span>
      <span>-40</span>
      <span>-20</span>
      <span>-12</span>
      <span>-6</span>
      <span>-3</span>
      <span>0 dB</span>
    </div>
  </div>
</div>

<style>
  .vu-card {
    gap: 10px;
  }

  .vu-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .vu-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 700;
    color: var(--color-text-main);
  }

  .vu-status-tags {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .clip-tag {
    font-size: 11px;
    font-weight: 800;
    color: white;
    background-color: var(--color-danger);
    padding: 1px 6px;
    border-radius: 4px;
    letter-spacing: 0.05em;
    animation: flash 0.3s infinite alternate;
  }

  @keyframes flash {
    from { opacity: 0.7; }
    to { opacity: 1; }
  }

  .db-readout {
    font-size: 12px;
    font-weight: 600;
    color: var(--color-text-muted);
  }

  .meter-container {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .meter-bar-track {
    position: relative;
    height: 16px;
    background-color: #0f172a;
    border-radius: 4px;
    overflow: hidden;
    box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.4);
  }

  .meter-bar-fill {
    height: 100%;
    background: linear-gradient(
      to right,
      #16a34a 0%,
      #16a34a 70%,
      #eab308 85%,
      #dc2626 95%,
      #ef4444 100%
    );
    border-radius: 3px;
    transition: width 0.05s ease-out;
  }

  .meter-peak-tick {
    position: absolute;
    top: 0;
    width: 3px;
    height: 100%;
    background-color: #ffffff;
    box-shadow: 0 0 4px #ffffff;
  }

  .scale-ticks {
    display: flex;
    justify-content: space-between;
    font-size: 10px;
    font-family: var(--font-mono);
    color: var(--color-text-dim);
    padding: 0 2px;
  }

  .font-mono {
    font-family: var(--font-mono);
  }
</style>
