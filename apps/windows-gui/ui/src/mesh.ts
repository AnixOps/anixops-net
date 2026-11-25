import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";

// 组网 (mesh) on the desktop (docs/mesh-ui.md, docs/architecture/mesh-client.md): the engine's `mesh_status` snapshot,
// the shared views of it (`view.mesh_*`, crates/client-view/src/mesh.rs) and the saved networks (`mesh_networks`).
// Grouping, folding, searching and the notices are decided by the shared views; this module types their answers,
// words them and checks the 添加网络 form. The Android and Apple clients read the same answers.

export type MeshStatus = Record<string, unknown>;

/** `view.mesh_entry`: the sidebar entry and the status bar. */
export interface MeshEntry {
  /** False while the engine has no mesh module: the entry is hidden. */
  available: boolean;
  network?: string | null;
  /** `joined`, `joining`, `partial`, `not_joined` or `failed`. */
  state: string;
  /** `forward_secrecy`, `standard` or `none`. */
  security?: string | null;
  online: number;
  total: number;
  attention: boolean;
}

export interface MeshMembers {
  groupBy: string;
  groups: MeshGroup[];
  total: number;
  matched: number;
}

export interface MeshGroup {
  key?: string | null;
  count: number;
  matched: number;
  open: boolean;
  worst?: { state: string; count: number } | null;
  link?: { latencyMs?: number | null; via?: string | null; state: string } | null;
  rows: MeshMemberRow[];
}

export interface MeshMemberRow {
  id: string;
  name: string;
  self?: boolean;
  role?: string | null;
  region?: string | null;
  state: string;
  address?: string | null;
  domain?: string | null;
  regionDomain?: string | null;
  connection: string;
  hops?: number;
  transport?: string | null;
  latencyMs?: number | null;
  tone?: string | null;
  contains?: { noun: string; count: number } | null;
  children?: MeshChild[];
  visibleChildren?: number;
  open?: boolean;
  marks?: string[];
}

export interface MeshChild {
  kind: string;
  name: string;
  address?: string | null;
  domain?: string | null;
  match?: boolean;
}

export interface MeshSubnets {
  rows: MeshSubnetRow[];
  refused: number;
}

export interface MeshSubnetRow {
  cidr: string;
  region?: string | null;
  regionName?: string | null;
  advertiser?: string | null;
  advertisers?: Array<{ id: string; name: string; inUse: boolean }>;
  path?: string[];
  latencyMs?: number | null;
  tone?: string | null;
  state: string;
  route: string;
  overlaps?: string[];
  marks?: string[];
}

export interface MeshPaths {
  policy: string;
  links?: MeshLink[] | null;
  custom?: MeshCustomPath[] | null;
}

export interface MeshLink {
  from: string;
  fromName: string;
  to: string;
  toName: string;
  via?: string | null;
  viaName?: string | null;
  borders?: string[];
  latencyMs?: number | null;
  tone?: string | null;
  loss?: number | null;
  state: string;
}

export interface MeshCustomPath {
  id: string;
  targetKind: string;
  target: string;
  targetLabel: string;
  advertiser?: string | null;
  via?: string[];
  state: string;
  reason?: string | null;
  unreachable?: string | null;
}

export interface MeshNotice {
  id: string;
  key: string;
  values?: unknown[];
  action?: string | null;
  notify?: boolean;
}

/** The parts of the snapshot the page shows as they are. */
export interface MeshHead {
  network?: {
    id: string;
    name?: string | null;
    state: string;
    security?: string | null;
    joining?: { stage?: string | null; peersConnected?: number | null } | null;
    routingPolicy?: string;
  } | null;
  self?: {
    id: string;
    name: string;
    address?: string | null;
    domain?: string | null;
    region?: string | null;
    regionBasis?: { border?: string | null; latencyMs?: number | null } | null;
    regionState?: string;
    /** Kernel 0.4, full nodes: relaying for other members, host mode, the advertised and suggested subnets. */
    relay?: boolean;
    hostMode?: boolean;
    listen?: Array<{ transport: string; port: number }>;
    advertised?: Array<string | { cidr: string }>;
    suggestedSubnets?: Array<{ cidr: string; interface?: string | null; public?: boolean }>;
    /** `on`, `off` or `not_needed`. */
    ipForwarding?: string | null;
  } | null;
  regions?: Array<{ code: string; name: string; local?: boolean }>;
}

