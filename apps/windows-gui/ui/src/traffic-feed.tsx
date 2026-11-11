import { useEffect, useSyncExternalStore } from "react";
import { desktop } from "./bridge";
import { emptyFeed, pushTraffic, TrafficFeed } from "./dashboard";
import { usePageVisible } from "./live";

// One feed for the whole window: the sidebar and the overview read the same samples, and the chart keeps its
// history while another page is open.
let feed: TrafficFeed = emptyFeed;
const listeners = new Set<() => void>();

function publish(next: TrafficFeed) {
  if (next === feed) return;
  feed = next;
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * Reads the engine's traffic once a second while `enabled` and the window is showing, and feeds everything that
 * calls `useTraffic`. Every answer is a sample, an unchanged one too, so an idle chart keeps moving.
 */
export function TrafficSource({ enabled }: { enabled: boolean }) {
  const visible = usePageVisible();
  useEffect(() => {
    if (!enabled) {
      publish(emptyFeed);
      return;
    }
    if (!visible) return;
    let cancelled = false;
    const poll = () => desktop.traffic()
      .then((traffic) => { if (!cancelled) publish(pushTraffic(feed, traffic)); })
      .catch(() => { if (!cancelled) publish(emptyFeed); });
    void poll();
    const timer = window.setInterval(() => void poll(), 1000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [enabled, visible]);
  return null;
}

/** The live traffic feed; empty while disconnected. */
export function useTraffic(): TrafficFeed {
  return useSyncExternalStore(subscribe, () => feed);
}
