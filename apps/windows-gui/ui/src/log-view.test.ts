import { describe, expect, it } from "vitest";
import type { LogEntry } from "./bridge";
import { filterLogs, logModules, logText } from "./log-view";

const entries: LogEntry[] = [
  { timeMs: 1, module: "app", level: "info", message: "Connected through HK" },
  { timeMs: 2, module: "gui", level: "warn", message: "slow start" },
  { timeMs: 3, module: "app", level: "error", message: "subscription update failed" },
];

describe("log view", () => {
  it("lists modules once", () => {
    expect(logModules(entries)).toEqual(["app", "gui"]);
  });

  it("filters by level, module and text, newest first", () => {
    expect(filterLogs(entries, "all", "", "").map((entry) => entry.timeMs)).toEqual([3, 2, 1]);
    expect(filterLogs(entries, "warn", "", "").map((entry) => entry.timeMs)).toEqual([3, 2]);
    expect(filterLogs(entries, "error", "", "").map((entry) => entry.timeMs)).toEqual([3]);
    expect(filterLogs(entries, "all", "app", "").map((entry) => entry.timeMs)).toEqual([3, 1]);
    expect(filterLogs(entries, "all", "", " hk ").map((entry) => entry.timeMs)).toEqual([1]);
  });

  it("copies oldest first", () => {
    const text = logText(filterLogs(entries, "error", "", ""));
    expect(text).toBe("1970-01-01T00:00:00.003Z ERROR [app] subscription update failed");
    expect(logText([{ timeMs: 0, module: "app", level: "info", message: "x" }])).toBe("- INFO [app] x");
  });
});
