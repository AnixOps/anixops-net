import { useState } from "react";
import { english } from "./i18n-en";
import { traditional } from "./i18n-zh-hant";

// Simplified Chinese is the source language: the UI is written with Chinese strings, and `t` looks each one up in
// the English or Traditional Chinese table when that language is active. Strings a table lacks are shown as written.
export type Language = "zh" | "zh-Hant" | "en";
export type LanguageChoice = "system" | Language;

// Each language is named in its own script, so it is never translated.
export const languages: Array<{ value: LanguageChoice; label: string }> = [
  { value: "system", label: "跟随系统" },
  { value: "zh", label: "简体中文" },
  { value: "zh-Hant", label: "繁體中文" },
  { value: "en", label: "English" },
];

const storageKey = "anixops.language";
let current: Language = "zh";

/** Translates a Chinese UI string and fills `{name}` placeholders. */
export function t(text: string, values?: Record<string, string | number | null | undefined>): string {
  const table = current === "en" ? english : current === "zh-Hant" ? traditional : undefined;
  let result = table?.[text] ?? text;
  if (values) {
    for (const [name, value] of Object.entries(values)) result = result.split(`{${name}}`).join(String(value ?? ""));
  }
  return result;
}

export function setLanguage(language: Language) {
  current = language;
  if (typeof document !== "undefined") document.documentElement.lang = language === "zh" ? "zh-CN" : language === "zh-Hant" ? "zh-TW" : "en";
}

export function resolveLanguage(choice: LanguageChoice, systemLanguage: string | undefined): Language {
  if (choice !== "system") return choice;
  const system = systemLanguage?.toLowerCase() ?? "";
  if (!system.startsWith("zh")) return "en";
  // zh-TW, zh-HK, zh-MO and zh-Hant-* read Traditional Chinese.
  return /^zh-(hant|tw|hk|mo)\b/.test(system) ? "zh-Hant" : "zh";
}

function loadChoice(): LanguageChoice {
  try {
    const stored = window.localStorage.getItem(storageKey);
    if (stored && languages.some((option) => option.value === stored)) return stored as LanguageChoice;
  } catch {
    // Storage can be unavailable; follow the system language.
  }
  return "system";
}

/**
 * The language choice, applied before the caller's children render. The app re-renders from the root when it
 * changes, so every `t` call picks up the new language.
 */
export function useLanguage(): [LanguageChoice, (next: LanguageChoice) => void] {
  const [choice, setChoice] = useState<LanguageChoice>(loadChoice);
  setLanguage(resolveLanguage(choice, typeof navigator === "undefined" ? undefined : navigator.language));

  const choose = (next: LanguageChoice) => {
    setChoice(next);
    try {
      window.localStorage.setItem(storageKey, next);
    } catch {
      // The choice still applies for this session.
    }
  };

  return [choice, choose];
}
