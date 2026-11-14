import { describe, expect, it } from "vitest";
import type { TrafficHour } from "./bridge";
import { buckets, hoursSince, periodStart, shares, totals } from "./traffic-stats";

const now = new Date(2026, 10, 14, 15, 30);
const today = periodStart("today", now);

function hour(offsetHours: number, upload: number, download: number, hosts: Record<string, number> = {}, policies: Record<string, number> = {}): TrafficHour {
  return { start: today + offsetHours * 3600, upload, download, hosts, policies };
}

describe("traffic statistics", () => {
  it("starts today at local midnight and the week six days before", () => {
    expect(new Date(today * 1000).getHours()).toBe(0);
    expect(today - periodStart("week", now)).toBeGreaterThanOrEqual(6 * 86400 - 3600);
  });

  it("adds up the period and ranks hosts", () => {
    const hours = [hour(-30, 100, 100, { old: 200 }), hour(1, 10, 20, { a: 20, b: 10 }), hour(3, 5, 5, { b: 10 }, { Proxy: 10 })];
    const kept = hoursSince({ hours }, today);
    expect(kept).toHaveLength(2);
    expect(totals(kept)).toEqual({ upload: 15, download: 25 });
    expect(shares(kept, "hosts")).toEqual([{ name: "a", bytes: 20 }, { name: "b", bytes: 20 }]);
    expect(shares(kept, "policies")).toEqual([{ name: "Proxy", bytes: 10 }]);
  });

  it("puts each hour in its bar", () => {
    const bars = buckets([hour(1, 10, 20), hour(3, 5, 5), hour(-30, 1, 1)], "today", now);
    expect(bars).toHaveLength(24);
    expect(bars[1].bytes).toBe(30);
    expect(bars[3].bytes).toBe(10);
    expect(bars.reduce((sum, bar) => sum + bar.bytes, 0)).toBe(40);
    const week = buckets([hour(1, 10, 20), hour(-30, 1, 1)], "week", now);
    expect(week).toHaveLength(7);
    expect(week[6].bytes).toBe(30);
    expect(week[4].bytes).toBe(2);
  });
});
