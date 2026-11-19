import { describe, expect, it } from "vitest";
import { skippedLines } from "./rule-sets";

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
