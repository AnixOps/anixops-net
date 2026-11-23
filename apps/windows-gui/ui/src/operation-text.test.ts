/// <reference types="vite/client" />
import { afterEach, describe, expect, it } from "vitest";
import { setLanguage } from "./i18n";
import { english } from "./i18n-en";
import { traditional } from "./i18n-zh-hant";
import { operationText, operationTexts } from "./operation-text";

// The backends that answer this window: the Windows app and the shared client core (Linux).
const backends = import.meta.glob(["../../src/gui/**/*.rs", "../../../../crates/client-core/src/**/*.rs"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

// The areas result codes are named in; a quoted `<area>.<name>` in the backends is a code (file names aside).
const areas = [
  "certificate", "connect", "crash_reports", "data_lists", "delay", "devices", "diagnostics", "disconnect", "dns",
  "driver", "egress", "fastest", "group", "links", "mitm", "node", "nodes", "plugins", "preferences", "profile", "proxy",
  "quic", "redirects", "restart", "root", "routing", "rules", "script_runtime", "subscription", "tun", "validate",
];
const codePattern = new RegExp(`"((?:${areas.join("|")})\\.[a-z_]+)"`, "g");

function backendCodes(): string[] {
  const codes = new Set<string>();
  for (const source of Object.values(backends)) {
    for (const match of source.matchAll(codePattern)) {
      if (!/\.(json|log|txt|rs|ya?ml)$/.test(match[1])) codes.add(match[1]);
    }
  }
  return [...codes].sort();
}

describe("operation result codes", () => {
  afterEach(() => setLanguage("zh"));

  it("reads the backends' sources", () => {
    expect(Object.keys(backends).some((file) => file.endsWith("tauri_ui.rs"))).toBe(true);
    expect(Object.keys(backends).some((file) => file.includes("client-core"))).toBe(true);
  });

  it("has words for every code a backend sends, in all three languages", () => {
    const codes = backendCodes();
    expect(codes.length).toBeGreaterThan(50);
    expect(codes.filter((code) => !(code in operationTexts))).toEqual([]);
    const texts = Object.values(operationTexts);
    expect(texts.filter((text) => !(text in english))).toEqual([]);
    expect(texts.filter((text) => !(text in traditional))).toEqual([]);
  });

  it("words a result in the window's language and falls back to the backend's message", () => {
    const result = { message: "3 rules saved and applied; open connections stay.", code: "rules.applied", params: { count: "3" } };
    expect(operationText(result)).toBe("已保存并应用 3 条规则，现有连接不受影响");
    setLanguage("en");
    expect(operationText(result)).toBe("3 rules saved and applied; open connections stay");
    setLanguage("zh-Hant");
    expect(operationText(result)).toBe("已儲存並套用 3 條規則，現有連線不受影響");
    // The note is in the window's language already and follows the text.
    expect(operationText({ message: "Disconnected", code: "disconnect.done", note: "目前的抓包已儲存為「中斷連線前」。" })).toBe(
      "已中斷連線 目前的抓包已儲存為「中斷連線前」。",
    );
    // A code this window does not know (another version of the backend), or none: the backend's own words.
    expect(operationText({ message: "Something new happened.", code: "future.thing" })).toBe("Something new happened.");
    expect(operationText({ message: "Plain." })).toBe("Plain.");
  });
});
