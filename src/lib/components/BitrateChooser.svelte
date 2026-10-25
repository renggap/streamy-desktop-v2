<script lang="ts">
  import { t } from "../../i18n";
  import type { DjProtocol } from "../../types";

  interface Props {
    protocol: DjProtocol;
    bitrate: number;
    samplerate: number;
    channels: number;
    onChange: (config: { bitrate: number; samplerate: number; channels: number }) => void;
  }

  let { protocol, bitrate, samplerate, channels, onChange }: Props = $props();

  const MP3_BITRATES = [16, 24, 32, 48, 64, 96, 128, 192];
  const MP3_RATES = [16000, 22050, 24000, 32000, 44100, 48000];

  function handleBitrate(newBitrate: number) {
    let newRate = samplerate;
    // MPEG-2 clamp rule: 16k and 24k MP3 require sample rate <= 24000 Hz
    if (newBitrate <= 24 && newRate > 24000) {
      newRate = 24000;
    }
    onChange({ bitrate: newBitrate, samplerate: newRate, channels });
  }

  function handleSampleRate(newRate: number) {
    // If rate > 24000 and bitrate <= 24, bump bitrate to 32
    let newBitrate = bitrate;
    if (newRate > 24000 && newBitrate <= 24) {
      newBitrate = 32;
    }
    onChange({ bitrate: newBitrate, samplerate: newRate, channels });
  }

  function handleChannels(newChannels: number) {
    onChange({ bitrate, samplerate, channels: newChannels });
  }
</script>

<div class="bitrate-chooser-grid">
  <div class="field-col">
    <label for="bitrate-select" class="chooser-label">{t("fieldBitrate")}</label>
    <select
      id="bitrate-select"
      class="chooser-select"
      value={bitrate}
      onchange={(e) => handleBitrate(parseInt(e.currentTarget.value, 10))}
    >
      {#each MP3_BITRATES as b}
        <option value={b}>{b} kbps {b <= 24 ? "(MPEG-2)" : ""}</option>
      {/each}
    </select>
  </div>

  <div class="field-col">
    <label for="rate-select" class="chooser-label">{t("fieldSampleRate")}</label>
    <select
      id="rate-select"
      class="chooser-select"
      value={samplerate}
      onchange={(e) => handleSampleRate(parseInt(e.currentTarget.value, 10))}
    >
      {#each MP3_RATES as r}
        <option value={r}>{r} Hz {r <= 24000 ? "(MPEG-2)" : "(MPEG-1)"}</option>
      {/each}
    </select>
  </div>

  <div class="field-col">
    <label for="channels-select" class="chooser-label">{t("fieldChannels")}</label>
    <select
      id="channels-select"
      class="chooser-select"
      value={channels}
      onchange={(e) => handleChannels(parseInt(e.currentTarget.value, 10))}
    >
      <option value={2}>{t("channelsStereo")}</option>
      <option value={1}>{t("channelsMono")}</option>
    </select>
  </div>
</div>

<style>
  .bitrate-chooser-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 10px;
  }

  .field-col {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .chooser-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
  }

  .chooser-select {
    padding: 8px 10px;
    border: 1px solid var(--color-border);
    border-radius: 6px;
    background-color: var(--color-page);
    font-size: 13px;
    color: var(--color-text-main);
    outline: none;
    cursor: pointer;
  }

  .chooser-select:focus {
    border-color: var(--color-primary);
  }
</style>
