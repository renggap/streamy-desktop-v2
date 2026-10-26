<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "../../i18n";
  import type { Profile, DjProtocol, BroadcastMode } from "../../types";
  import BitrateChooser from "./BitrateChooser.svelte";

  interface Props {
    profile: Profile | null;
    onSave: (saved: Profile) => Promise<void>;
    onCancel: () => void;
  }

  let { profile, onSave, onCancel }: Props = $props();

  let name = $state("");
  let server = $state("");
  let port = $state(8000);
  let sid = $state(1);
  let username = $state("");
  let mount = $state("/live");
  let password = $state("");
  let protocol = $state<DjProtocol>("SHOUTCAST");
  let mode = $state<BroadcastMode>("DJ");
  let bitrate = $state(128);
  let samplerate = $state(44100);
  let channels = $state(2);

  // Metadata
  let streamName = $state("");
  let streamGenre = $state("");
  let streamDesc = $state("");
  let streamUrl = $state("");
  let streamAim = $state("");
  let streamIcq = $state("");
  let streamIrc = $state("");

  let isSubmitting = $state(false);
  let errorMessage = $state<string | null>(null);

  const isEditing = $derived(profile !== null && profile.id !== undefined && profile.id !== 0);

  onMount(() => {
    if (profile) {
      name = profile.name || "";
      server = profile.host || profile.server || "";
      port = profile.port || 8000;
      sid = profile.sid || 1;
      username = profile.dj_user || profile.username || "";
      mount = profile.mount || profile.dj_mount || "/live";
      password = profile.password || "";
      protocol = (profile.dj_protocol || profile.protocol || "SHOUTCAST") as DjProtocol;
      mode = (profile.mode || "DJ") as BroadcastMode;
      bitrate = profile.bitrate_kbps || profile.bitrate || 128;
      samplerate = profile.sample_rate_hz || profile.samplerate || 44100;
      channels = profile.mono ? 1 : (profile.channels || 2);
      streamName = profile.stream_name || "";
      streamGenre = profile.stream_genre || "";
      streamDesc = profile.stream_desc || "";
      streamUrl = profile.stream_url || "";
      streamAim = profile.stream_aim || "";
      streamIcq = profile.stream_icq || "";
      streamIrc = profile.stream_irc || "";
    }
  });

  async function handleSubmit() {
    if (!name.trim()) {
      errorMessage = "Profile name is required";
      return;
    }
    if (!server.trim()) {
      errorMessage = "Server address is required";
      return;
    }
    if (port <= 0 || port > 65535) {
      errorMessage = "Valid port (1-65535) is required";
      return;
    }

    isSubmitting = true;
    errorMessage = null;

    const payload: Profile = {
      id: profile?.id ?? 0,
      name: name.trim(),
      host: server.trim(),
      server: server.trim(),
      port: Number(port),
      sid: Number(sid) || 1,
      mount: mount.trim() || "/live",
      dj_mount: mount.trim() || "/live",
      mode,
      codec: "MP3",
      bitrate_kbps: bitrate,
      bitrate,
      sample_rate_hz: samplerate,
      samplerate,
      mono: channels === 1,
      channels,
      password: password || undefined,
      dj_user: username.trim() || undefined,
      username: username.trim() || undefined,
      dj_protocol: protocol,
      protocol,
      stream_name: streamName.trim() || undefined,
      stream_genre: streamGenre.trim() || undefined,
      stream_desc: streamDesc.trim() || undefined,
      stream_url: streamUrl.trim() || undefined,
    };

    try {
      await onSave(payload);
    } catch (e) {
      console.error(e);
      errorMessage = String(e);
    } finally {
      isSubmitting = false;
    }
  }
</script>

<div
  class="modal-backdrop"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={(e) => { if (e.target === e.currentTarget) onCancel(); }}
  onkeydown={(e) => { if (e.key === "Escape") onCancel(); }}
