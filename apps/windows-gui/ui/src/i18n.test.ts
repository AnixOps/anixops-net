/// <reference types="vite/client" />
import { afterEach, describe, expect, it } from "vitest";
import { resolveLanguage, setLanguage, t } from "./i18n";
import { english } from "./i18n-en";
import { traditional } from "./i18n-zh-hant";
import { connectionTitle, factLabel } from "./labels";

const sources = import.meta.glob(["./**/*.ts", "./**/*.tsx", "!./**/*.test.ts", "!./i18n-en.ts", "!./i18n-zh-hant.ts"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const cjk = /[一-鿿]/;

function codeLines(source: string): string[] {
  return source.split("\n").filter((line) => !/^\s*(\/\/|\*|\/\*)/.test(line));
}

describe("English table", () => {
  it("has an entry for every Chinese string in the UI", () => {
    const missing: string[] = [];
    for (const [file, source] of Object.entries(sources)) {
      for (const line of codeLines(source)) {
        for (const match of line.matchAll(/"([^"\n]*)"/g)) {
          if (cjk.test(match[1]) && !(match[1] in english)) missing.push(`${file}: ${match[1]}`);
        }
      }
    }
    expect(missing).toEqual([]);
  });

  it("finds no Chinese in template literals or bare JSX text, which t() would miss", () => {
    const untranslated: string[] = [];
    for (const [file, source] of Object.entries(sources)) {
      for (const line of codeLines(source)) {
        if (/`[^`]*[一-鿿]/.test(line) || />[^<>{}"]*[一-鿿][^<>{}"]*</.test(line)) untranslated.push(`${file}: ${line.trim()}`);
      }
    }
    expect(untranslated).toEqual([]);
  });
});

describe("t", () => {
  afterEach(() => setLanguage("zh"));

  it("keeps Chinese by default and translates when English is active", () => {
    expect(t("已连接")).toBe("已连接");
    setLanguage("en");
    expect(t("已连接")).toBe("Connected");
    expect(connectionTitle("Configuration error", "failed")).toBe("Configuration error");
    expect(factLabel("Running (PID 42)")).toBe("Running");
  });

  it("fills placeholders and falls back to the source text", () => {
    setLanguage("en");
    expect(t("{count} 个节点", { count: 3 })).toBe("3 nodes");
    expect(t("没有这条文案")).toBe("没有这条文案");
  });

  it("follows the system language unless one is chosen", () => {
    expect(resolveLanguage("system", "zh-CN")).toBe("zh");
    expect(resolveLanguage("system", "en-US")).toBe("en");
    expect(resolveLanguage("system", undefined)).toBe("en");
    expect(resolveLanguage("zh", "en-US")).toBe("zh");
    expect(resolveLanguage("system", "zh-TW")).toBe("zh-Hant");
    expect(resolveLanguage("system", "zh-HK")).toBe("zh-Hant");
    expect(resolveLanguage("system", "zh-Hant-TW")).toBe("zh-Hant");
    expect(resolveLanguage("system", "zh-Hans-CN")).toBe("zh");
    expect(resolveLanguage("system", "zh")).toBe("zh");
    expect(resolveLanguage("zh-Hant", "en-US")).toBe("zh-Hant");
  });

  it("translates to Traditional Chinese", () => {
    setLanguage("zh-Hant");
    expect(t("已连接")).toBe("已連線");
    expect(t("{count} 个节点", { count: 3 })).toBe("3 個節點");
    expect(t("没有这条文案")).toBe("没有这条文案");
  });
});

describe("Traditional Chinese table", () => {
  it("covers every key of the English table, with the same placeholders", () => {
    const placeholders = (text: string) => [...text.matchAll(/\{\w+\}/g)].map((match) => match[0]).sort();
    const missing = Object.keys(english).filter((key) => !(key in traditional));
    expect(missing).toEqual([]);
    const mismatched = Object.keys(traditional).filter(
      (key) => placeholders(key).join() !== placeholders(traditional[key]).join(),
    );
    expect(mismatched).toEqual([]);
  });
});
