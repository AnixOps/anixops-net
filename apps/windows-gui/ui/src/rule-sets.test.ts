import { describe, expect, it } from "vitest";
import { busiestRules, dataName, dataRule, skippedLines } from "./rule-sets";

describe("skippedLines", () => {
  it("is empty when nothing was skipped", () => {
    expect(skippedLines({})).toEqual({ count: 0, summary: "" });
    expect(skippedLines({ skipped: {} })).toEqual({ count: 0, summary: "" });
  });

  it("counts every line and lists the types, most first", () => {
    expect(skippedLines({ skipped: { SCRIPT: 1, "IP-ASN": 12, GEOIP: 1 } })).toEqual({
      count: 14,
      summary: "IP-ASN ×12, GEOIP ×1, SCRIPT ×1",
    });
  });
});

describe("data lists", () => {
  it("names the rule a missing list serves", () => {
    expect(dataRule("geoip-us")).toBe("GEOIP,US");
    expect(dataRule("asn-13335")).toBe("IP-ASN,13335");
  });

  it("reads the list name from what the user typed", () => {
    for (const text of ["us", "US", "GEOIP,US", "geoip-us", " GeoIP, us "]) expect(dataName(text)).toBe("geoip-us");
    for (const text of ["13335", "AS13335", "IP-ASN,13335", "ip-asn,AS13335", "asn-13335"]) expect(dataName(text)).toBe("asn-13335");
    for (const text of ["", "usa", "geoip-1a", "0", "AS013", "4294967296", "geosite-cn"]) expect(dataName(text)).toBeNull();
    expect(dataName("4294967295")).toBe("asn-4294967295");
  });

  it("lists the busiest rules first and leaves out the unused", () => {
    const rules = [{ rule: "a", hits: 0 }, { rule: "b", hits: 3 }, { rule: "c", hits: 9 }];
    expect(busiestRules(rules).map((rule) => rule.rule)).toEqual(["c", "b"]);
    expect(busiestRules(rules, 1).map((rule) => rule.rule)).toEqual(["c"]);
  });
});
