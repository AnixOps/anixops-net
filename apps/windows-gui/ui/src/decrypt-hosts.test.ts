import { describe, expect, it } from "vitest";
import { coveredBy, isDecryptableHost, pendingSteps } from "./decrypt-hosts";

describe("decrypt hosts", () => {
  it("offers decryption for domain names only", () => {
    expect(isDecryptableHost("api.example.com")).toBe(true);
    expect(isDecryptableHost("API.example.com:443")).toBe(true);
    expect(isDecryptableHost("1.2.3.4")).toBe(false);
    expect(isDecryptableHost("[2001:db8::1]")).toBe(false);
    expect(isDecryptableHost("localhost")).toBe(false);
    expect(isDecryptableHost("")).toBe(false);
    expect(isDecryptableHost(null)).toBe(false);
  });

  it("knows which hosts the list already covers", () => {
    expect(coveredBy("api.example.com:443", ["api.example.com"])).toBe(true);
    expect(coveredBy("img.cdn.example.com", ["*.cdn.example.com"])).toBe(true);
    expect(coveredBy("cdn.example.com", ["*.cdn.example.com"])).toBe(false);
    expect(coveredBy("other.com", ["api.example.com"])).toBe(false);
  });

  it("lists the setup steps still to do, in order", () => {
    const ready = { connected: true, trusted: true, decrypting: true, domains: 2, capturing: true };
    expect(pendingSteps(ready)).toEqual([]);
    expect(pendingSteps({ ...ready, trusted: null })).toEqual([]);
    expect(pendingSteps({ connected: false, trusted: false, decrypting: false, domains: 0, capturing: false }))
      .toEqual(["connect", "certificate", "decryption", "domains", "capture"]);
  });
});
