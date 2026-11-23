import { t } from "./i18n";
import { subscriptionErrorText } from "./subscription-format";

/**
 * Refusals in the window's language. A backend refusal is a string; a coded one reads `<code>: <English>`
 * (crates/client-core/src/refusal.rs, and the kernel's own errors such as `subscription.core.clash_yaml_unsupported:
 * …`). A code this window knows is shown in its words with the English as the detail; any other refusal is shown as
 * what failed ("连接失败") with the backend's English as the detail, never as the headline (finding ux-i18n-a11y-02).
 */
export interface ErrorText {
  /** In the window's language. */
  text: string;
  /** The backend's own words, for the "Details" disclosure and a report; absent when `text` says it all. */
  detail?: string;
}

/** The words for each `error.<name>` code the backends send, in Chinese; error-text.test.ts checks every one is here. */
export const errorTexts: Record<string, string> = {
  "error.name_required": "请输入名称",
  "error.subscription_missing": "找不到这个订阅，它可能已被删除",
  "error.subscription_removed": "订阅在更新过程中被删除了",
  "error.no_supported_nodes": "订阅里没有本应用能使用的节点",
  "error.no_nodes": "订阅里没有找到节点",
  "error.node_missing": "找不到这个节点，订阅可能已经更新",
  "error.node_not_editable": "只能编辑当前订阅里的节点",
  "error.no_subscription": "还没有选择订阅",
  "error.no_policy_groups": "当前订阅没有策略组",
  "error.import_input_required": "请输入订阅链接、分享链接或文件路径",
  "error.disconnect_first": "先断开连接，再更改这项设置",
  "error.connect_first_replay": "先连接：重放的请求经正在运行的内核发出",
  "error.no_delay_answer": "没有节点响应测速，检查网络后再试",
  "error.not_updatable": "这个订阅来自文件或粘贴的链接，不能更新",
  "error.no_default_route": "没有可用的网络：先连上网络，再开启 TUN 模式",
  "error.vpn_off": "VPN 没有打开，请重新连接",
  "error.rules_rejected": "内核不接受这些规则，检查后再保存",
};

/** What failed, by the operation names `run` is given (App.tsx, pages/*.tsx); others read "操作没有完成". */
export const operationFailures: Record<string, string> = {
  connect: "连接失败",
  disconnect: "断开失败",
  restart: "重启内核失败",
  validate: "配置检查没有通过",
  routing: "切换分流模式失败",
  node: "节点操作失败",
  switch: "切换节点失败",
  policy: "切换策略组失败",
  "native selector": "切换策略组失败",
  "fastest node": "选择最快的节点失败",
  delay: "测速失败",
  subscription: "订阅操作失败",
  scan: "扫码导入失败",
  dns: "保存 DNS 设置失败",
  QUIC: "保存 QUIC 设置失败",
  TUN: "切换 TUN 模式失败",
  mitm: "切换 HTTPS 解密失败",
  plugin: "插件操作失败",
  certificate: "证书操作失败",
  "decrypt-host": "更新解密列表失败",
  mapping: "映射操作失败",
  preferences: "保存设置失败",
  "my rules": "保存规则失败",
  redirects: "保存重定向失败",
  diagnostics: "生成诊断报告失败",
};

/** The fallback headline. */
export const failedOperation = "操作没有完成";

const codePattern = /^([a-z][a-z0-9_]*(?:\.[a-z0-9_]+)+): ([\s\S]*)$/;

/** The code of a coded refusal and its English, or none for a plain one (as `refusal::split`). */
export function splitRefusal(message: string): { code?: string; detail: string } {
  const match = codePattern.exec(message);
  return match ? { code: match[1], detail: match[2] } : { detail: message };
}

/**
 * What to show for a failure. Strings are backend refusals; an `Error` made by this window (`new Error(t(…))`) is
 * already in its language and shown as it is; anything else (a browser error) gets the operation's headline.
 */
export function errorText(error: unknown, operation?: string): ErrorText {
  if (error instanceof Error && error.constructor === Error) return { text: error.message };
  const message = error instanceof Error ? error.message : String(error);
  const { code, detail } = splitRefusal(message);
  if (code && errorTexts[code]) return { text: t(errorTexts[code]), detail };
  if (code === "error.subscription_failed") {
    // "Subscription refresh failed (<reason code>)." carries the reason code, which says what went wrong.
    const reason = /\(([a-z0-9_.]+)\)/.exec(detail)?.[1];
    const text = subscriptionErrorText(reason);
    if (text) return { text };
  }
  if (code?.startsWith("subscription.")) {
    const text = subscriptionErrorText(code);
    if (text) return { text, detail };
  }
  const headline = (operation && operationFailures[operation]) || failedOperation;
  return { text: t(headline), detail: message || undefined };
}

/** One line for a failure shown in place (a card, a row): the text, then the detail in brackets. */
export function errorLine(error: unknown, operation?: string): string {
  const { text, detail } = errorText(error, operation);
  return detail ? t("{text}（{detail}）", { text, detail }) : text;
}

/** The kind of each line `routing_warnings` sends (`<kernel code>: <English>`), in Chinese. */
export const routingWarningKinds: Record<string, string> = {
  "config.core.routing_import.rule_skipped": "跳过了规则",
  "config.core.routing_import.group_skipped": "跳过了策略组",
  "config.core.routing_import.member_skipped": "跳过了策略组成员",
  "config.core.routing_import.option_ignored": "忽略了不支持的选项",
  "config.core.routing_import.invalid": "订阅规则没有导入",
  "routing_warnings.more": "其余的没有列出",
};

/** A line of `routing_warnings`: its kind in the window's language and the kernel's English as the detail. */
export function routingWarningText(line: string): ErrorText {
  const { code, detail } = splitRefusal(line);
  if (code && routingWarningKinds[code]) return { text: t(routingWarningKinds[code]), detail };
  return { text: line };
}