>
  <div class="modal-window card">
    <div class="modal-header">
      <h2 class="modal-title">{isEditing ? t("editProfileTitle") : t("newProfileTitle")}</h2>
      <button class="btn-close" onclick={onCancel}>&times;</button>
    </div>

    {#if errorMessage}
      <div class="modal-error">
        <span>{errorMessage}</span>
      </div>
    {/if}

    <form class="modal-body" onsubmit={(e) => { e.preventDefault(); handleSubmit(); }}>
      <!-- Basic Config -->
      <div class="form-section">
        <div class="form-row">
          <div class="form-group flex-2">
            <label for="prof-name">{t("fieldName")} *</label>
            <input id="prof-name" type="text" class="input-text" required bind:value={name} />
          </div>
          <div class="form-group flex-1">
            <label for="prof-proto">{t("fieldProtocol")}</label>
            <select id="prof-proto" class="input-select" bind:value={protocol}>
              <option value="SHOUTCAST">{t("protocolShoutcast")}</option>
              <option value="ICECAST">{t("protocolIcecast")}</option>
            </select>
          </div>
          <div class="form-group flex-1">
            <label for="prof-mode">{t("fieldMode")}</label>
            <select id="prof-mode" class="input-select" bind:value={mode}>
              <option value="DJ">{t("modeDj")}</option>
              <option value="AUTOMATION">{t("modeAutomation")}</option>
            </select>
          </div>
        </div>

        <div class="form-row">
          <div class="form-group flex-2">
            <label for="prof-server">{t("fieldServer")} *</label>
            <input id="prof-server" type="text" class="input-text" placeholder="e.g. stream.example.com" required bind:value={server} />
          </div>
          <div class="form-group flex-1">
            <label for="prof-port">{t("fieldPort")} *</label>
            <input id="prof-port" type="number" min="1" max="65535" class="input-text" required bind:value={port} />
          </div>
          {#if protocol === "SHOUTCAST"}
            <div class="form-group flex-1">
              <label for="prof-sid">{t("fieldSid")}</label>
              <input id="prof-sid" type="number" min="1" class="input-text" placeholder="1" bind:value={sid} />
            </div>
          {:else}
            <div class="form-group flex-1">
              <label for="prof-mount">{t("fieldMount")}</label>
              <input id="prof-mount" type="text" class="input-text" placeholder="/live" bind:value={mount} />
            </div>
          {/if}
        </div>

        <div class="form-row">
          <div class="form-group flex-1">
            <label for="prof-username">{t("fieldUsername")}</label>
            <input id="prof-username" type="text" class="input-text" placeholder="e.g. source / djlogin" bind:value={username} />
          </div>
          <div class="form-group flex-1">
            <label for="prof-password">{t("fieldPassword")}</label>
            <input id="prof-password" type="password" class="input-text" placeholder={isEditing ? "(Unchanged if blank)" : "Server password"} bind:value={password} />
          </div>
        </div>
      </div>

      <!-- Audio Encoding -->
      <div class="form-section">
        <BitrateChooser
          {protocol}
          {bitrate}
          {samplerate}
          {channels}
          onChange={(cfg) => {
            bitrate = cfg.bitrate;
            samplerate = cfg.samplerate;
            channels = cfg.channels;
          }}
        />
      </div>

      <!-- Station Metadata (Islamic Broadcast Theme) -->
      <div class="form-section">
        <div class="form-row">
          <div class="form-group flex-1">
            <label for="prof-sname">{t("fieldStreamName")}</label>
            <input id="prof-sname" type="text" class="input-text" placeholder="cth. Radio Dakwah Islamiah / Suara Al-Qur'an" bind:value={streamName} />
          </div>
          <div class="form-group flex-1">
            <label for="prof-genre">{t("fieldStreamGenre")}</label>
            <input id="prof-genre" type="text" class="input-text" placeholder="Kajian Islam / Murottal / Ceramah" bind:value={streamGenre} />
          </div>
        </div>

        <div class="form-row">
          <div class="form-group flex-2">
            <label for="prof-desc">{t("fieldStreamDesc")}</label>
            <input id="prof-desc" type="text" class="input-text" placeholder="Siaran dakwah dan kajian Islam ilmiah" bind:value={streamDesc} />
          </div>
          <div class="form-group flex-2">
            <label for="prof-url">{t("fieldStreamUrl")}</label>
            <input id="prof-url" type="text" class="input-text" placeholder="https://radiodakwah.com" bind:value={streamUrl} />
          </div>
        </div>
      </div>

      <div class="modal-footer">
        <button type="button" class="btn-cancel" onclick={onCancel}>{t("btnCancel")}</button>
        <button type="submit" class="btn-save" disabled={isSubmitting}>
          {t("btnSave")}
        </button>
      </div>
    </form>
  </div>
</div>

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background-color: rgba(15, 23, 42, 0.6);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    padding: 20px;
  }

  .modal-window {
    width: 100%;
    max-width: 650px;
    max-height: 90vh;
    overflow-y: auto;
    background: white;
    border-radius: 12px;
    box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.2);
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    border-bottom: 1px solid var(--color-border);
    padding-bottom: 12px;
  }

  .modal-title {
    font-size: 16px;
    font-weight: 700;
    color: var(--color-text-main);
  }

  .btn-close {
    background: transparent;
    border: none;
    font-size: 20px;
    line-height: 1;
    cursor: pointer;
    color: var(--color-text-muted);
  }

  .btn-close:hover {
    color: var(--color-text-main);
  }

  .modal-error {
    background-color: var(--color-danger-soft);
    color: var(--color-danger-text);
    padding: 8px 12px;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 500;
  }

  .modal-body {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .form-section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .form-row {
    display: flex;
    gap: 10px;
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .flex-1 { flex: 1; }
  .flex-2 { flex: 2; }

  label {
    font-size: 11px;
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
  }

  .input-text, .input-select {
    padding: 8px 12px;
    border: 1px solid var(--color-border);
    border-radius: 6px;
    font-size: 13px;
    color: var(--color-text-main);
    background-color: var(--color-page);
    outline: none;
  }

  .input-text:focus, .input-select:focus {
    border-color: var(--color-primary);
    background-color: white;
  }


  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    border-top: 1px solid var(--color-border);
    padding-top: 14px;
    margin-top: 8px;
  }

  .btn-cancel {
    padding: 8px 16px;
    border: 1px solid var(--color-border);
    border-radius: 6px;
    background: transparent;
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text-muted);
    cursor: pointer;
  }

  .btn-cancel:hover {
    background-color: var(--color-muted-bg);
    color: var(--color-text-main);
  }

  .btn-save {
    padding: 8px 20px;
    border: none;
    border-radius: 6px;
    background-color: var(--color-primary);
    color: white;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
  }

  .btn-save:hover:not(:disabled) {
    background-color: var(--color-primary-hover);
  }

  .btn-save:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
