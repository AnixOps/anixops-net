import { describe, expect, it } from "vitest";
import { initialMode, wantsDirectEgress } from "./mode";

describe("initialMode", () => {
  it("keeps a stored choice", () => {
    expect(initialMode("debug", true)).toBe("debug");
    expect(initialMode("proxy", false)).toBe("proxy");
  });

  it("asks a new user and skips the question for someone with a subscription", () => {
    expect(initialMode(null, false)).toBe("ask");
    expect(initialMode(null, true)).toBe("proxy");
  });

  it("waits until the backend has said whether there is a profile", () => {
    expect(initialMode(null, null)).toBe("wait");
  });
});

describe("wantsDirectEgress", () => {
  it("runs without a node only in debug mode with the direct exit or with no nodes yet", () => {
    expect(wantsDirectEgress("debug", "direct", true)).toBe(true);
    expect(wantsDirectEgress("debug", "node", false)).toBe(true);
    expect(wantsDirectEgress("debug", "node", true)).toBe(false);
    expect(wantsDirectEgress("proxy", "direct", false)).toBe(false);
  });
});
