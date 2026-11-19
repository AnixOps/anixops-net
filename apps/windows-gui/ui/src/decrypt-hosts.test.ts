import { describe, expect, it } from "vitest";
import { coveredBy, excludedBy, failureReason, isDecryptableHost, isFailing, matchesPattern, pendingSteps } from "./decrypt-hosts";

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

  it("reads wildcards as the engine does and lets exclusions win", () => {
    expect(matchesPattern("img-?.cdn.*", "IMG-1.cdn.test")).toBe(true);
    expect(matchesPattern("img-?.cdn.*", "img-12.cdn.test")).toBe(false);
    expect(matchesPattern("*spclient*.spotify.com", "spclient.wg.spotify.com")).toBe(true);
    expect(coveredBy("pay.example.com", ["*.example.com"], ["pay.example.com"])).toBe(false);
    expect(coveredBy("api.example.com", ["*.example.com"], ["pay.example.com"])).toBe(true);
    expect(excludedBy("x.bank.example.com", ["*.bank.example.com"])).toBe(true);
  });

  it("marks domains whose last connection failed", () => {
    expect(isFailing({ host: "a", decrypted: 0, failed: 2, last_failure: "client_closed", last_failure_at_ms: 5 })).toBe(true);
    expect(isFailing({ host: "a", decrypted: 3, failed: 1, last_failure_at_ms: 5, last_decrypted_at_ms: 9 })).toBe(false);
    expect(isFailing({ host: "a", decrypted: 3, failed: 0 })).toBe(false);
    expect(failureReason("client_rejected_certificate")).toContain("证书");
    expect(failureReason("something_new")).toBe("解密失败");
  });

  it("lists the setup steps still to do, in order", () => {
    const ready = { connected: true, trusted: true, decrypting: true, domains: 2, capturing: true };
    expect(pendingSteps(ready)).toEqual([]);
    expect(pendingSteps({ ...ready, trusted: null })).toEqual([]);
    expect(pendingSteps({ connected: false, trusted: false, decrypting: false, domains: 0, capturing: false }))
      .toEqual(["connect", "certificate", "decryption", "domains", "capture"]);
  });
});
