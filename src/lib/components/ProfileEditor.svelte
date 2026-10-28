<script lang="ts">
  import { untrack } from "svelte";
  import type { Profile } from "../../types";
  import { t } from "../../i18n/index.svelte";

  let { profile, onsubmit }: { profile: Profile; onsubmit: (p: Profile) => void } = $props();

  const BITRATES = [16, 24, 32, 48, 64, 96, 128, 192];
  const ALL_RATES = [16000, 22050, 44100, 48000];
  const LOW_RATES = [16000, 22050];

  // Secrets are write-only: start blank, blank means "keep the saved one".
  let p = $state<Profile>(untrack(() => ({ ...profile, password: "", dj_password: "" })));
  let djPort = $state(untrack(() => (profile.dj_port ? String(profile.dj_port) : "")));
  let touched = $state(false);

  const isDj = $derived(p.mode === "DJ");
  const isIcecast = $derived(isDj && p.dj_protocol === "ICECAST");
  const rates = $derived(p.bitrate_kbps <= 24 ? LOW_RATES : ALL_RATES);

  // 16/24 kbps MP3 is MPEG-2 only: keep the sample rate legal as the bitrate changes.
  $effect(() => {
    if (!rates.includes(p.sample_rate_hz)) {
      p.sample_rate_hz = rates.includes(22050) ? 22050 : rates[0];
    }
  });

  const hostError = $derived(touched && !p.host.trim() ? t("errHostRequired") : null);
  const portError = $derived(
    !Number.isInteger(p.port) || p.port < 1 || p.port > 65535 ? t("errPortInvalid") : null,
  );
  const djPortValue = $derived(djPort.trim() === "" ? null : Number(djPort));
  const djPortError = $derived(
    isDj && djPortValue !== null && (!Number.isInteger(djPortValue) || djPortValue < 1 || djPortValue > 65535)
      ? t("errPortInvalid")
      : null,
  );
  // New profiles need a password for the mode in use.
  const passwordMissing = $derived(
    touched &&
      (isDj
        ? !profile.has_dj_password && !profile.has_password && !(p.dj_password ?? "").trim()
        : !profile.has_password && !p.password.trim()),
  );

  export function submit() {
    touched = true;
    if (!p.host.trim() || portError || djPortError || passwordMissing) return;
    onsubmit({
      ...p,
      sid: Number(p.sid) || 1,
      dj_port: isDj ? djPortValue : null,
      dj_user: p.dj_user?.trim() || null,
      dj_mount: p.dj_mount?.trim() || null,
    });
  }

  function setMode(mode: Profile["mode"]) {
    p.mode = mode;
    if (mode === "DJ" && p.dj_protocol === "ICECAST" && !p.dj_mount) p.dj_mount = "/";
  }

  function setProtocol(e: Event) {
    p.dj_protocol = (e.currentTarget as HTMLSelectElement).value as Profile["dj_protocol"];
    if (p.dj_protocol === "ICECAST" && !p.dj_mount) p.dj_mount = "/";
  }
</script>

<form
  class="editor"
  onsubmit={(e) => {
    e.preventDefault();
    submit();
  }}
  novalidate
