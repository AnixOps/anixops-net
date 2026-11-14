import { describe, expect, it } from "vitest";
import type { NativeGroupSummary, NodeSummary } from "./bridge";
import { findLinkMember, findLinkNode, maxImportLength, parseAppLink } from "./app-link";

describe("anixops:// links", () => {
  it("reads the actions in all three spellings", () => {
    expect(parseAppLink("anixops://connect")).toEqual({ action: "connect" });
    expect(parseAppLink("anixops:///disconnect")).toEqual({ action: "disconnect" });
    expect(parseAppLink("anixops://x-callback-url/toggle")).toEqual({ action: "toggle" });
    expect(parseAppLink("ANIXOPS://Start")).toEqual({ action: "connect" });
  });

  it("reads modes, selections and imports", () => {
    expect(parseAppLink("anixops://mode?set=Rule")).toEqual({ action: "mode", mode: "rule" });
    expect(parseAppLink("anixops://mode?set=global")).toEqual({ error: "这个客户端没有该分流模式", detail: "global" });
    expect(parseAppLink("anixops://select?node=%E9%A6%99%E6%B8%AF&group=Main")).toEqual({ action: "select", node: "香港", group: "Main" });
    expect(parseAppLink("anixops://select?policy=Proxy&node=HK")).toEqual({ action: "selectMember", policy: "Proxy", member: "HK" });
    expect(parseAppLink("anixops://import?url=https%3A%2F%2Fa.example%2Fsub%3Ftoken%3Dx")).toEqual({ action: "import", url: "https://a.example/sub?token=x" });
  });

  it("refuses what it cannot run", () => {
    expect(parseAppLink("https://a.example")).toEqual({ error: "不是 anixops:// 链接" });
    expect(parseAppLink("anixops://")).toEqual({ error: "链接缺少操作" });
    expect(parseAppLink("anixops://reboot")).toEqual({ error: "不认识的操作", detail: "reboot" });
    expect(parseAppLink("anixops://select")).toEqual({ error: "链接缺少参数", detail: "node" });
    expect(parseAppLink(`anixops://import?url=${"a".repeat(maxImportLength + 1)}`)).toEqual({ error: "链接太长" });
    expect(parseAppLink("not a url")).toEqual({ error: "链接无效" });
  });

  it("finds the named node and policy member", () => {
    const nodes: NodeSummary[] = [
      { id: "1", label: "HK", protocol: "ss", selected: false, group: "Main" },
      { id: "2", label: "hk", protocol: "ss", selected: false, group: "Backup" },
    ];
    expect(findLinkNode(nodes, "HK")?.id).toBe("1");
    expect(findLinkNode(nodes, "HK", "backup")?.id).toBe("2");
    expect(findLinkNode(nodes, "2")?.id).toBe("2");
    expect(findLinkNode(nodes, "JP")).toBeUndefined();
    const groups: NativeGroupSummary[] = [{ tag: "Proxy", groupType: "select", outbounds: ["n1", "n2"], outboundLabels: ["香港", "日本"] }];
    expect(findLinkMember(groups, "proxy", "日本")).toEqual({ tag: "Proxy", outbound: "n2" });
    expect(findLinkMember(groups, "Proxy", "n1")).toEqual({ tag: "Proxy", outbound: "n1" });
    expect(findLinkMember(groups, "Proxy", "美国")).toBeUndefined();
  });
});
