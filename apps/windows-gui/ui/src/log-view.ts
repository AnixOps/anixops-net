import type { LogEntry, LogLevel } from "./bridge";

/** Which lines the log viewer shows: all, warnings and errors, or errors only. */
export type LevelFilter = "all" | "warn" | "error";

const rank: Record<LogLevel, number> = { info: 0, warn: 1, error: 2 };

/** The modules present, in the order they first appear. */
export function logModules(entries: LogEntry[]): string[] {
  return [...new Set(entries.map((entry) => entry.module))];
}

/** The lines at or above `level`, of `module` (empty for all) and containing `query`, newest first. */
export function filterLogs(entries: LogEntry[], level: LevelFilter, module: string, query: string): LogEntry[] {
  const floor = level === "all" ? 0 : rank[level];
  const needle = query.trim().toLowerCase();
  return entries
    .filter((entry) => rank[entry.level] >= floor)
    .filter((entry) => !module || entry.module === module)
    .filter((entry) => !needle || entry.message.toLowerCase().includes(needle))
    .reverse();
}

/** The lines as plain text, oldest first, for copying. */
export function logText(entries: LogEntry[]): string {
  return [...entries]
    .reverse()
    .map((entry) => `${entry.timeMs ? new Date(entry.timeMs).toISOString() : "-"} ${entry.level.toUpperCase()} [${entry.module}] ${entry.message}`)
    .join("\n");
}
