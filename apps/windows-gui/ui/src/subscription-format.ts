import { t } from "./i18n";

/** Automatic update intervals offered per subscription, in hours; 0 is by hand. */
export const updateIntervalPresets = [0, 1, 3, 6, 12, 24, 48, 72, 168];

/** The presets, plus a server-chosen interval that is not one of them. */
export function updateIntervalChoices(current: number): number[] {
  return updateIntervalPresets.includes(current) ? updateIntervalPresets : [...updateIntervalPresets, current].sort((a, b) => a - b);
}

export function updateIntervalLabel(hours: number): string {
  if (hours === 0) return t("手动更新");
  if (hours === 168) return t("每周自动更新");
  if (hours % 24 === 0) return t("每 {days} 天自动更新", { days: hours / 24 });
  return t("每 {hours} 小时自动更新", { hours });
}

/** Binary units, as the iOS app shows them. */
export function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${value} ${units[0]}` : `${value.toFixed(value >= 100 ? 0 : value >= 10 ? 1 : 2)} ${units[unit]}`;
}

/** `YYYY-MM-DD` in local time from Unix seconds. */
export function formatDay(unixSeconds: number): string {
  const date = new Date(unixSeconds * 1000);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** Used share of the total, 0..1, when both are known. */
export function usedFraction(used: number | undefined, total: number | undefined): number | undefined {
  if (used == null || !total) return undefined;
  return Math.min(1, used / total);
}
