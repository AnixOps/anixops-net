import { useCallback, useState } from "react";
import { t } from "./i18n";

// One app, two ways to use it (debug client plan A3/A4): proxy mode is the client as it was, debug mode puts capture
// first and keeps subscriptions and nodes under "Egress" (G2). Debug mode with the direct exit runs the engine with no
// node, so switching modes can restart a running connection; otherwise the mode only changes the window.
export type AppMode = "proxy" | "debug";

// Debug first: it is what the app is for (A1), and the first-launch dialog lists it first. Names and stored values
// match the Apple clients (docs/debug-mode.md).
export const modes: Array<{ value: AppMode; label: string; hint: string }> = [
  { value: "debug", label: "调试网络", hint: "抓包、HTTPS 解密、改写和重放，给开发和测试用" },
  { value: "proxy", label: "代理|mode", hint: "订阅、节点和分流规则，日常上网用" },
];

/** Where debug mode's traffic leaves (G2): straight out, or through the nodes and rules of proxy mode. */
export type DebugExit = "direct" | "node";

const storageKey = "anixops.app-mode";
const exitKey = "anixops.debug-exit";

function loadMode(): AppMode | null {
  try {
    const stored = window.localStorage.getItem(storageKey);
    return stored === "proxy" || stored === "debug" ? stored : null;
  } catch {
    return null;
  }
}

/**
 * The first-launch question is skipped for someone who already has a subscription or nodes (they came for the
 * proxy, for example by importing a subscription link): they stay in proxy mode until they switch.
 */
export function initialMode(stored: AppMode | null, hasProfile: boolean | null): AppMode | "ask" | "wait" {
  if (stored) return stored;
  if (hasProfile === null) return "wait";
  return hasProfile ? "proxy" : "ask";
}

/** The chosen mode (null before the first choice) and a function that saves a choice. */
export function useMode(): [AppMode | null, (next: AppMode) => void] {
  const [mode, setMode] = useState<AppMode | null>(loadMode);
  const choose = useCallback((next: AppMode) => {
    setMode(next);
    try {
      window.localStorage.setItem(storageKey, next);
    } catch {
      // The choice holds for this session; the question comes back next start.
    }
  }, []);
  return [mode, choose];
}

function loadExit(): DebugExit {
  try {
    return window.localStorage.getItem(exitKey) === "node" ? "node" : "direct";
  } catch {
    return "direct";
  }
}

/** Debug mode's egress, direct until the user picks the nodes or imports some in debug mode. */
export function useDebugExit(): [DebugExit, (next: DebugExit) => void] {
  const [exit, setExit] = useState<DebugExit>(loadExit);
  const choose = useCallback((next: DebugExit) => {
    setExit(next);
    try {
      window.localStorage.setItem(exitKey, next);
    } catch {
      // Holds for this session.
    }
  }, []);
  return [exit, choose];
}

/** The engine runs with no node in debug mode with the direct exit, and in debug mode before any node exists. */
export function wantsDirectEgress(mode: AppMode, exit: DebugExit, hasNodes: boolean): boolean {
  return mode === "debug" && (exit === "direct" || !hasNodes);
}

export function modeLabel(mode: AppMode): string {
  return t(modes.find((option) => option.value === mode)?.label ?? "");
}
