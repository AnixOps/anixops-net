import type { TrafficHour, TrafficStats } from "./bridge";

export type StatsPeriod = "today" | "week";

/** Unix seconds of the start of the period in local time: today's midnight, or six days before it. */
export function periodStart(period: StatsPeriod, now = new Date()): number {
  const midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  if (period === "week") midnight.setDate(midnight.getDate() - 6);
  return Math.floor(midnight.getTime() / 1000);
}

export function hoursSince(stats: TrafficStats | null, since: number): TrafficHour[] {
  return (stats?.hours ?? []).filter((hour) => hour.start >= since);
}

export function totals(hours: TrafficHour[]): { upload: number; download: number } {
  return hours.reduce((sum, hour) => ({ upload: sum.upload + hour.upload, download: sum.download + hour.download }), { upload: 0, download: 0 });
}

/** The hosts or policies that moved the most bytes, largest first, equal counts by name. */
export function shares(hours: TrafficHour[], key: "hosts" | "policies", limit = 20): Array<{ name: string; bytes: number }> {
  const sums = new Map<string, number>();
  for (const hour of hours) {
    for (const [name, bytes] of Object.entries(hour[key])) sums.set(name, (sums.get(name) ?? 0) + bytes);
  }
  return [...sums.entries()]
    .map(([name, bytes]) => ({ name, bytes }))
    .sort((a, b) => b.bytes - a.bytes || a.name.localeCompare(b.name))
    .slice(0, limit);
}

/** Bytes per bucket for the bar chart: the 24 hours of today, or the 7 days of the week, oldest first. */
export function buckets(hours: TrafficHour[], period: StatsPeriod, now = new Date()): Array<{ start: number; bytes: number }> {
  const start = periodStart(period, now);
  const count = period === "today" ? 24 : 7;
  const result = Array.from({ length: count }, (_, index) => {
    const date = new Date(start * 1000);
    if (period === "today") date.setHours(index);
    else date.setDate(date.getDate() + index);
    return { start: Math.floor(date.getTime() / 1000), bytes: 0 };
  });
  for (const hour of hours) {
    let index = result.length - 1;
    while (index > 0 && result[index].start > hour.start) index -= 1;
    if (hour.start >= result[0].start) result[index].bytes += hour.upload + hour.download;
  }
  return result;
}
