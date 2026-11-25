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

const errorReasons: Record<string, string> = {
  fetch_timeout: "下载订阅超时",
  fetch_unreachable: "无法访问订阅服务器",
  fetch_failed: "下载订阅失败",
  payload_too_large: "订阅内容超过 16 MiB 上限",
  invalid_payload: "订阅内容里没有可用的节点",
  runtime_active: "先断开连接再使用这份订阅",
  // The kernel's own parse refusals (`subscription.core.<kind>`, config-core).
  parse_failed: "无法识别订阅内容的格式",
  inline_payload_empty: "订阅内容是空的",
  location_empty: "订阅链接是空的",
};

/** The kernel's other parse refusals, by how their kind ends. */
function kernelReason(kind: string): string | undefined {
  if (kind.endsWith("_unsupported")) return "订阅里有本应用不支持的节点类型或写法";
  if (kind.endsWith("_invalid")) return "订阅里有写法不正确的节点";
  return undefined;
}

/**
 * What a failed update's code means, with the code after it for a report. The app keeps only the code because the
 * raw error can carry the subscription's token.
 */
export function subscriptionErrorText(code: string | null | undefined): string | undefined {
  if (!code) return undefined;
  const kind = code.slice(code.lastIndexOf(".") + 1);
  const status = /^http_status_(\d+)$/.exec(kind)?.[1];
  let reason: string;
  if (status && ["401", "403", "404", "410"].includes(status)) reason = t("订阅服务器拒绝了请求（HTTP {status}），订阅可能已过期或链接已失效", { status });
  else if (status) reason = t("订阅服务器返回了错误（HTTP {status}）", { status });
  else reason = t(errorReasons[kind] ?? kernelReason(kind) ?? "更新订阅失败");
  return `${reason} (${code})`;
}
