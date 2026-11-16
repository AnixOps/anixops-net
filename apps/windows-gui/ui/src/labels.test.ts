import { describe, expect, it } from "vitest";
import { availableRoutingModes, connectionTitle, connectPhase, factLabel, failureReason, groupMemberLabel, protocolAbbreviation, routingLabel, sidebarDetail } from "./labels";

describe("connectPhase", () => {
  it("prefers a running connect or disconnect operation over the reported state", () => {
    expect(connectPhase("Not connected", "connect")).toBe("connecting");
    expect(connectPhase("Connected", "disconnect")).toBe("disconnecting");
  });

  it("maps the runtime's connection labels", () => {
    expect(connectPhase(undefined, null)).toBe("loading");
    expect(connectPhase("Connected", null)).toBe("on");
    expect(connectPhase("Not connected", null)).toBe("off");
    expect(connectPhase("Core error", null)).toBe("failed");
    expect(connectPhase("Connected", "routing")).toBe("on");
  });

  it("names failures specifically", () => {
    expect(connectionTitle("Configuration error", "failed")).toBe("配置有误");
    expect(connectionTitle("Connected", "on")).toBe("已连接");
  });
});

describe("failureReason", () => {
  it("gives nothing unless the connection failed", () => {
    expect(failureReason({ lastError: "boom" }, "off", "proxy")).toBeUndefined();
    expect(failureReason({ lastError: "boom" }, "on", "debug")).toBeUndefined();
  });

  it("prefers the configuration error, then the last error", () => {
    expect(failureReason({ configurationError: "bad config", lastError: "boom" }, "failed", "proxy")).toBe("bad config");
    expect(failureReason({ lastError: "boom" }, "failed", "debug")).toBe("boom");
  });

  it("points to a page the mode has when the backend gave no reason", () => {
    expect(failureReason({}, "failed", "proxy")).toBe("查看“活动”了解原因");
    // Debug mode has no Activity page: the session facts are under Requests › Connections.
    expect(failureReason(null, "failed", "debug")).toBe("查看“请求 › 连接”了解原因");
  });
});

describe("sidebarDetail", () => {
  it("shows the failure reason in the danger tone", () => {
    expect(sidebarDetail({ phase: "failed", reason: "boom", unconfigured: false })).toEqual({ text: "boom", danger: true });
  });

  it("says what to do while the switch cannot be turned on", () => {
    expect(sidebarDetail({ phase: "off", unconfigured: true })).toEqual({ text: "先导入订阅或配置文件", danger: false });
  });

  it("shows the node and routing while connected and the shortcut while off", () => {
    expect(sidebarDetail({ phase: "on", unconfigured: false, node: "HK 01", routing: "规则" }).text).toBe("HK 01 · 规则");
    expect(sidebarDetail({ phase: "off", unconfigured: false }).text).toBe("Ctrl+Shift+C");
    expect(sidebarDetail({ phase: "connecting", unconfigured: false }).text).toBe("");
  });
});

describe("labels", () => {
  it("translates known status labels and keeps unknown ones", () => {
    expect(factLabel("Running (PID 42)")).toBe("运行中");
    expect(factLabel("NotInstalled")).toBe("未安装");
    expect(factLabel("Exited (PID 7, exit 1)")).toBe("已退出");
    expect(factLabel("Something new")).toBe("Something new");
    expect(factLabel(undefined)).toBe("读取中");
  });

  it("abbreviates protocols and names routing modes", () => {
    expect(protocolAbbreviation("Shadowsocks")).toBe("SS");
    expect(protocolAbbreviation("WireGuard")).toBe("WG");
    expect(protocolAbbreviation("Custom")).toBe("Custom");
    expect(routingLabel("direct")).toBe("直连");
    expect(routingLabel(undefined)).toBe("规则");
  });

  it("offers rules and direct and no preset list modes", () => {
    expect(availableRoutingModes().map((mode) => mode.value)).toEqual(["rule", "direct"]);
    expect(routingLabel("rule")).toBe("规则");
  });

  it("labels group members by node name", () => {
    const group = { outbounds: ["Auto", "node-1"], outboundLabels: ["Auto", "Tokyo"] };
    expect(groupMemberLabel(group, "node-1")).toBe("Tokyo");
    expect(groupMemberLabel({ outbounds: ["direct"] }, "direct")).toBe("direct");
    expect(groupMemberLabel(group, undefined)).toBeUndefined();
  });
});
