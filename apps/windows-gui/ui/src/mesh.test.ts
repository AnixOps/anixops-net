import { afterEach, describe, expect, it } from "vitest";
import { setLanguage } from "./i18n";
import {
  advertisedCidr,
  currentPaths,
  routingPaths,
  connectionLabel,
  forwardingLabel,
  listenText,
  localDnsNotices,
  draftConfig,
  draftProblem,
  emptyDraft,
  joinedState,
  noticeDetail,
  pathFallbackText,
  pathStateText,
  noticeText,
  parsePeer,
  refusalOf,
  regionName,
  securityLabel,
  validAddress,
} from "./mesh";

const filled = { ...emptyDraft, name: "office", secret: "s3cret", peers: "203.0.113.10:11010\nudp://[2001:db8::1]:11011" };

describe("the 添加网络 form", () => {
  it("names the first problem, the same rules as the phones'", () => {
    expect(draftProblem(emptyDraft)).toEqual({ kind: "name" });
    expect(draftProblem({ ...filled, name: "a/b" })).toEqual({ kind: "name" });
    expect(draftProblem({ ...filled, name: "x".repeat(65) })).toEqual({ kind: "name" });
    expect(draftProblem({ ...filled, secret: "" })).toEqual({ kind: "secret" });
    expect(draftProblem({ ...filled, peers: " \n " })).toEqual({ kind: "peers" });
    expect(draftProblem({ ...filled, peers: "host:70000" })).toEqual({ kind: "peer", line: "host:70000" });
    expect(draftProblem({ ...filled, address: "10.0.0.1/31" })).toEqual({ kind: "address" });
    expect(draftProblem({ ...filled, port: "0" })).toEqual({ kind: "port" });
    expect(draftProblem(filled, ["office"])).toEqual({ kind: "duplicate" });
    expect(draftProblem(filled)).toBeNull();
  });

  it("builds a full node's configuration with its listening port", () => {
    const config = draftConfig({ ...filled, name: " office ", address: "10.144.144.5/24", port: "11012", forwardSecrecy: false });
    expect(config).toEqual({
      config_version: 1,
      network: { name: "office", secret: "s3cret", kind: "legacy", secure_mode: false },
      node: {
        role: "full",
        address: "10.144.144.5/24",
        listen: [
          { transport: "tcp", port: 11012 },
          { transport: "udp", port: 11012 },
        ],
      },
      peers: [
        { server: "203.0.113.10", port: 11010, transport: "tcp" },
        { server: "2001:db8::1", port: 11011, transport: "udp" },
      ],
    });
    const plain = draftConfig(filled) as { node: Record<string, unknown> };
    expect(plain.node.address).toBeUndefined();
    expect(plain.node.listen).toEqual([{ transport: "tcp", port: 11010 }, { transport: "udp", port: 11010 }]);
    expect(draftConfig({ ...filled, secret: "" })).toBeNull();
  });

  it("reads peer lines and virtual addresses", () => {
    expect(parsePeer("peer.example.com")).toEqual({ server: "peer.example.com", port: 11010, transport: "tcp" });
    expect(parsePeer("TCP://10.0.0.1:1")).toEqual({ server: "10.0.0.1", port: 1, transport: "tcp" });
    expect(parsePeer("[::1]")).toEqual({ server: "::1", port: 11010, transport: "tcp" });
    expect(parsePeer("[not-v6]:1")).toBeNull();
    expect(parsePeer("a:b:c")).toBeNull();
    expect(parsePeer("bad host")).toBeNull();
    expect(validAddress("10.144.144.5/24")).toBe(true);
    expect(validAddress("10.144.144.05/24")).toBe(false);
    expect(validAddress("10.144.144.5")).toBe(false);
    expect(validAddress("10.144.144.5/7")).toBe(false);
  });
});

