<script lang="ts">
  import { untrack } from "svelte";
  import { t } from "../../i18n/index.svelte";

  let { rms, peak, active }: { rms: number; peak: number; active: boolean } = $props();

  // Speech lives between about -40 and -6 dBFS: a -60..0 window keeps it readable.
  const FLOOR = -60;
  const TICKS = [-60, -48, -36, -24, -18, -12, -6, -3, 0];
  const pct = (db: number) => Math.max(0, Math.min(100, ((db - FLOOR) / -FLOOR) * 100));

  // Peak hold: freeze the highest peak for a moment, then fall.
  let held = $state(FLOOR);
  let heldAt = 0;
  let clipUntil = $state(0);
  let now = $state(Date.now());

  $effect(() => {
    const p = peak;
    const ts = Date.now();
    untrack(() => {
      if (p >= held || ts - heldAt > 1200) {
        held = p;
        heldAt = ts;
      }
      if (p > -0.5) clipUntil = ts + 1500;
    });
  });

  $effect(() => {
    const id = setInterval(() => (now = Date.now()), 250);
    return () => clearInterval(id);
  });

  const clipping = $derived(active && clipUntil > now);
</script>

<div class="meter" class:inactive={!active} role="meter" aria-valuemin={FLOOR} aria-valuemax={0} aria-valuenow={Math.round(rms)} aria-label="Input level">
  <div class="track">
    <div class="fill" style="clip-path: inset(0 {100 - pct(rms)}% 0 0)"></div>
    <div class="peak" style="left: calc({pct(peak)}% - 1px)"></div>
    <div class="hold" style="left: calc({pct(held)}% - 1px)"></div>
  </div>
  <span class="clip" class:on={clipping}>{t("clip")}</span>
</div>
<div class="scale" aria-hidden="true">
  {#each TICKS as tick}
    <span style="left: {pct(tick)}%">{tick}</span>
  {/each}
</div>

<style>
  .meter {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .track {
    position: relative;
    flex: 1;
    height: 14px;
    border-radius: 999px;
    background: var(--meter-bg);
    overflow: hidden;
  }
  /* Gradient stops match the tick positions: -18 dB = 70%, -6 dB = 90%. */
  .fill {
    position: absolute;
    inset: 0;
    background: linear-gradient(
      90deg,
      var(--meter-green) 0%,
      var(--meter-green) 70%,
      var(--meter-yellow) 70%,
      var(--meter-yellow) 90%,
      var(--meter-red) 90%
    );
    transition: clip-path 60ms linear;
  }
  .peak,
  .hold {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    background: #fff;
    opacity: 0.55;
  }
  .hold {
    opacity: 0.9;
    background: var(--meter-yellow);
  }
  .inactive .fill,
  .inactive .peak,
  .inactive .hold {
    opacity: 0;
  }
  .clip {
    font-size: 10px;
    font-weight: 800;
    padding: 2px 5px;
    border-radius: 6px;
    color: var(--color-gray-400);
    border: 1px solid var(--color-gray-200);
    width: 38px;
    text-align: center;
  }
  .clip.on {
    background: var(--meter-red);
    border-color: var(--meter-red);
    color: #fff;
  }
  .scale {
    position: relative;
    height: 14px;
    margin-right: 46px;
    font-size: 10px;
    color: var(--color-gray-400);
    font-family: var(--font-mono);
  }
  .scale span {
    position: absolute;
    transform: translateX(-50%);
  }
  .scale span:first-child {
    transform: none;
  }
  .scale span:last-child {
    transform: translateX(-100%);
  }
</style>
