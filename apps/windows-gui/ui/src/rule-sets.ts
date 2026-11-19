import type { RuleSetStatus } from "./bridge";

/** How many lines a rule set skipped in all, and the types as `IP-ASN ×12, SCRIPT ×1`, most first. */
export function skippedLines(set: Pick<RuleSetStatus, "skipped">): { count: number; summary: string } {
  const entries = Object.entries(set.skipped ?? {}).sort(([a, x], [b, y]) => (y - x) || (a < b ? -1 : a > b ? 1 : 0));
  return {
    count: entries.reduce((sum, [, count]) => sum + count, 0),
    summary: entries.map(([kind, count]) => `${kind} ×${count}`).join(", "),
  };
}

/** The rule a missing data list serves, as the user writes it: `geoip-us` is `GEOIP,US`, `asn-13335` is `IP-ASN,13335`. */
export function dataRule(name: string): string {
  if (name.startsWith("geoip-")) return `GEOIP,${name.slice("geoip-".length).toUpperCase()}`;
  if (name.startsWith("asn-")) return `IP-ASN,${name.slice("asn-".length)}`;
  return name;
}

/** The list name for what the user typed: `us`, `GEOIP,US` or `geoip-us`, `AS13335`, `IP-ASN,13335` or `asn-13335`. */
export function dataName(text: string): string | null {
  const value = text.trim().toLowerCase().replace(/^geoip[,-]\s*/, "geoip-").replace(/^ip-asn,\s*/, "asn-").replace(/^(asn-)?as(?=\d)/, "asn-");
  if (/^[a-z]{2}$/.test(value)) return `geoip-${value}`;
  if (/^geoip-[a-z]{2}$/.test(value)) return value;
  const number = /^asn-([1-9]\d{0,9})$/.exec(value)?.[1] ?? /^([1-9]\d{0,9})$/.exec(value)?.[1];
  return number && Number(number) <= 4294967295 ? `asn-${number}` : null;
}

/** The rules that decided at least one connection, most first, at most `limit`. */
export function busiestRules<T extends { hits: number }>(rules: T[], limit = 50): T[] {
  return rules.filter((rule) => rule.hits > 0).sort((a, b) => b.hits - a.hits).slice(0, limit);
}