/** A saved network as `mesh_networks` lists it: never its secret. */
export interface MeshSavedNetwork {
  name: string;
  joined: boolean;
  security: string;
  peers: number;
  address?: string;
}

/** Why a mesh command did not take effect. */
export interface MeshRefusal {
  code: string;
  detail?: string;
}

export interface MeshView {
  status: MeshStatus;
  head: MeshHead;
  entry: MeshEntry;
  members: MeshMembers;
  subnets: MeshSubnets;
  paths: MeshPaths;
  notices: MeshNotice[];
}

export interface MemberQuery {
  groupBy: string;
  filter: string;
  sort: string;
  query: string;
}

/** The refusal in an engine answer (`{"ok":false,"error":{code,detail}}`), or null. */
export function refusalOf(answer: unknown): MeshRefusal | null {
  if (!answer || typeof answer !== "object") return null;
  const object = answer as { ok?: unknown; error?: { code?: unknown; detail?: unknown } };
  if (object.ok !== false) return null;
  return {
    code: typeof object.error?.code === "string" ? object.error.code : "other",
    detail: typeof object.error?.detail === "string" ? object.error.detail : undefined,
  };
}

const control = (request: Record<string, unknown>) => invoke<unknown>("runtime_control", { request });

async function command(name: string, args: Record<string, unknown>): Promise<MeshRefusal | null> {
  try {
    return refusalOf(await invoke<unknown>(name, args));
  } catch (error) {
    return { code: "other", detail: String(error) };
  }
}

export const meshApi = {
  /**
   * The page in one go; null while the engine does not answer `mesh_status` (not running and no saved answer, or no
   * mesh module): the entry is then hidden.
   */
  async view(query: MemberQuery): Promise<MeshView | null> {
    let status: unknown;
    try {
      status = await control({ cmd: "mesh_status" });
    } catch {
      return null;
    }
    if (!status || typeof status !== "object" || refusalOf(status)) return null;
    const entry = await invoke<MeshEntry>("view.mesh_entry", { status });
    if (!entry.available) return null;
    const [members, subnets, paths, notices] = await Promise.all([
      invoke<MeshMembers>("view.mesh_members", { status, ...query, devices: [] }),
      invoke<MeshSubnets>("view.mesh_subnets", { status, query: query.query }),
      invoke<MeshPaths>("view.mesh_paths", { status }),
      invoke<MeshNotice[]>("view.mesh_notices", { status }),
    ]);
    return { status: status as MeshStatus, head: status as MeshHead, entry, members, subnets, paths, notices };
  },
  networks: () => invoke<MeshSavedNetwork[]>("mesh_networks"),
  save: (config: Record<string, unknown>) => command("save_mesh_network", { config }),
  setJoined: (network: string, joined: boolean) => command("set_mesh_joined", { network, joined }),
  remove: (network: string) => command("remove_mesh_network", { network }),
  setSubnetRoute: (network: string, cidr: string, route: "local" | "mesh") =>
    engine({ cmd: "mesh_set_subnet_route", network, cidr, route }),
  /** Kernel 0.4, full nodes (docs/architecture/mesh-client.md, 控制). */
  setRelay: (network: string, enabled: boolean) => engine({ cmd: "mesh_set_relay", network, enabled }),
  setHostMode: (network: string, enabled: boolean) => engine({ cmd: "mesh_set_host_mode", network, enabled }),
  advertise: (network: string, cidr: string) => engine({ cmd: "mesh_advertise", network, cidr }),
  withdraw: (network: string, cidr: string) => engine({ cmd: "mesh_withdraw", network, cidr }),
  requestRegion: (network: string, region: string) => engine({ cmd: "mesh_request_region", network, region }),
  /** Kernel 0.5. */
  setPolicy: (network: string, policy: "latency" | "hops") => engine({ cmd: "mesh_set_policy", network, policy }),
  setPaths: (network: string, paths: Record<string, unknown>[]) => engine({ cmd: "mesh_set_paths", network, paths }),
  /** A one-time join credential (个人电脑用入网凭据加入). */
  joinCredential: (credential: string) => engine({ cmd: "mesh_join_credential", credential: credential.trim() }),
};

