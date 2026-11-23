/// <reference types="vite/client" />
import { afterEach, describe, expect, it } from "vitest";
import { errorLine, errorText, errorTexts, failedOperation, operationFailures, routingWarningKinds, routingWarningText, splitRefusal, subscriptionFailureLine } from "./error-text";
import { setLanguage } from "./i18n";
import { english } from "./i18n-en";
import { traditional } from "./i18n-zh-hant";

// The backends that answer this window: the Windows app and the shared client core (Linux).
const backends = import.meta.glob(["../../src/gui/**/*.rs", "../../../../crates/client-core/src/**/*.rs"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

function backendErrorCodes(): string[] {
  const codes = new Set<string>();
  for (const source of Object.values(backends)) {
    for (const match of source.matchAll(/"(error\.[a-z_]+)"/g)) codes.add(match[1]);
  }
  return [...codes].sort();
}

describe("refusal codes", () => {
  afterEach(() => setLanguage("zh"));

  it("has words for every code a backend refuses with, in all three languages", () => {
    const codes = backendErrorCodes();
    expect(codes.length).toBeGreaterThan(10);
    // `error.subscription_failed` is worded by the reason code inside it (subscription-format.ts).
    expect(codes.filter((code) => code !== "error.subscription_failed" && !(code in errorTexts))).toEqual([]);
    const texts = [...Object.values(errorTexts), ...Object.values(operationFailures), ...Object.values(routingWarningKinds), failedOperation];
    expect(texts.filter((text) => !(text in english))).toEqual([]);
    expect(texts.filter((text) => !(text in traditional))).toEqual([]);
  });

  it("splits a coded refusal as the Rust side does", () => {
    expect(splitRefusal("error.name_required: enter a name")).toEqual({ code: "error.name_required", detail: "enter a name" });
    expect(splitRefusal("subscription.core.clash_yaml_unsupported: clash proxy type must be ss")).toEqual({
      code: "subscription.core.clash_yaml_unsupported",
      detail: "clash proxy type must be ss",
    });
    expect(splitRefusal("Disconnect first: the engine runs")).toEqual({ detail: "Disconnect first: the engine runs" });
    expect(splitRefusal("error: boom")).toEqual({ detail: "error: boom" });
  });

  it("never shows a backend's English as the headline", () => {
    // A known code: the window's words, with the English as the detail.
    expect(errorText("error.disconnect_first: Disconnect before changing the routing mode.", "routing")).toEqual({
      text: "先断开连接，再更改这项设置",
      detail: "Disconnect before changing the routing mode.",
    });
    setLanguage("en");
    expect(errorText("error.name_required: enter a name").text).toBe("Enter a name.");
    setLanguage("zh-Hant");
    expect(errorText("error.name_required: enter a name").text).toBe("請輸入名稱");
    setLanguage("zh");
    // A plain refusal: what failed, with the backend's words as the detail.
    expect(errorText("desktop core lock failed", "connect")).toEqual({ text: "连接失败", detail: "desktop core lock failed" });
    expect(errorText("something new", "an operation with no headline")).toEqual({ text: "操作没有完成", detail: "something new" });
    expect(errorText("something new")).toEqual({ text: "操作没有完成", detail: "something new" });
    // A kernel code this window does not know: also only the detail.
    expect(errorText("future.kernel.code: words", "node").text).toBe("节点操作失败");
    // An error this window made is in its language already.
    expect(errorText(new Error("端口要在 1024 到 65535 之间。"), "phone")).toEqual({ text: "端口要在 1024 到 65535 之间。" });
    // A browser error is not.
    expect(errorText(new TypeError("Failed to fetch"), "plugin")).toEqual({ text: "插件操作失败", detail: "Failed to fetch" });
  });

  it("says why a subscription failed by its reason code", () => {
    expect(errorText("error.subscription_failed: Subscription refresh failed (windows.subscription.fetch_timeout).", "subscription").text).toBe(
      "下载订阅超时 (windows.subscription.fetch_timeout)",
    );
    expect(
      errorText("error.subscription_failed: Subscription refresh failed (subscription.core.clash_yaml_unsupported).", "subscription").text,
    ).toBe("订阅里有本应用不支持的节点类型或写法 (subscription.core.clash_yaml_unsupported)");
    // Linux reads the kernel's refusal as it is.
    expect(errorText("subscription.core.vmess_link_invalid: vmess link is not valid", "subscription")).toEqual({
      text: "订阅里有写法不正确的节点 (subscription.core.vmess_link_invalid)",
      detail: "vmess link is not valid",
    });
  });

  it("says why a subscription's last update failed on its row, on Windows and on Linux", () => {
    // Windows: the reason code beside the English.
    expect(subscriptionFailureLine("windows.subscription.fetch_timeout", "The download timed out.")).toBe("下载订阅超时 (windows.subscription.fetch_timeout)");
    // Linux: no reason code, only the kernel's coded refusal; never its English or code alone.
    expect(subscriptionFailureLine(undefined, "subscription.core.vmess_link_invalid: vmess link is not valid")).toBe(
      "订阅里有写法不正确的节点 (subscription.core.vmess_link_invalid)",
    );
    expect(subscriptionFailureLine(null, "subscription.core.clash_yaml_unsupported: clash proxy type must be ss")).toBe(
      "订阅里有本应用不支持的节点类型或写法 (subscription.core.clash_yaml_unsupported)",
    );
    // A client refusal is worded by its own code; a plain one gets the operation's headline.
    expect(subscriptionFailureLine(undefined, "error.no_nodes: no nodes found")).toBe("订阅里没有找到节点（no nodes found）");
    expect(subscriptionFailureLine(undefined, "connection reset")).toBe("订阅操作失败（connection reset）");
  });

  it("puts the detail in brackets for a line in place", () => {
    expect(errorLine("error.vpn_off: the VPN is not on; connect again")).toBe("VPN 没有打开，请重新连接（the VPN is not on; connect again）");
    expect(errorLine(new Error("已在本窗口说明"))).toBe("已在本窗口说明");
    setLanguage("en");
    expect(errorLine("boom", "restart")).toBe("Restarting the core failed (boom)");
  });

  it("names the kind of each line the rules import left out", () => {
    expect(routingWarningText("config.core.routing_import.group_skipped: policy group Chain was skipped: type relay is unsupported")).toEqual({
      text: "跳过了策略组",
      detail: "policy group Chain was skipped: type relay is unsupported",
    });
    // An older backend's line is shown as it is.
    expect(routingWarningText("4 IP-ASN rules were not imported")).toEqual({ text: "4 IP-ASN rules were not imported" });
  });
});
