import { useEffect, useState } from "react";
import type { RuntimeSnapshot } from "./bridge";

/** Whether this window runs the engine, so live traffic and connections can be read. */
export function liveEngine(snapshot: RuntimeSnapshot | null): boolean {
  return snapshot?.connection === "Connected" && snapshot.runtimeMode !== "service";
}

/**
 * Calls `load` once a second while `enabled`. The value is null before the first answer, after a failure and
 * while disabled; `reload` asks again at once, after a close for example.
 */
export function usePolling<T>(load: () => Promise<T>, enabled: boolean): [T | null, () => void] {
  const [value, setValue] = useState<T | null>(null);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    if (!enabled) {
      setValue(null);
      return;
    }
    let cancelled = false;
    const poll = () => load().then((next) => !cancelled && setValue(next)).catch(() => !cancelled && setValue(null));
    void poll();
    const timer = window.setInterval(() => void poll(), 1000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
    // `load` is one of the bridge functions, which never change.
  }, [enabled, tick]);

  return [value, () => setTick((current) => current + 1)];
}