/** A mesh command for the engine; its refusal, or null once it took effect. */
async function engine(request: Record<string, unknown>): Promise<MeshRefusal | null> {
  try {
    return refusalOf(await control(request));
  } catch (error) {
    return { code: "other", detail: String(error) };
  }
}

// Custom paths (docs/mesh-ui.md, 路径; kernel 0.5). The snapshot names members by id; `mesh_set_paths` takes the
// kernel's `routing.paths`, which names them by virtual address. The whole list is sent each time.

/** A custom path as the editor holds it: a member id or a subnet, and the members to pass, by id, in order. */
export interface PathDraft {
  kind: "member" | "subnet";
  target: string;
  via: string[];
}

/** The most waypoints a path may name (mesh-module.md, `routing.paths`). */
export const maxWaypoints = 6;

type Json = Record<string, unknown>;
const record = (value: unknown): Json => (value && typeof value === "object" && !Array.isArray(value) ? value as Json : {});
const array = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);
const bare = (address: unknown) => (typeof address === "string" ? address.split("/")[0] : "");

/** Member id to virtual address (without its prefix), this device included. */
function addresses(status: MeshStatus): Map<string, string> {
  const known = new Map<string, string>();
  const self = record(status.self);
  if (typeof self.id === "string" && bare(self.address)) known.set(self.id, bare(self.address));
  for (const member of array(status.members)) {
    const row = record(member);
    if (typeof row.id === "string" && bare(row.address)) known.set(row.id, bare(row.address));
  }
  return known;
}

/** The paths of the snapshot as the editor holds them. */
export function currentPaths(status: MeshStatus): Array<PathDraft & { id: string }> {
  return array(status.paths).map((entry) => {
    const path = record(entry);
    const target = record(path.target);
    const via = array(path.via).filter((id): id is string => typeof id === "string");
    const id = typeof path.id === "string" ? path.id : "";
    return typeof target.peer === "string"
      ? { id, kind: "member" as const, target: target.peer, via }
      : { id, kind: "subnet" as const, target: typeof target.subnet === "string" ? target.subnet : "", via };
  });
}

/**
 * `mesh_set_paths`' `paths` after removing the path `remove` (an id) and adding `add`, which replaces a path to the
 * same target. Null when a member has no virtual address in the snapshot, or `add` names no waypoint or too many.
 */
export function routingPaths(status: MeshStatus, change: { remove?: string; add?: PathDraft }): Json[] | null {
  const known = addresses(status);
  let paths: PathDraft[] = currentPaths(status).filter((path) => path.id !== change.remove);
  if (change.add) {
    const add = change.add;
    if (add.target === "" || add.via.length === 0 || add.via.length > maxWaypoints) return null;
    paths = paths.filter((path) => !(path.kind === add.kind && path.target === add.target));
    paths.push(add);
  }
  const result: Json[] = [];
  for (const path of paths) {
    const via = path.via.map((id) => known.get(id));
    if (via.some((address) => !address)) return null;
    if (path.kind === "member") {
      const address = known.get(path.target);
      if (!address) return null;
      result.push({ to_peer: address, via });
    } else {
      result.push({ to_subnet: path.target, via });
    }
  }
  return result;
}

