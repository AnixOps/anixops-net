/// <reference types="vite/client" />
import { afterEach, describe, expect, it } from "vitest";
import { resolveLanguage, setLanguage, sourceText, t } from "./i18n";
import { english } from "./i18n-en";
import { traditional } from "./i18n-zh-hant";
import { connectionSwitchLabel, connectionTitle, factLabel } from "./labels";

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

describe("words with more than one sense", () => {
  afterEach(() => setLanguage("zh"));

  it("shows the Chinese before the bar and translates each sense on its own", () => {
    expect(sourceText("连接|verb")).toBe("连接");
    expect(sourceText("a | b")).toBe("a | b");
    expect(t("连接|verb")).toBe("连接");
    expect(t("代理|mode")).toBe("代理");
    setLanguage("en");
    expect(t("连接")).toBe("Connections");
    expect(t("连接|noun")).toBe("Connection");
    expect(t("连接|verb")).toBe("Connect");
    expect(t("代理")).toBe("Proxies");
    expect(t("代理|mode")).toBe("Proxy");
    setLanguage("zh-Hant");
    expect(t("连接|verb")).toBe("連線");
  });

  // "连接" alone is the Connections list and "代理" alone the Proxies page. Anywhere else (an action, the connection
  // switch, a mode name) needs its sense key, or English shows a plural noun where a verb belongs (ux-i18n-a11y-04).
  it("uses the bare words only where the plural noun is meant", () => {
    const allowed: Record<string, string[]> = {
      "连接": ["./pages/ActivityPage.tsx", "./pages/CaptureViews.tsx", "./pages/RequestsPage.tsx"],
      "代理": ["./App.tsx"],
    };
    for (const [word, files] of Object.entries(allowed)) {
      const users = Object.entries(sources)
        .filter(([, source]) => codeLines(source).some((line) => line.includes(`"${word}"`)))
        .map(([file]) => file)
        .sort();
      expect(users).toEqual(files);
    }
  });

  it("names the connection switch the same whatever its state", () => {
    setLanguage("en");
    expect(connectionSwitchLabel()).toBe("Connection");
    for (const source of Object.values(sources)) {
      expect(source).not.toMatch(/label=\{phase === "on" \?/);
    }
  });
});