describe("words", () => {
  afterEach(() => setLanguage("zh"));

  it("words states, security, connections and notices", () => {
    expect(joinedState("partial")).toBe(true);
    expect(joinedState("failed")).toBe(false);
    expect(securityLabel("standard")).toBe("未启用前向保密");
    expect(securityLabel(undefined)).toBeNull();
    expect(connectionLabel("relay", 2)).toBe("中转 2 跳");
    expect(noticeText({ id: "mesh.notice.partial", key: "partial", values: [2, ["a", "b"]] })).toBe("2 台成员不可达：a、b。");
    const port = { id: "mesh.notice.failure", key: "failure:port_in_use", values: ["11010"] };
    expect(noticeText(port)).toBe("无法启动组网：监听端口已被占用。");
    expect(noticeDetail(port)).toBe("11010");
    const noForward = { id: "mesh.notice.path_unsupported", key: "fallback:p:m", values: ["tyo-border", "192.168.30.0/24"] };
    expect(noticeText(noForward)).toBe("自定义路径已回落：tyo-border 不支持按此路径转发。192.168.30.0/24 使用默认路径。");
    expect(noticeText({ id: "mesh.notice.path_hop_limit", key: "fallback:p:m", values: ["192.168.30.0/24"] })).toBe("自定义路径已回落：跳数超出上限。192.168.30.0/24 使用默认路径。");
    const path = { id: "p", targetKind: "subnet", target: "192.168.30.0/24", targetLabel: "192.168.30.0/24", state: "fallback", unreachable: "tyo-border" };
    expect(pathFallbackText({ ...path, reason: "not_relay" })).toBe("tyo-border 不支持按此路径转发");
    expect(pathFallbackText({ ...path, reason: "hop_limit" })).toBe("跳数超出上限");
    expect(pathFallbackText({ ...path, reason: "unreachable" })).toBe("tyo-border 不可达");
    expect(pathFallbackText({ ...path, state: "pending" })).toBeNull();
    expect(pathFallbackText({ ...path, reason: "no_echo" })).toBe("tyo-border 探测无响应");
    expect(pathFallbackText({ ...path, reason: "pending_timeout" })).toBe("tyo-border 确认超时");
    expect(noticeText({ id: "mesh.notice.path_pending_timeout", key: "fallback:p:m", values: ["tyo-border", "192.168.30.0/24"] })).toBe("自定义路径已回落：tyo-border 确认超时。192.168.30.0/24 使用默认路径。");
    expect(pathFallbackText({ ...path, reason: "later_reason" })).toBe("tyo-border 不可达");
    expect(pathFallbackText({ ...path, state: "first_hop_only", reason: "not_relay" })).toBe("tyo-border 不支持按此路径转发");
    expect(pathFallbackText({ ...path, state: "first_hop_only" })).toBeNull();
    expect(pathStateText("pending")).toBe("待确认");
    const refused = { id: "mesh.notice.peer_not_native", key: "failure:peer_not_native:node-7f3a", values: ["lon-host"] };
    expect(noticeText(refused)).toBe("已拒绝对端连接：对端非本系统节点。");
    expect(noticeDetail(refused)).toBe("lon-host");
    setLanguage("en");
    expect(noticeText({ id: "mesh.notice.partial", key: "partial", values: [2, ["a", "b"]] })).toBe("2 members unreachable: a, b.");
  });

  it("words what keeps the internal domain names from resolving on this device", () => {
    expect(localDnsNotices({ network: { state: "running" } })).toEqual([]);
    expect(localDnsNotices(null)).toEqual([]);
    const notices = localDnsNotices({
      network: {
        local_dns: [
          { code: "write_failed", detail: null },
          { code: "policy_override", detail: null },
          { code: "suffix_refused", detail: "com, local" },
          { code: "later_code", detail: null },
        ],
      },
    });
    expect(notices.map((notice) => notice.key)).toEqual(["local_dns:write_failed", "local_dns:policy_override", "local_dns:suffix_refused"]);
    expect(notices.map(noticeText)).toEqual([
      "内网域名暂不可用：无法写入名称解析规则，稍后自动重试。",
      "内网域名不可用：本机的名称解析规则由组策略管理，本地规则不生效。",
      "已忽略内网域名后缀 com, local：后缀须位于 internal 之下。",
    ]);
    expect(notices.map(noticeDetail)).toEqual([null, null, null]);
    setLanguage("en");
    expect(noticeText(notices[2])).toBe("Internal domain suffixes ignored: com, local. A suffix must be under internal.");
  });

  it("reads refusals and region names", () => {
    expect(refusalOf({ ok: false, error: { code: "secret_mismatch", detail: "x" } })).toEqual({ code: "secret_mismatch", detail: "x" });
    expect(refusalOf({ ok: false })).toEqual({ code: "other", detail: undefined });
    expect(refusalOf({ ok: true })).toBeNull();
    expect(regionName({ regions: [{ code: "eu", name: "欧洲" }] }, "eu")).toBe("欧洲");
    expect(regionName({}, "ap")).toBe("ap");
  });
});