/** The CIDR of an `advertised` entry, which the kernel may give as a string or as `{ cidr }`. */
export function advertisedCidr(entry: string | { cidr: string }): string {
  return typeof entry === "string" ? entry : entry.cidr;
}

/** The words for `self.ipForwarding`; null when the kernel does not say. */
export function forwardingLabel(state: string | null | undefined): string | null {
  switch (state) {
    case "on":
      return t("已开启");
    case "off":
      return t("未开启");
    case "not_needed":
      return t("无需开启");
    default:
      return null;
  }
}

/** `self.listen` as one line: `TCP 11010 · UDP 11010`. */
export function listenText(listen: Array<{ transport: string; port: number }> | undefined): string {
  return (listen ?? []).map((entry) => `${entry.transport.toUpperCase()} ${entry.port}`).join(" · ");
}

// The 添加网络 form (docs/mesh-ui.md, 添加网络). The desktop is a full node: it listens, on TCP and UDP, at its port.

/** The default listening port of mesh members. */
export const defaultMeshPort = 11010;

export interface MeshDraft {
  name: string;
  secret: string;
  /** One peer per line: `host:port`, `[v6]:port` or `host`, optionally with `tcp://` or `udp://` in front. */
  peers: string;
  /** The IPv4 virtual address with its prefix; empty when the network assigns one. */
  address: string;
  /** The listening port, as typed; empty for the default. */
  port: string;
  forwardSecrecy: boolean;
}

export const emptyDraft: MeshDraft = { name: "", secret: "", peers: "", address: "", port: "", forwardSecrecy: true };

export type DraftProblem =
  | { kind: "name" }
  | { kind: "secret" }
  | { kind: "peers" }
  | { kind: "peer"; line: string }
  | { kind: "address" }
  | { kind: "port" }
  | { kind: "duplicate" };

export interface MeshPeer {
  server: string;
  port: number;
  transport: "tcp" | "udp";
}

const peerLines = (peers: string) => peers.split(/\r?\n/).map((line) => line.trim()).filter((line) => line.length > 0);

/** The first thing wrong with the form, or null when it can be saved. `taken` are the saved networks' names. */
export function draftProblem(draft: MeshDraft, taken: string[] = []): DraftProblem | null {
  const name = draft.name.trim();
  if (name.length === 0 || [...name].length > 64 || name.includes("/") || /[\r\n]/.test(name)) return { kind: "name" };
  if (draft.secret.length === 0) return { kind: "secret" };
  const lines = peerLines(draft.peers);
  if (lines.length === 0) return { kind: "peers" };
  const wrong = lines.find((line) => parsePeer(line) === null);
  if (wrong !== undefined) return { kind: "peer", line: wrong };
  const address = draft.address.trim();
  if (address.length > 0 && !validAddress(address)) return { kind: "address" };
  if (draft.port.trim().length > 0 && validPort(draft.port.trim()) === null) return { kind: "port" };
  if (taken.includes(name)) return { kind: "duplicate" };
  return null;
}

/** `save_mesh_network`'s `config`, secret included; null while the form has a problem. */
export function draftConfig(draft: MeshDraft, taken: string[] = []): Record<string, unknown> | null {
  if (draftProblem(draft, taken)) return null;
  const port = validPort(draft.port.trim()) ?? defaultMeshPort;
  const address = draft.address.trim();
  return {
    config_version: 1,
    network: {
      name: draft.name.trim(),
      secret: draft.secret,
      kind: draft.forwardSecrecy ? "managed" : "legacy",
      secure_mode: draft.forwardSecrecy,
    },
    node: {
      role: "full",
      ...(address.length > 0 ? { address } : {}),
      listen: [
        { transport: "tcp", port },
        { transport: "udp", port },
      ],
    },
    peers: peerLines(draft.peers).map(parsePeer),
  };
}

