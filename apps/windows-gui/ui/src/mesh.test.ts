import { afterEach, describe, expect, it } from "vitest";
import { setLanguage } from "./i18n";
import {
  connectionLabel,
  draftConfig,
  draftProblem,
  emptyDraft,
  joinedState,
  noticeDetail,
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
    setLanguage("en");
    expect(noticeText({ id: "mesh.notice.partial", key: "partial", values: [2, ["a", "b"]] })).toBe("2 members unreachable: a, b.");
  });

  it("reads refusals and region names", () => {
    expect(refusalOf({ ok: false, error: { code: "secret_mismatch", detail: "x" } })).toEqual({ code: "secret_mismatch", detail: "x" });
    expect(refusalOf({ ok: false })).toEqual({ code: "other", detail: undefined });
    expect(refusalOf({ ok: true })).toBeNull();
    expect(regionName({ regions: [{ code: "eu", name: "欧洲" }] }, "eu")).toBe("欧洲");
    expect(regionName({}, "ap")).toBe("ap");
  });
});
