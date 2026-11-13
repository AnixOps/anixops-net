import { describe, expect, it } from "vitest";
import { needsConsent, termsBlocks, termsVersion, termsVersionOf } from "./terms";

describe("terms", () => {
  it("reads the version from the bundled TERMS.md", () => {
    expect(termsVersion).toMatch(/^\d+\.\d+\.\d+$/);
    expect(termsVersion).not.toBe("0.0.0");
    expect(termsVersionOf("基准版本 2.1.0，来源：x")).toBe("2.1.0");
    expect(termsVersionOf("no version here")).toBeNull();
  });

  it("asks until accepted, and again only for a new major version", () => {
    expect(needsConsent(null, "2.0.0")).toBe(true);
    expect(needsConsent("", "2.0.0")).toBe(true);
    expect(needsConsent("garbage", "2.0.0")).toBe(true);
    expect(needsConsent("2.0.0", "2.0.0")).toBe(false);
    expect(needsConsent("2.0.0", "2.3.1")).toBe(false);
    expect(needsConsent("1.0.0", "2.0.0")).toBe(true);
  });

  it("turns Markdown into headings, paragraphs and items without marks or comments", () => {
    const blocks = termsBlocks("# Title\n\n<!-- hidden -->\n## 1. Part\n\n- **one** item\n  continues\n- [two](https://x)\n\nplain\ntext\n\n---\n");
    expect(blocks).toEqual([
      { kind: "h1", text: "Title" },
      { kind: "h2", text: "1. Part" },
      { kind: "li", text: "one itemcontinues" },
      { kind: "li", text: "two" },
      { kind: "p", text: "plaintext" },
    ]);
  });
});