/** A peer line: `[tcp://|udp://]host[:port]`, with IPv6 hosts in brackets. */
export function parsePeer(line: string): MeshPeer | null {
  let rest = line.trim();
  let transport: "tcp" | "udp" = "tcp";
  for (const scheme of ["tcp", "udp"] as const) {
    if (rest.toLowerCase().startsWith(`${scheme}://`)) {
      transport = scheme;
      rest = rest.slice(scheme.length + 3);
    }
  }
  let port = defaultMeshPort;
  let host: string;
  if (rest.startsWith("[")) {
    const close = rest.indexOf("]");
    if (close < 0) return null;
    host = rest.slice(1, close);
    const after = rest.slice(close + 1);
    if (after.length > 0) {
      if (!after.startsWith(":")) return null;
      const parsed = validPort(after.slice(1));
      if (parsed === null) return null;
      port = parsed;
    }
    if (!isIpv6(host)) return null;
  } else {
    const parts = rest.split(":");
    if (parts.length > 2 || parts[0].length === 0) return null;
    host = parts[0];
    if (parts.length === 2) {
      const parsed = validPort(parts[1]);
      if (parsed === null) return null;
      port = parsed;
    }
    if (!/^[A-Za-z0-9.-]+$/.test(host)) return null;
  }
  return { server: host, port, transport };
}

/** An IPv4 address with a prefix of /8 to /30 (mesh-module.md, `node.address`). */
export function validAddress(text: string): boolean {
  const parts = text.split("/");
  if (parts.length !== 2 || !isIpv4(parts[0]) || !/^\d+$/.test(parts[1])) return false;
  const prefix = Number(parts[1]);
  return prefix >= 8 && prefix <= 30;
}

function validPort(text: string): number | null {
  if (!/^\d{1,5}$/.test(text)) return null;
  const port = Number(text);
  return port >= 1 && port <= 65535 ? port : null;
}

function isIpv4(text: string): boolean {
  const parts = text.split(".");
  return parts.length === 4 && parts.every((part) => /^\d{1,3}$/.test(part) && Number(part) <= 255 && (part.length === 1 || part[0] !== "0"));
}

function isIpv6(text: string): boolean {
  if (text.length === 0 || !text.includes(":")) return false;
  const double = text.indexOf("::");
  if (double >= 0 && text.indexOf("::", double + 1) >= 0) return false;
  const groups = text.split(":");
  let count = 0;
  for (const [index, group] of groups.entries()) {
    if (group.length === 0) continue;
    if (index === groups.length - 1 && group.includes(".")) {
      if (!isIpv4(group)) return false;
      count += 2;
      continue;
    }
    if (!/^[0-9a-fA-F]{1,4}$/.test(group)) return false;
    count += 1;
  }
  return double >= 0 ? count < 8 : count === 8 && groups.every((group) => group.length > 0);
}

// Words (docs/mesh-ui.md, 用词 and 状态), the same as the phones'.

export function stateLabel(state: string | null | undefined): string {
  switch (state) {
    case "joined":
      return t("已加入");
    case "joining":
      return t("正在加入");
    case "partial":
      return t("部分不可达");
    case "failed":
      return t("无法加入");
    default:
      return t("未加入");
  }
}

/** Whether the join switch shows on for a network state. */
export function joinedState(state: string | null | undefined): boolean {
  return state === "joined" || state === "joining" || state === "partial";
}

/** The dot of a network or member state. */
export function stateTone(state: string | null | undefined): "success" | "warning" | "danger" | "neutral" | "info" {
  switch (state) {
    case "joined":
    case "online":
      return "success";
    case "joining":
      return "info";
    case "partial":
      return "warning";
    case "failed":
    case "unreachable":
      return "danger";
    default:
      return "neutral";
  }
}

export function securityLabel(security: string | null | undefined): string | null {
  switch (security) {
    case "forward_secrecy":
      return t("前向保密");
    case "standard":
      return t("未启用前向保密");
    case "none":
      return t("未加密");
    default:
      return null;
  }
}