>
  <label class="field">
    <span>{t("fieldName")}</span>
    <input class="input" bind:value={p.name} placeholder={t("fieldNamePlaceholder")} />
  </label>

  <fieldset>
    <legend>{t("fieldType")}</legend>
    <div class="segmented" role="group" aria-label={t("fieldType")}>
      <button type="button" aria-pressed={!isDj} onclick={() => setMode("SOURCE")}>{t("typeSource")}</button>
      <button type="button" aria-pressed={isDj} onclick={() => setMode("DJ")}>{t("typeDj")}</button>
    </div>
    <p class="hint">{isDj ? t("typeDjHint") : t("typeSourceHint")}</p>
  </fieldset>

  <fieldset>
    <legend>{t("sectionConnection")}</legend>
    {#if isDj}
      <label class="field">
        <span>{t("fieldProtocol")}</span>
        <select class="select" value={p.dj_protocol} onchange={setProtocol}>
          <option value="ICECAST">{t("protocolIcecast")}</option>
          <option value="ULTRAVOX">{t("protocolUltravox")}</option>
        </select>
      </label>
    {/if}
    <div class="grid host">
      <label class="field">
        <span>{t("fieldHost")}</span>
        <input
          class="input"
          bind:value={p.host}
          placeholder={t("fieldHostPlaceholder")}
          aria-invalid={hostError ? "true" : undefined}
          autocomplete="off"
          spellcheck="false"
        />
        {#if hostError}<span class="field-error">{hostError}</span>{/if}
      </label>
      <label class="field">
        <span>{t("fieldPort")}</span>
        <input class="input" type="number" min="1" max="65535" bind:value={p.port} aria-invalid={portError ? "true" : undefined} />
      </label>
    </div>
    <p class="hint">{portError ?? t("fieldHostHint")}</p>
    {#if isDj}
      <label class="field">
        <span>{t("fieldDjPort")}</span>
        <input
          class="input"
          inputmode="numeric"
          bind:value={djPort}
          placeholder={String(p.port || "")}
          aria-invalid={djPortError ? "true" : undefined}
        />
        <span class={djPortError ? "field-error" : "hint"}>{djPortError ?? t("fieldDjPortHint")}</span>
      </label>
    {/if}
  </fieldset>

  <fieldset>
    <legend>{t("sectionLogin")}</legend>
    {#if !isIcecast}
      <label class="field">
        <span>{t("fieldSid")}</span>
        <input class="input" type="number" min="1" bind:value={p.sid} />
      </label>
    {/if}
    {#if isDj}
      <div class="grid two">
        <label class="field">
          <span>{t("fieldDjUser")}</span>
          <input class="input" bind:value={p.dj_user} autocomplete="off" spellcheck="false" />
        </label>
        <label class="field">
          <span>{t("fieldDjPassword")}</span>
          <input
            class="input"
            type="password"
            bind:value={p.dj_password}
            autocomplete="new-password"
            placeholder={profile.has_dj_password ? t("passwordKeepPlaceholder") : t("passwordNewPlaceholder")}
            aria-invalid={passwordMissing ? "true" : undefined}
          />
        </label>
      </div>
      {#if isIcecast}
        <label class="field">
          <span>{t("fieldMount")}</span>
          <input class="input" bind:value={p.dj_mount} placeholder="/" spellcheck="false" />
          <span class="hint">{t("fieldMountHint")}</span>
        </label>
      {/if}
    {:else}
      <label class="field">
        <span>{t("fieldPassword")}</span>
        <input
          class="input"
          type="password"
          bind:value={p.password}
          autocomplete="new-password"
          placeholder={profile.has_password ? t("passwordKeepPlaceholder") : t("passwordNewPlaceholder")}
          aria-invalid={passwordMissing ? "true" : undefined}
        />
      </label>
    {/if}
    {#if passwordMissing}<span class="field-error">{t("errPasswordMissing")}</span>{/if}
  </fieldset>

  <fieldset>
    <legend>{t("sectionAudio")}</legend>
    <div class="grid three">
      <label class="field">
        <span>{t("fieldBitrate")}</span>
        <select class="select" bind:value={p.bitrate_kbps}>
          {#each BITRATES as b}
            <option value={b}>{b} kbps</option>
          {/each}
        </select>
      </label>
      <label class="field">
        <span>{t("fieldSampleRate")}</span>
        <select class="select" bind:value={p.sample_rate_hz}>
          {#each rates as r}
            <option value={r}>{(r / 1000).toLocaleString(undefined, { maximumFractionDigits: 2 })} kHz</option>
          {/each}
        </select>
      </label>
      <label class="field">
        <span>{t("fieldChannels")}</span>
        <select class="select" bind:value={p.mono}>
          <option value={true}>{t("mono")}</option>
          <option value={false}>{t("stereo")}</option>
        </select>
      </label>
    </div>
    <p class="hint">{p.bitrate_kbps <= 24 ? t("lowBitrateHint") : t("speechHint")}</p>
  </fieldset>

  <details class="station">
    <summary>{t("sectionStation")}</summary>
    <p class="hint">{t("sectionStationHint")}</p>
    <div class="grid two">
      <label class="field">
        <span>{t("fieldStreamName")}</span>
        <input class="input" bind:value={p.stream_name} placeholder={t("fieldStreamNamePlaceholder")} />
      </label>
      <label class="field">
        <span>{t("fieldGenre")}</span>
        <input class="input" bind:value={p.genre} placeholder={t("fieldGenrePlaceholder")} />
      </label>
    </div>
    <label class="field">
      <span>{t("fieldUrl")}</span>
      <input class="input" type="url" bind:value={p.station_url} placeholder="https://" spellcheck="false" />
    </label>
    <label class="check">
      <input type="checkbox" bind:checked={p.is_public} />
      <span>{t("fieldPublic")}</span>
    </label>
  </details>

  <!-- Enter in any field submits. -->
  <button type="submit" hidden aria-hidden="true" tabindex="-1"></button>
</form>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  fieldset {
    border: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }
  legend,
  summary {
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
    margin-bottom: 8px;
  }
  summary {
    cursor: pointer;
    margin-bottom: 0;
  }
  .station {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .station[open] > :not(summary) {
    margin-top: 10px;
  }
  .grid {
    display: grid;
    gap: 10px;
  }
  .host {
    grid-template-columns: minmax(0, 1fr) 110px;
  }
  .two {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .three {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
  @media (max-width: 460px) {
    .two,
    .three {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
