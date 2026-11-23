import { describe, expect, it } from "vitest";
import privacyMarkdown from "../../../../PRIVACY.md?raw";
import { needsConsent, privacyForLanguage, termsBlocks, termsVersion, termsVersionOf } from "./terms";

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

  it("shows the privacy policy half in the app's language, without the in-page links", () => {
    const sample = "# Policy\n\nEffective\n\n[English](#english) · [中文](#中文)\n\n## English\n\nWe collect nothing.\n\n## 中文\n\n我们不收集。\n";
    const english = privacyForLanguage(sample, true);
    const chinese = privacyForLanguage(sample, false);
    expect(english.startsWith("# Policy\n\nEffective")).toBe(true);
    expect(english.endsWith("We collect nothing.")).toBe(true);
    expect(english).not.toContain("我们不收集");
    expect(english).not.toContain("](#");
    expect(chinese.startsWith("# Policy\n\nEffective")).toBe(true);
    expect(chinese.endsWith("我们不收集。")).toBe(true);
    expect(chinese).not.toContain("We collect");
    expect(privacyForLanguage("# Only one half\n", true)).toBe("# Only one half\n");
  });

  it("bundles PRIVACY.md with both halves", () => {
    const english = privacyForLanguage(privacyMarkdown, true);
    const chinese = privacyForLanguage(privacyMarkdown, false);
    expect(english).toContain("What we collect");
    expect(english).not.toContain("我们收集什么");
    expect(chinese).toContain("我们收集什么");
    expect(chinese).not.toContain("What we collect");
    expect(termsBlocks(chinese).length).toBeGreaterThan(5);
  });
});
