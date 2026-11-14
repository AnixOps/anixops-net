import { describe, expect, it } from "vitest";
import { decryptionConsentNeeded, decryptionConsentPoints } from "./decryption-consent";
import termsMarkdown from "../../../../TERMS.md?raw";

describe("HTTPS decryption consent", () => {
  it("asks until confirmed once", () => {
    expect(decryptionConsentNeeded(null)).toBe(true);
    expect(decryptionConsentNeeded(undefined)).toBe(true);
    expect(decryptionConsentNeeded("")).toBe(true);
    expect(decryptionConsentNeeded("2026-11-14T00:00:00.000Z")).toBe(false);
  });

  it("points at the section of TERMS.md that covers capture and decryption", () => {
    expect(decryptionConsentPoints.some((point) => point.includes("第 4 节"))).toBe(true);
    expect(termsMarkdown).toMatch(/## 4\. 抓包、HTTPS 解密/);
  });
});
