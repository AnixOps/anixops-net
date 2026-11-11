import { describe, expect, it } from "vitest";
import type { ConnectionSnapshot } from "./bridge";
import { chartCeiling, chartPoints, colorFor, daysLeft, emptyFeed, historyLength, memberNode, pushTraffic, siteOf, topUsage } from "./dashboard";

function connection(changes: Partial<ConnectionSnapshot>): ConnectionSnapshot {
  return { id: 1, network: "tcp", destination: "1.2.3.4:443", rule: "final", chain: [], outbound: "proxy", started_at_ms: 0, upload: 0, download: 0, ...changes };
}

describe("pushTraffic", () => {
  const sample = (rate: number) => ({ upload_total: 0, download_total: 0, upload_rate: rate / 10, download_rate: rate, active: 1 });

  it("keeps the last samples and the peak", () => {
    let feed = emptyFeed;
    for (let index = 1; index <= historyLength + 5; index += 1) feed = pushTraffic(feed, sample(index));
    expect(feed.down).toHaveLength(historyLength);
    expect(feed.down[feed.down.length - 1]).toBe(historyLength + 5);
    expect(feed.down[0]).toBe(6);
    expect(feed.peak).toBe(historyLength + 5);
  });

  it("clears when the engine stops answering", () => {
    const feed = pushTraffic(emptyFeed, sample(10));
    expect(pushTraffic(feed, null)).toBe(emptyFeed);
    expect(pushTraffic(emptyFeed, null)).toBe(emptyFeed);
  });
});

describe("siteOf", () => {
  it("groups hosts by site", () => {
    expect(siteOf("i.ytimg.com")).toBe("ytimg.com");
    expect(siteOf("WWW.Example.com.cn:443")).toBe("example.com.cn");
    expect(siteOf("api.bbc.co.uk")).toBe("bbc.co.uk");
    expect(siteOf("github.com")).toBe("github.com");
    expect(siteOf("localhost")).toBe("localhost");
  });

  it("leaves addresses alone", () => {
    expect(siteOf("1.2.3.4:443")).toBe("1.2.3.4");
    expect(siteOf("[2001:db8::1]:443")).toBe("2001:db8::1");
  });
});

describe("topUsage", () => {
  const list = [
    connection({ id: 1, host: "a.example.com", upload: 10, download: 90, chain: ["Proxy", "HK 01"] }),
    connection({ id: 2, host: "b.example.com", upload: 0, download: 500, chain: ["Media", "SG 02"] }),
    connection({ id: 3, host: "other.net", upload: 1, download: 1, outbound: "direct" }),
    connection({ id: 4, host: null }),
  ];

  it("adds up by key and names where the largest went", () => {
    const rows = topUsage(list, (item) => item.host && siteOf(item.host), 5);
    expect(rows).toEqual([
      { key: "example.com", connections: 2, bytes: 600, via: "SG 02" },
      { key: "other.net", connections: 1, bytes: 2, via: "direct" },
    ]);
  });

  it("keeps only the largest", () => {
    expect(topUsage(list, (item) => item.host, 1).map((row) => row.key)).toEqual(["b.example.com"]);
  });
});

describe("helpers", () => {
  it("gives a name the same color every time", () => {
    expect(colorFor("chrome.exe")).toBe(colorFor("chrome.exe"));
  });

  it("finds a group member's node by id, then by name", () => {
    const nodes = [{ id: "n1", label: "HK 01", protocol: "anx", selected: true }, { id: "n2", label: "SG 02", protocol: "vless", selected: false }];
    expect(memberNode(nodes, "n2", "whatever")?.id).toBe("n2");
    expect(memberNode(nodes, "tag", "HK 01")?.id).toBe("n1");
    expect(memberNode(nodes, "DIRECT", undefined)).toBeUndefined();
  });

  it("counts whole days until expiry", () => {
    expect(daysLeft(86_400 * 3, 86_400_000)).toBe(2);
    expect(daysLeft(0, 1)).toBe(-1);
  });

  it("right-aligns chart samples and clamps them", () => {
    const points = chartPoints([0, 50, 200], 590, 100, 100, 60);
    expect(points[2]).toEqual([590, 4]);
    expect(points[0]).toEqual([570, 100]);
    expect(points[1][1]).toBe(52);
  });

  it("rounds the chart ceiling up", () => {
    expect(chartCeiling(0)).toBe(64 * 1024);
    expect(chartCeiling(100 * 1024)).toBe(128 * 1024);
    expect(chartCeiling(6 * 1024 * 1024)).toBeGreaterThanOrEqual(6 * 1024 * 1024);
  });
});
