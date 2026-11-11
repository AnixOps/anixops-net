import type { ConnectionSnapshot, NodeSummary, TrafficSnapshot } from "./bridge";
import type { TileColor } from "./ui";

/** How many one-second samples the traffic chart keeps. */
export const historyLength = 60;

/** The latest traffic answer, the recent rates (oldest first) and the highest rate seen since connecting. */
export interface TrafficFeed {
  traffic: TrafficSnapshot | null;
  up: number[];
  down: number[];
  peak: number;
}

export const emptyFeed: TrafficFeed = { traffic: null, up: [], down: [], peak: 0 };

/** The feed after one more answer; no answer (disconnected, or the engine did not reply) clears it. */
export function pushTraffic(feed: TrafficFeed, traffic: TrafficSnapshot | null): TrafficFeed {
  if (!traffic) return feed.traffic === null ? feed : emptyFeed;
  return {
    traffic,
    up: [...feed.up, traffic.upload_rate].slice(-historyLength),
    down: [...feed.down, traffic.download_rate].slice(-historyLength),
    peak: Math.max(feed.peak, traffic.download_rate, traffic.upload_rate),
  };
}

const secondLevel = new Set(["com", "co", "net", "org", "gov", "edu", "ac"]);

/**
 * The site a host belongs to, so `i.ytimg.com` and `www.ytimg.com` count together: the last two labels, or three
 * for names like `example.com.cn`. Addresses and single-label names stay as they are; a port is dropped.
 */
export function siteOf(host: string): string {
  if (host.startsWith("[")) return host.slice(1, host.indexOf("]"));
  if (host.split(":").length > 2) return host.toLowerCase();
  const name = host.replace(/:\d+$/, "").toLowerCase();
  if (/^\d+(\.\d+){3}$/.test(name)) return name;
  const labels = name.split(".").filter(Boolean);
  if (labels.length <= 2) return name;
  const keep = labels[labels.length - 1].length === 2 && secondLevel.has(labels[labels.length - 2]) ? 3 : 2;
  return labels.slice(-keep).join(".");
}

/** Connections that share a site, a program or an outbound, added up. */
export interface UsageRow {
  key: string;
  connections: number;
  bytes: number;
  /** Where the largest of them went: the last policy of its chain, or the outbound. */
  via: string;
}

/** The `limit` keys that moved the most bytes, largest first; connections without a key are left out. */
export function topUsage(connections: ConnectionSnapshot[], keyOf: (connection: ConnectionSnapshot) => string | undefined | null, limit: number): UsageRow[] {
  const rows = new Map<string, UsageRow & { largest: number }>();
  for (const connection of connections) {
    const key = keyOf(connection);
    if (!key) continue;
    const bytes = connection.upload + connection.download;
    const via = connection.chain[connection.chain.length - 1] ?? connection.outbound;
    const row = rows.get(key);
    if (!row) {
      rows.set(key, { key, connections: 1, bytes, via, largest: bytes });
      continue;
    }
    row.connections += 1;
    row.bytes += bytes;
    if (bytes > row.largest) {
      row.largest = bytes;
      row.via = via;
    }
  }
  return [...rows.values()]
    .sort((a, b) => b.bytes - a.bytes || b.connections - a.connections || a.key.localeCompare(b.key))
    .slice(0, limit)
    .map(({ largest: _largest, ...row }) => row);
}

const letterColors: TileColor[] = ["indigo", "sky", "green", "orange", "purple", "pink", "teal", "red"];

/** A steady tile color for a name, so a site or program keeps its color between refreshes. */
export function colorFor(name: string): TileColor {
  let hash = 0;
  for (const char of name) hash = (hash * 31 + char.codePointAt(0)!) % 9973;
  return letterColors[hash % letterColors.length];
}

/** The node a policy group member stands for, by id first and then by its shown name. */
export function memberNode(nodes: NodeSummary[], outbound: string, label: string | undefined): NodeSummary | undefined {
  return nodes.find((node) => node.id === outbound) ?? nodes.find((node) => node.label === (label ?? outbound));
}

/** Whole days from `now` until the Unix-seconds `expire`; negative once it has passed. */
export function daysLeft(expire: number, now: number): number {
  return Math.floor((expire * 1000 - now) / 86_400_000);
}

/** The points of an area chart `width` × `height` for samples that end at the right edge, scaled to `max`. */
export function chartPoints(values: number[], width: number, height: number, max: number, slots = historyLength): Array<[number, number]> {
  const step = width / (slots - 1);
  const offset = (slots - values.length) * step;
  const top = Math.max(max, 1);
  return values.map((value, index) => [offset + index * step, height - (Math.min(value, top) / top) * (height - 4)]);
}

/** A round upper bound for the chart: 1, 2 or 5 times a power of 1024-ish units, never under 64 KB/s. */
export function chartCeiling(peak: number): number {
  let ceiling = 64 * 1024;
  const steps = [2, 2.5, 2];
  let index = 0;
  while (ceiling < peak) {
    ceiling *= steps[index % steps.length];
    index += 1;
  }
  return ceiling;
}