/** 未加密 in the danger colour, 未启用前向保密 in the warning colour. */
export function securityTone(security: string | null | undefined): "danger" | "warning" | "accent" {
  return security === "none" ? "danger" : security === "standard" ? "warning" : "accent";
}

export function roleLabel(role: string | null | undefined): string | null {
  switch (role) {
    case "border":
      return t("边界");
    case "gateway":
      return t("网关");
    case "host":
      return t("宿主");
    case "server":
      return t("主机|role");
    case "client":
      return t("接入端");
    default:
      return null;
  }
}

export function connectionLabel(connection: string, hops = 0): string {
  switch (connection) {
    case "self":
      return t("本机");
    case "direct":
      return t("直连");
    case "relay":
      return t("中转 {count} 跳", { count: hops });
    case "cross_region":
      return t("跨区 {count} 跳", { count: hops });
    case "via_host":
      return t("经宿主");
    case "via_gateway":
      return t("经网关");
    default:
      return t("不可达");
  }
}

export function markLabel(mark: string): string | null {
  switch (mark) {
    case "duplicate":
      return t("重名");
    case "unmanaged":
      return t("地址未受管");
    case "fallback":
      return t("已回落");
    case "first_hop_only":
      return t("仅首跳生效");
    case "multi_source":
      return t("多源");
    case "overlap":
      return t("重叠");
    case "local_overlap":
      return t("与本机网络重叠");
    case "refused":
      return t("已拒绝");
    default:
      return null;
  }
}

export function latencyText(milliseconds: number | null | undefined): string {
  return milliseconds === null || milliseconds === undefined ? t("超时") : t("{value} ms", { value: milliseconds });
}

export function joiningText(joining: { stage?: string | null; peersConnected?: number | null } | null | undefined): string | null {
  if (!joining) return null;
  if (joining.stage === "connecting" && typeof joining.peersConnected === "number") {
    return t("正在加入：已连接 {count} 个对端", { count: joining.peersConnected });
  }
  switch (joining.stage) {
    case "connecting":
      return t("正在连接对端");
    case "assigning_address":
      return t("正在取得虚拟地址");
    case "measuring_regions":
      return t("正在测量延迟以确定地区");
    default:
      return null;
  }
}

export function failureText(code: string | null | undefined): string {
  switch (code) {
    case "unencrypted_refused":
      return t("已拒绝加入：网络未加密。在网络设置中允许后方可加入。");
    case "secret_mismatch":
      return t("加入失败：密钥不匹配。核对网络名称与密钥。");
    case "security_mismatch":
      return t("加入失败：加密模式与网络不一致。");
    case "port_in_use":
      return t("无法启动组网：监听端口已被占用。");
    case "tun_in_use":
      return t("无法启动组网：虚拟网卡已被占用。");
    case "route_overlap":
      return t("无法启动组网：虚拟网段与现有路由重叠。");
    case "address_in_use":
      return t("无法加入：虚拟地址已被其他成员使用。");
    case "permission_denied":
      return t("无法启动组网：缺少系统权限。");
    case "identity_online":
      return t("无法加入：本机身份已在其他设备上线。");
    case "existing_program":
      return t("无法启动组网：本机正在运行现有组网程序。");
    case "unreachable_peers":
      return t("无法加入：对端均不可达。");
    case "peer_not_native":
      return t("已拒绝对端连接：对端非本系统节点。");
    case "native_keys_pending":
      return t("暂未加入：本机公钥尚未列入网络名单。");
    case "unsupported":
      return t("当前内核不支持此操作。");
    case "invalid_argument":
      return t("操作未完成：参数无效。");
    case "not_found":
      return t("操作未完成：网络不存在。");
    case "not_allowed":
      return t("操作未完成：网络不允许此操作。");
    default:
      return t("操作未完成。");
  }
}

