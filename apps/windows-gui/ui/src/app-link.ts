import type { NativeGroupSummary, NodeSummary, RuntimeSnapshot } from "./bridge";

/**
 * What an `anixops://` link asks for, the same actions as on iOS (`AppURLCommand.swift`):
 *
 * - `anixops://connect`, `anixops://disconnect`, `anixops://toggle`
 * - `anixops://mode?set=rule|direct|bypass_china|gfw_list|return_china`
 * - `anixops://select?node=<name>[&group=<subscription>]`, or `anixops://select?policy=<group>&node=<member>`
 * - `anixops://import?url=<subscription or node link>` fills in the import form; nothing is saved until the
 *   user presses Import
 *
 * `anixops:///connect` and `anixops://x-callback-url/connect` are accepted too.
 */
export type AppLink =
  | { action: "connect" }
  | { action: "disconnect" }
  | { action: "toggle" }
  | { action: "mode"; mode: RuntimeSnapshot["routingMode"] }
  | { action: "select"; node: string; group?: string }
  | { action: "selectMember"; policy: string; member: string }
  | { action: "import"; url: string };

/** Why a link was refused, in the interface's words. */
export type AppLinkError = { error: string; detail?: string };

export const appLinkScheme = "anixops";
/** Longer imports are refused, so a page cannot hand the form megabytes of text. */
export const maxImportLength = 8 * 1024;

const modes: Record<string, RuntimeSnapshot["routingMode"]> = {
  rule: "rule",
  direct: "direct",
  bypass_china: "bypass_china",
  gfw_list: "gfw_list",
  return_china: "return_china",
};

export function parseAppLink(raw: string): AppLink | AppLinkError {
  let url: URL;
  try {
    url = new URL(raw.trim());
  } catch {
    return { error: "链接无效" };
  }
  if (url.protocol.toLowerCase() !== appLinkScheme + ":") return { error: "不是 anixops:// 链接" };
  if (raw.length > maxImportLength + 256) return { error: "链接太长" };
  const segments = url.pathname.split("/").filter(Boolean);
  const host = url.hostname.toLowerCase();
  const action = (host === "" || host === "x-callback-url" ? segments[0] ?? "" : host).toLowerCase();
  const query = new Map<string, string>();
  url.searchParams.forEach((value, name) => {
    if (!query.has(name.toLowerCase())) query.set(name.toLowerCase(), value.trim());
  });
  const value = (name: string) => query.get(name) || undefined;

  switch (action) {
    case "connect":
    case "start":
    case "on":
      return { action: "connect" };
    case "disconnect":
    case "stop":
    case "off":
      return { action: "disconnect" };
    case "toggle":
      return { action: "toggle" };
    case "mode": {
      const raw = value("set") ?? value("mode");
      if (!raw) return { error: "链接缺少参数", detail: "set" };
      const mode = modes[raw.toLowerCase()];
      return mode ? { action: "mode", mode } : { error: "这个客户端没有该分流模式", detail: raw };
    }
    case "select": {
      const node = value("node");
      if (!node) return { error: "链接缺少参数", detail: "node" };
      const policy = value("policy");
      if (policy) return { action: "selectMember", policy, member: node };
      const group = value("group");
      return group ? { action: "select", node, group } : { action: "select", node };
    }
    case "import":
    case "install-config":
    case "add": {
      const link = value("url");
      if (!link) return { error: "链接缺少参数", detail: "url" };
      if (link.length > maxImportLength) return { error: "链接太长" };
      return { action: "import", url: link };
    }
    case "":
      return { error: "链接缺少操作" };
    default:
      return { error: "不认识的操作", detail: action };
  }
}

/** The node a `select` link names: in subscription `group` when given (else any), by name, else by id. */
export function findLinkNode(nodes: NodeSummary[], name: string, group?: string): NodeSummary | undefined {
  const pool = group ? nodes.filter((node) => node.group?.toLowerCase() === group.toLowerCase()) : nodes;
  const lower = name.toLowerCase();
  return pool.find((node) => node.label === name)
    ?? pool.find((node) => node.label.toLowerCase() === lower)
    ?? pool.find((node) => node.id === name);
}

/** The policy group and member tag a `selectMember` link names, by tag or by shown name. */
export function findLinkMember(groups: NativeGroupSummary[], policy: string, member: string): { tag: string; outbound: string } | undefined {
  const lower = policy.toLowerCase();
  const group = groups.find((item) => item.tag === policy) ?? groups.find((item) => item.tag.toLowerCase() === lower);
  if (!group) return undefined;
  const index = group.outbounds.indexOf(member);
  const byLabel = group.outboundLabels?.indexOf(member) ?? -1;
  const outbound = index >= 0 ? group.outbounds[index] : byLabel >= 0 ? group.outbounds[byLabel] : undefined;
  return outbound ? { tag: group.tag, outbound } : undefined;
}
