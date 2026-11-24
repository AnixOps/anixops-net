import type { DnsPreset, RuntimeSnapshot } from "./bridge";
import { errorLine } from "./error-text";
import { t } from "./i18n";

export type ConnectPhase = "loading" | "off" | "connecting" | "on" | "disconnecting" | "failed";

/** Maps the runtime's connection label and any running operation to the phase the connect button shows. */
export function connectPhase(connection: string | undefined, busy: string | null): ConnectPhase {
  if (busy === "connect") return "connecting";
  if (busy === "disconnect") return "disconnecting";
  switch (connection) {
    case undefined:
      return "loading";
    case "Connected":
      return "on";
    case "Connecting":
      return "connecting";
    case "Disconnecting":
      return "disconnecting";
    case "Connection failed":
    case "Core error":
    case "Configuration error":
      return "failed";
    default:
      return "off";
  }
}

// Labels below stay in Chinese, the source language; the functions translate them when they are shown.
export const phaseTitles: Record<ConnectPhase, string> = {
  loading: "正在读取状态",
  off: "未连接",
  connecting: "正在连接…",
  on: "已连接",
  disconnecting: "正在断开…",
  failed: "连接出错",
};

const connectionTitles: Record<string, string> = {
  "Connection failed": "连接失败",
  "Core error": "内核出错",
  "Configuration error": "配置有误",
};

/**
 * The name of the connection switch (sidebar and overview). It names the thing switched, the same in either state:
 * a screen reader says the state from `aria-checked`, so a name that flipped to "Disconnect" would be read as
 * "Disconnect, switch, on" (finding ux-i18n-a11y-04).
 */
export function connectionSwitchLabel(): string {
  return t("连接|noun");
}

export function connectionTitle(connection: string | undefined, phase: ConnectPhase): string {
  return t((connection && connectionTitles[connection]) || phaseTitles[phase]);
}

// The runtime state line the backends report (Windows: what the engine and the proxy are doing; Linux: the
// connection state itself). Fixed sentences, so the window words them.
const connectionLabels: Record<string, string> = {
  "The embedded engine is running and managing the current-user proxy.": "内核正在运行，并管理当前用户的系统代理",
  "The embedded engine is running; the current-user proxy is not applied.": "内核正在运行，当前用户的系统代理未应用",
  "Connect to start the embedded engine.": "连接后启动内核",
  Connected: "已连接",
  "Not connected": "未连接",
  ...connectionTitles,
};

/** The activity page's runtime state, in the window's language; anything unknown is shown as is. */
export function connectionLabelText(label: string | undefined): string {
  if (!label) return t("读取中");
  return connectionLabels[label] ? t(connectionLabels[label]) : label;
}

const refreshResults: Record<string, string> = { success: "成功", failed: "失败", running: "进行中" };

/** The last automatic subscription update's result (`success`, `failed`, `running`), in the window's language. */
export function refreshResultText(result: string | undefined): string {
  if (!result) return t("等待中");
  return refreshResults[result] ? t(refreshResults[result]) : result;
}

/**
 * Why the connection failed, in the window's language (a coded reason in its own words, otherwise "连接失败" with the
 * backend's words in brackets, error-text.ts), or where to read more when it gave none. Only while it has failed. The overview shows it under the state; the sidebar shows it on every page, because debug mode has no
 * overview and a failure must never go unexplained (docs/ui-ux.md, "提示在所有页面可见").
 */
export function failureReason(
  snapshot: Pick<RuntimeSnapshot, "configurationError" | "lastError"> | null | undefined,
  phase: ConnectPhase,
  mode: "proxy" | "debug",
): string | undefined {
  if (phase !== "failed") return undefined;
  const reason = snapshot?.configurationError ?? snapshot?.lastError;
  if (reason) return errorLine(reason, "connect");
  return mode === "debug" ? t("查看“请求 › 连接”了解原因") : t("查看“活动”了解原因");
}

/**
 * The small line under the sidebar's connection state: the reason after a failure, what to do while there is nothing
 * to connect with (the switch is disabled then, and a disabled switch alone does not say why), the node and routing
 * while connected, and the shortcut while off.
 */
export function sidebarDetail(input: {
  phase: ConnectPhase;
  reason?: string;
  unconfigured: boolean;
  node?: string;
  routing?: string;
}): { text: string; danger: boolean } {
  if (input.phase === "failed" && input.reason) return { text: input.reason, danger: true };
  if (input.unconfigured && input.phase !== "on") return { text: t("先导入订阅或配置文件"), danger: false };
  if (input.phase === "on") return { text: [input.node, input.routing].filter(Boolean).join(" · "), danger: false };
  if (input.phase === "off") return { text: "Ctrl+Shift+C", danger: false };
  return { text: "", danger: false };
}

export const routingModes: Array<{ value: RuntimeSnapshot["routingMode"]; label: string; hint: string }> = [
  { value: "rule", label: "规则", hint: "按订阅规则和自定义规则分流，其余走代理" },
  { value: "global", label: "全局", hint: "所有流量都走当前节点，不看规则" },
  { value: "direct", label: "直连", hint: "所有流量直连，不经过节点" },
];

/** The modes a profile can use. Public builds ship no preset list modes (docs/no-presets.md). */
export function availableRoutingModes() {
  return routingModes;
}

/** The name to show for a group member. */
export function groupMemberLabel(group: { outbounds: string[]; outboundLabels?: string[] }, member: string | undefined): string | undefined {
  if (member === undefined) return undefined;
  return group.outboundLabels?.[group.outbounds.indexOf(member)] ?? member;
}

export function routingLabel(mode: RuntimeSnapshot["routingMode"] | undefined): string {
  return t(routingModes.find((item) => item.value === mode)?.label ?? routingModes[0].label);
}

const dnsPresetLabels: Record<DnsPreset, string> = { system: "系统 DNS", encrypted: "加密 DNS", custom: "自定义 DNS" };

/** The short name of a DNS choice; backends that do not report one use the system's. */
export function dnsPresetLabel(preset: DnsPreset | undefined): string {
  return t(dnsPresetLabels[preset ?? "system"]);
}

const exactFacts: Record<string, string> = {
  "Not used": "未使用",
  Running: "运行中",
  Stopped: "已停止",
  Starting: "正在启动",
  StartPending: "正在启动",
  StopPending: "正在停止",
  Paused: "已暂停",
  NotInstalled: "未安装",
  Unknown: "未知",
  Enabled: "已开启",
  Disabled: "已关闭",
  Unavailable: "不可用",
  "Not configured": "未配置",
};

/** Translates the short status labels the Rust side reports; anything unknown is shown as is. */
export function factLabel(label: string | undefined): string {
  if (!label) return t("读取中");
  if (exactFacts[label]) return t(exactFacts[label]);
  if (label.startsWith("Running (")) return t("运行中");
  if (label.startsWith("Embedded engine running")) return t("运行中");
  if (label.startsWith("Stopped: ")) return t("已停止");
  if (label.startsWith("Exited")) return t("已退出");
  if (label.startsWith("Status unavailable")) return t("状态不可用");
  return label;
}

const protocolAbbreviations: Record<string, string> = {
  anx: "ANX",
  shadowsocks: "SS",
  vless: "VLESS",
  vmess: "VMess",
  trojan: "Trojan",
  wireguard: "WG",
  hysteria2: "HY2",
  tuic: "TUIC",
};

export function protocolAbbreviation(protocol: string): string {
  return protocolAbbreviations[protocol.toLowerCase()] ?? protocol;
}
