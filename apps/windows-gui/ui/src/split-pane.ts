// The request page's split between the table and the inspector: where the inspector docks and how much room it
// takes. Kept apart from the component so the arithmetic is tested.

export type Dock = "right" | "bottom";

/** The least room each side keeps, in pixels along the split. */
export const splitLimits: Record<Dock, { table: number; inspector: number }> = {
  right: { table: 380, inspector: 340 },
  bottom: { table: 140, inspector: 200 },
};

/** The inspector's share when the user has not dragged the divider. */
export const defaultShare: Record<Dock, number> = { right: 0.42, bottom: 0.5 };

/**
 * The inspector's size in pixels for a share of `total`, keeping both sides at least their minimum. When the room
 * is too small for both minimums the inspector gets what the table's minimum leaves, but never less than nothing.
 */
export function inspectorSize(dock: Dock, share: number, total: number): number {
  const limits = splitLimits[dock];
  const most = Math.max(0, total - limits.table);
  const wanted = Math.round(total * share);
  return Math.max(Math.min(limits.inspector, most), Math.min(wanted, most));
}

/** The share that puts the divider `pixels` from the far edge, within the limits. */
export function shareFor(dock: Dock, pixels: number, total: number): number {
  if (total <= 0) return defaultShare[dock];
  const size = inspectorSize(dock, pixels / total, total);
  return Math.round((size / total) * 1000) / 1000;
}

export function parseDock(stored: string | null): Dock {
  return stored === "bottom" ? "bottom" : "right";
}

/** A stored share, or the default when it is missing or not a sensible fraction. */
export function parseShare(dock: Dock, stored: string | null): number {
  const value = stored === null ? NaN : Number(stored);
  return Number.isFinite(value) && value >= 0.1 && value <= 0.9 ? value : defaultShare[dock];
}