describe("this device", () => {
  it("words how the node runs and reads either form of an advertised subnet", () => {
    expect(listenText([{ transport: "tcp", port: 11010 }, { transport: "udp", port: 11010 }])).toBe("TCP 11010 · UDP 11010");
    expect(listenText(undefined)).toBe("");
    expect(forwardingLabel("on")).toBe("已开启");
    expect(forwardingLabel("not_needed")).toBe("无需开启");
    expect(forwardingLabel(undefined)).toBeNull();
    expect(advertisedCidr("192.168.1.0/24")).toBe("192.168.1.0/24");
    expect(advertisedCidr({ cidr: "10.0.0.0/8" })).toBe("10.0.0.0/8");
  });
});

describe("custom paths", () => {
  const status = {
    self: { id: "me", address: "10.144.0.2/16" },
    members: [
      { id: "a", address: "10.144.0.1/16" },
      { id: "b", address: "10.144.1.1/16" },
      { id: "c" },
    ],
    paths: [
      { id: "p1", target: { subnet: "172.20.0.0/16" }, via: ["a", "b"], state: "active" },
      { id: "p2", target: { peer: "b" }, via: ["a"], state: "fallback" },
    ],
  };

  it("reads the snapshot's paths by member id", () => {
    expect(currentPaths(status)).toEqual([
      { id: "p1", kind: "subnet", target: "172.20.0.0/16", via: ["a", "b"] },
      { id: "p2", kind: "member", target: "b", via: ["a"] },
    ]);
  });

  it("sends the whole list by virtual address, without one removed or with one added", () => {
    expect(routingPaths(status, { remove: "p1" })).toEqual([{ to_peer: "10.144.1.1", via: ["10.144.0.1"] }]);
    expect(routingPaths(status, { add: { kind: "subnet", target: "192.168.9.0/24", via: ["b"] } })).toEqual([
      { to_subnet: "172.20.0.0/16", via: ["10.144.0.1", "10.144.1.1"] },
      { to_peer: "10.144.1.1", via: ["10.144.0.1"] },
      { to_subnet: "192.168.9.0/24", via: ["10.144.1.1"] },
    ]);
  });

  it("replaces a path to the same target and refuses what cannot be sent", () => {
    expect(routingPaths(status, { add: { kind: "member", target: "b", via: ["me"] } })).toEqual([
      { to_subnet: "172.20.0.0/16", via: ["10.144.0.1", "10.144.1.1"] },
      { to_peer: "10.144.1.1", via: ["10.144.0.2"] },
    ]);
    expect(routingPaths(status, { add: { kind: "member", target: "c", via: ["a"] } })).toBeNull();
    expect(routingPaths(status, { add: { kind: "subnet", target: "10.9.0.0/16", via: [] } })).toBeNull();
    expect(routingPaths(status, { add: { kind: "subnet", target: "10.9.0.0/16", via: ["a", "a", "a", "a", "a", "a", "a"] } })).toBeNull();
  });
});
