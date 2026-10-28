import { en, type TranslationKey } from "./en";
import { id } from "./id";

export type Language = "en" | "id";

// Reactive: any template or $derived calling t() re-renders on language change,
// without remounting the app.
let current = $state<Language>("en");

export function getLanguage(): Language {
  return current;
}

export function setLanguage(lang: Language) {
  current = lang === "id" ? "id" : "en";
  document.documentElement.lang = current;
}

export function t(key: TranslationKey, params?: Record<string, string | number>): string {
  const dict = current === "id" ? id : en;
  let text: string = dict[key] ?? en[key] ?? key;
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      text = text.split(`{${k}}`).join(String(v));
    }
  }
  return text;
}

export { en, id, type TranslationKey };
