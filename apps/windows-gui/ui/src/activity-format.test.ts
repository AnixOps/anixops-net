import { describe, expect, it } from "vitest";
import type { ConnectionSnapshot } from "./bridge";
import { connectionDetail, connectionName, formatRate, matchesConnection } from "./activity-format";

const connection: ConnectionSnapshot = {
  id: 1,
  network: "tcp",
  host: "www.example.com",
  process: "chrome.exe",
  destination: "93.184.216.34:443",
  rule: "DOMAIN-SUFFIX,example.com",
  chain: ["Proxy", "Tokyo"],
  outbound: "Tokyo",
  started_at_ms: 0,
  upload: 1536,
  download: 512,
};

describe("connection labels", () => {
  it("names a connection by host and falls back to the destination", () => {
    expect(connectionName(connection)).toBe("www.example.com");
    expect(connectionName({ ...connection, host: null })).toBe("93.184.216.34:443");
  });

  it("shows the rule, chain, program and bytes", () => {
    expect(connectionDetail(connection)).toBe("TCP · DOMAIN-SUFFIX,example.com · Proxy → Tokyo · chrome.exe · ↑ 1.50 KB ↓ 512 B");
    expect(connectionDetail({ ...connection, rule: "", chain: [], process: undefined, error: "rejected" })).toBe("TCP · Tokyo · ↑ 1.50 KB ↓ 512 B · 被规则拒绝");
  });

  it("formats rates and filters by any field", () => {
    expect(formatRate(2048)).toBe("2.00 KB/s");
    expect(matchesConnection(connection, "CHROME")).toBe(true);
    expect(matchesConnection(connection, "proxy")).toBe(true);
    expect(matchesConnection(connection, "  ")).toBe(true);
    expect(matchesConnection(connection, "openai")).toBe(false);
  });
});
