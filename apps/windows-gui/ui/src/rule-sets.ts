import type { RuleSetStatus } from "./bridge";

/** How many lines a rule set skipped in all, and the types as `IP-ASN ×12, SCRIPT ×1`, most first. */
export function skippedLines(set: Pick<RuleSetStatus, "skipped">): { count: number; summary: string } {
  const entries = Object.entries(set.skipped ?? {}).sort(([a, x], [b, y]) => (y - x) || (a < b ? -1 : a > b ? 1 : 0));
  return {
    count: entries.reduce((sum, [, count]) => sum + count, 0),
    summary: entries.map(([kind, count]) => `${kind} ×${count}`).join(", "),
  };
}
