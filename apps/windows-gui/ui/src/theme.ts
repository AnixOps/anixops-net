import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";

// Colors are AnixOps Design CSS variables (design/anixops/tokens.css), aliased in design/tokens.css.
// AnixOps Design's dark theme is pure black, so the former "oled" choice reads as "dark".
export type Appearance = "system" | "light" | "dark";
export type Theme = "light" | "dark";

export const appearances: Array<{ value: Appearance; label: string }> = [
  { value: "system", label: "跟随系统" },
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
];

const storageKey = "anixops.appearance";

export function resolveTheme(appearance: Appearance, prefersDark: boolean): Theme {
  if (appearance === "system") return prefersDark ? "dark" : "light";
  return appearance;
}

function loadAppearance(): Appearance {
  try {
    const stored = window.localStorage.getItem(storageKey);
    if (stored === "oled") return "dark";
    if (stored && appearances.some((option) => option.value === stored)) return stored as Appearance;
  } catch {
    // Storage can be unavailable; fall back to the system appearance.
  }
  return "system";
}

/** Keeps the page theme and the native title bar in step with the chosen appearance and the system setting. */
export function useAppearance(): [Appearance, (next: Appearance) => void, Theme] {
  const [appearance, setAppearance] = useState<Appearance>(loadAppearance);
  const [prefersDark, setPrefersDark] = useState(() => window.matchMedia("(prefers-color-scheme: dark)").matches);
  const theme = resolveTheme(appearance, prefersDark);

  useEffect(() => {
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const update = (event: MediaQueryListEvent) => setPrefersDark(event.matches);
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    const native = appearance === "system" ? null : appearance;
    void getCurrentWindow().setTheme(native).catch(() => undefined);
  }, [theme, appearance]);

  const choose = (next: Appearance) => {
    setAppearance(next);
    try {
      window.localStorage.setItem(storageKey, next);
    } catch {
      // The choice still applies for this session.
    }
  };

  return [appearance, choose, theme];
}
