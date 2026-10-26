import type { ConnectionSnapshot } from "./bridge";
import { t } from "./i18n";
import { formatBytes } from "./subscription-format";

export function formatRate(bytesPerSecond: number): string {
  return `${formatBytes(bytesPerSecond)}/s`;
}

/** The host name when the engine knows one, else the requested destination. */
export function connectionName(connection: ConnectionSnapshot): string {
  return connection.host || connection.destination;
}

/** Rule, the policy chain it went through, the program and the transferred bytes. */
export function connectionDetail(connection: ConnectionSnapshot): string {
  const route = connection.chain.length ? connection.chain.join(" → ") : connection.outbound;
  return [
    connection.network.toUpperCase(),
    connection.rule ? `${connection.rule} · ${route}` : route,
    connection.process,
    `↑ ${formatBytes(connection.upload)} ↓ ${formatBytes(connection.download)}`,
    connection.error === "rejected" ? t("被规则拒绝") : connection.error ? t("连接失败") : null,
  ].filter(Boolean).join(" · ");
}

/** Case-insensitive match on the host, destination, rule, policies and program. */
export function matchesConnection(connection: ConnectionSnapshot, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  return [connection.host, connection.destination, connection.rule, connection.outbound, connection.process, ...connection.chain]
    .some((value) => value?.toLowerCase().includes(needle));
}
