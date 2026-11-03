import { useEffect, useState } from "react";
import type { RuntimeSnapshot } from "./bridge";

/** Whether this window runs the engine, so live traffic and connections can be read. */
export function liveEngine(snapshot: RuntimeSnapshot | null): boolean {
  return snapshot?.connection === "Connected" && snapshot.runtimeMode !== "service";
}

/**
 * The previous value when the next one carries the same data, so a poll that found nothing new does not hand
 * React a new object and redraw every page that reads it.
 */
export function keepIfUnchanged<T>(previous: T, next: T): T {
  if (previous === next) return previous;
  if (previous === null || next === null || typeof previous !== "object" || typeof next !== "object") return next;
  return JSON.stringify(previous) === JSON.stringify(next) ? previous : next;
}

/** Whether the window is showing; false while it is minimized or hidden in the tray. */
export function usePageVisible(): boolean {
  const [visible, setVisible] = useState(() => document.visibilityState !== "hidden");
  useEffect(() => {
    const update = () => setVisible(document.visibilityState !== "hidden");
    document.addEventListener("visibilitychange", update);
    return () => document.removeEventListener("visibilitychange", update);
  }, []);
  return visible;
}

/**
 * Calls `load` once a second while `enabled` and the window is showing. The value is null before the first answer,
 * after a failure and while disabled; `reload` asks again at once, after a close for example. An answer equal to the
 * last one keeps the last object.
 */
export function usePolling<T>(load: () => Promise<T>, enabled: boolean): [T | null, () => void] {
  const [value, setValue] = useState<T | null>(null);
  const [tick, setTick] = useState(0);
  const visible = usePageVisible();

  useEffect(() => {
    if (!enabled) {
      setValue(null);
      return;
    }
    if (!visible) return;
    let cancelled = false;
    const poll = () => load()
      .then((next) => !cancelled && setValue((previous) => keepIfUnchanged(previous, next)))
      .catch(() => !cancelled && setValue(null));
    void poll();
    const timer = window.setInterval(() => void poll(), 1000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
    // `load` is one of the bridge functions, which never change.
  }, [enabled, visible, tick]);

  return [value, () => setTick((current) => current + 1)];
}

/** How many rows a long list draws at first, and how many more each "show more" adds. */
export const listPage = 200;
