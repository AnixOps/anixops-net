import { describe, expect, it } from "vitest";
import type { NativeGroupSummary, NodeSummary } from "./bridge";
import { findLinkMember, findLinkNode, linkMustWait, LinkReadiness, maxImportLength, nextAppLink, parseAppLink } from "./app-link";

describe("anixops:// links", () => {
  it("reads the actions in all three spellings", () => {
    expect(parseAppLink("anixops://connect")).toEqual({ action: "connect" });
    expect(parseAppLink("anixops:///disconnect")).toEqual({ action: "disconnect" });
    expect(parseAppLink("anixops://x-callback-url/toggle")).toEqual({ action: "toggle" });
    expect(parseAppLink("ANIXOPS://Start")).toEqual({ action: "connect" });
  });

  it("reads modes, selections and imports", () => {
    expect(parseAppLink("anixops://mode?set=Rule")).toEqual({ action: "mode", mode: "rule" });
    expect(parseAppLink("anixops://mode?set=global")).toEqual({ action: "mode", mode: "global" });
    expect(parseAppLink("anixops://mode?set=direct")).toEqual({ action: "mode", mode: "direct" });
    // No preset list modes in public builds (docs/no-presets.md).
    expect(parseAppLink("anixops://mode?set=bypass_china")).toEqual({ error: "这个客户端没有该分流模式", detail: "bypass_china" }); // neutral-terms: allow (value saved by an earlier version)
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

  describe("links that arrive before the window is ready", () => {
    const starting: LinkReadiness = { loaded: false, modeChosen: false, busy: false, phase: "loading" };
    const ready: LinkReadiness = { loaded: true, modeChosen: true, busy: false, phase: "off" };

    it("keeps a cold start select until the nodes are loaded, then finds the node", () => {
      const queue = ["anixops://select?node=%E9%A6%99%E6%B8%AF01"];
      // Before the first snapshot the node list is empty: the link waits instead of reporting a missing node.
      expect(nextAppLink(queue, starting)).toBeUndefined();
      expect(queue).toHaveLength(1);
      const link = nextAppLink(queue, ready);
      expect(link).toEqual({ action: "select", node: "香港01" });
      expect(queue).toHaveLength(0);
      const nodes: NodeSummary[] = [{ id: "7", label: "香港01", protocol: "ss", selected: false }];
      expect(link && "node" in link ? findLinkNode(nodes, link.node)?.id : undefined).toBe("7");
    });

    it("runs a toggle that arrived while the state was loading once it is known", () => {
      const queue = ["anixops://toggle"];
      expect(nextAppLink(queue, { ...ready, phase: "loading" })).toBeUndefined();
      expect(nextAppLink(queue, { ...ready, phase: "connecting" })).toBeUndefined();
      expect(nextAppLink(queue, { ...ready, phase: "disconnecting" })).toBeUndefined();
      expect(nextAppLink(queue, { ...ready, busy: true })).toBeUndefined();
      expect(queue).toEqual(["anixops://toggle"]);
      expect(nextAppLink(queue, { ...ready, phase: "on" })).toEqual({ action: "toggle" });
      expect(queue).toEqual([]);
    });

    it("does not start a second connection while one is being made", () => {
      expect(linkMustWait({ action: "connect" }, { ...ready, phase: "connecting" })).toBe(true);
      expect(linkMustWait({ action: "connect" }, { ...ready, busy: true })).toBe(true);
      expect(linkMustWait({ action: "connect" }, { ...ready, phase: "failed" })).toBe(false);
      expect(linkMustWait({ action: "disconnect" }, { ...ready, phase: "on" })).toBe(false);
    });

    it("keeps an import until the first-launch mode question is answered", () => {
      const queue = ["anixops://import?url=https%3A%2F%2Fa.example%2Fsub"];
      expect(nextAppLink(queue, { ...ready, modeChosen: false })).toBeUndefined();
      expect(queue).toHaveLength(1);
      // An import only fills in the form, so a running operation does not hold it back.
      expect(nextAppLink(queue, { ...ready, busy: true })).toEqual({ action: "import", url: "https://a.example/sub" });
    });

    it("holds operations back one at a time and keeps the order", () => {
      const queue = ["anixops://mode?set=rule", "anixops://select?node=HK"];
      expect(nextAppLink(queue, ready)).toEqual({ action: "mode", mode: "rule" });
      // The mode change is running: the selection waits behind it.
      expect(nextAppLink(queue, { ...ready, busy: true })).toBeUndefined();
      expect(nextAppLink(queue, ready)).toEqual({ action: "select", node: "HK" });
      expect(nextAppLink(queue, ready)).toBeUndefined();
    });

    it("tells a refused link only once the window can show it", () => {
      const queue = ["anixops://reboot"];
      expect(nextAppLink(queue, starting)).toBeUndefined();
      expect(nextAppLink(queue, { ...ready, busy: true })).toEqual({ error: "不认识的操作", detail: "reboot" });
    });
  });
});
