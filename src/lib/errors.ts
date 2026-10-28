import { t, type TranslationKey } from "../i18n/index.svelte";

export interface FriendlyError {
  message: string;
  /** Raw backend text, for the "Details" disclosure and support. */
  detail: string | null;
}

// First match wins; codes come from src-tauri/src/commands.rs, phrases from
// streamy-core's SourceClientError / transport.
const RULES: Array<[RegExp, TranslationKey]> = [
  [/^no_profile$/, "errNoProfile"],
  [/^profile_missing$/, "errProfileMissing"],
  [/^password_missing$/, "errPasswordMissing"],
  [/^profile_host_required$/, "errHostRequired"],
  [/^profile_port_invalid$/, "errPortInvalid"],
  [/^profile_bitrate_invalid$/, "errBitrateInvalid"],
  [/^profile_samplerate_invalid$/, "errSampleRateInvalid"],
  [/^autostart_failed/, "errAutostart"],
  [/^playlist_empty$/, "errPlaylistEmpty"],
  [/^mic_unavailable$/, "errMicUnavailable"],
  [/^track_missing$/, "errTrackMissing"],
  [/unsupported|no audio track|probe|decode/i, "errUnsupportedFile"],
  [/mic_denied/, "errMicDenied"],
  [/mic_not_found/, "errMicNotFound"],
  [/mic_busy/, "errMicBusy"],
  [/mic_lost/, "errMicLost"],
  [/start the broadcast before recording/i, "errRecordWhileIdle"],
  [/auth rejected|unauthorized|401|403|invalid password|bad password/i, "errAuth"],
  [/cannot resolve|no such host|name or service not known|failed to lookup/i, "errResolve"],
  [/refused|actively refused|10061/i, "errRefused"],
  [/timed out|timeout|10060/i, "errTimeout"],
  [/handshake rejected|source rejected|nak|in use|not an icecast/i, "errHandshake"],
  [/connection lost|broken pipe|reset by peer|10054|aborted/i, "errLost"],
  [/gave_up/i, "errGaveUp"],
];

const CODE_ONLY = /^[a-z_]+$/;

export function friendlyError(raw: string | null | undefined): FriendlyError {
  const text = (raw ?? "").trim();
  if (!text) return { message: t("errGeneric"), detail: null };
  for (const [re, key] of RULES) {
    if (re.test(text)) {
      // Bare codes carry no extra information worth showing.
      return { message: t(key), detail: CODE_ONLY.test(text) ? null : text };
    }
  }
  return { message: text, detail: null };
}