function value(values: unknown[] | undefined, index: number): string {
  const item = values?.[index];
  return typeof item === "string" || typeof item === "number" ? String(item) : "";
}

function list(items: string[]): string {
  return items.join(t("、"));
}

/** A notice's sentence, in the order of docs/mesh-ui.md 提示. */
export function noticeText(notice: MeshNotice): string {
  const values = notice.values;
  switch (notice.id) {
    case "mesh.notice.path_fallback":
      return t("自定义路径已回落：{member} 不可达。{target} 使用默认路径。", { member: value(values, 0), target: value(values, 1) });
    case "mesh.notice.path_unsupported":
      return t("自定义路径已回落：{member} 不支持按此路径转发。{target} 使用默认路径。", { member: value(values, 0), target: value(values, 1) });
    case "mesh.notice.path_hop_limit":
      return t("自定义路径已回落：跳数超出上限。{target} 使用默认路径。", { target: value(values, 0) });
    case "mesh.notice.detour_loss":
      return t("{from}至{to}已改经{via}：直连链路丢包 {loss}%。", { from: value(values, 0), to: value(values, 1), via: value(values, 2), loss: value(values, 3) || "0" });
    case "mesh.notice.detour_latency":
      return typeof values?.[3] === "number"
        ? t("{from}至{to}已改经{via}：直连链路延迟 {latency} ms。", { from: value(values, 0), to: value(values, 1), via: value(values, 2), latency: value(values, 3) })
        : t("{from}至{to}已改经{via}：直连链路延迟过高。", { from: value(values, 0), to: value(values, 1), via: value(values, 2) });
    case "mesh.notice.partial": {
      const names = Array.isArray(values?.[1]) ? (values?.[1] as unknown[]).filter((name): name is string => typeof name === "string") : [];
      return t("{count} 台成员不可达：{names}。", { count: names.length, names: list(names) });
    }
    default:
      return failureText(notice.key.startsWith("failure:") ? notice.key.slice("failure:".length).split(":")[0] : null);
  }
}

/** The raw detail of a failure notice (the port, the overlapping network). */
export function noticeDetail(notice: MeshNotice): string | null {
  return notice.key.startsWith("failure:") && notice.values?.length ? value(notice.values, 0) || null : null;
}

export function draftProblemText(problem: DraftProblem): string {
  switch (problem.kind) {
    case "name":
      return t("名称须为 1–64 个字符，不含斜杠。");
    case "secret":
      return t("缺少密钥。");
    case "peers":
      return t("缺少对端地址。");
    case "peer":
      return t("对端地址无效：{line}", { line: problem.line });
    case "address":
      return t("虚拟地址无效，格式如 10.144.144.5/24。");
    case "port":
      return t("监听端口须为 1–65535。");
    case "duplicate":
      return t("已存在同名网络。");
  }
}

/** A region's name from the snapshot's list, else its code. */
export function regionName(head: MeshHead, code: string | null | undefined): string {
  if (!code) return "";
  return head.regions?.find((region) => region.code === code)?.name || code;
}

/** Why a custom path fell back, for its row (mesh-status.md `paths[].reason`); null while it has not. */
export function pathFallbackText(path: MeshCustomPath): string | null {
  if (path.state !== "fallback") return null;
  switch (path.reason) {
    case "unsupported":
    case "not_relay":
      return path.unreachable ? t("{name} 不支持按此路径转发", { name: path.unreachable }) : null;
    case "hop_limit":
      return t("跳数超出上限");
    default:
      return path.unreachable ? t("{name} 不可达", { name: path.unreachable }) : null;
  }
}

/** A custom path's state, for its row. */
export function pathStateText(state: string): string {
  switch (state) {
    case "fallback":
      return t("已回落");
    case "first_hop_only":
      return t("仅首跳生效");
    case "pending":
      return t("待确认");
    default:
      return t("生效");
  }
}
