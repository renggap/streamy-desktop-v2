import { en, type TranslationKey } from "./en";
import { id } from "./id";

export type Language = "en" | "id";

let currentLang: Language = "en";
const listeners = new Set<() => void>();

export function getLanguage(): Language {
  return currentLang;
}

export function setLanguage(lang: Language) {
  if (currentLang !== lang) {
    currentLang = lang;
    for (const l of listeners) l();
  }
}

export function subscribeLanguage(fn: () => void): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

export function t(key: TranslationKey, params?: Record<string, string | number>): string {
  const dict = currentLang === "id" ? id : en;
  let text = dict[key] || en[key] || (key as string);
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      text = text.replace(new RegExp(`\\{${k}\\}`, "g"), String(v));
    }
  }
  return text;
}

export { en, id, type TranslationKey };
