import { describe, expect, it } from "vitest";
import { decryptsBroadly } from "./plugin-review";

describe("plugin review", () => {
  it("warns about plugins that decrypt every site", () => {
    expect(decryptsBroadly(["*"])).toBe(true);
    expect(decryptsBroadly(["api.example.com", "*.*"])).toBe(true);
    expect(decryptsBroadly(["*.com"])).toBe(true);
    expect(decryptsBroadly(["*.example.com", "ipleak.net"])).toBe(false);
    expect(decryptsBroadly(["-*"])).toBe(false);
    expect(decryptsBroadly([])).toBe(false);
  });
});
