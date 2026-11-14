import { invoke } from "@tauri-apps/api/core";

export type Tone = "success" | "warning" | "danger" | "neutral";

export interface StatusFact {
  label: string;
  detail?: string;
  tone: Tone;
}

/** The Linux and Android clients run the engine in the app itself, with no Windows service or driver. */
export function inProcessEngine(snapshot: { platform?: string } | null | undefined): boolean {
  return snapshot?.platform === "linux" || snapshot?.platform === "android";
}

export interface RuntimeSnapshot {
  /** Sent by the Linux and Android clients; the Windows client leaves it out. */
  platform?: "windows" | "linux" | "android";
  connection: string;
  connectionLabel: string;
  core: StatusFact;
  proxy: StatusFact;
  selectedNode?: string;
  subscription?: string;
  subscriptionLastUpdated?: string;
  subscriptionLastAttempt?: string;
  subscriptionNextAttempt?: string;
  subscriptionRefreshResult: string;
  subscriptionAddedNodeCount: number;
  subscriptionRemovedNodeCount: number;
  subscriptionChangedNodeCount: number;
  subscriptionRefreshErrorCode?: string;
  subscriptionError?: string;
  lastError?: string;
  configurationError?: string;
  startAfterLogin: boolean;
  autoConnect: boolean;
  autoRecoverCore: boolean;
  autoSubscriptionRefresh: boolean;
  autoSelectFastestNode: boolean;
  bilibiliWebAdBlockEnabled: boolean;
  tunModeEnabled?: boolean;
  scriptRuntimeConfigured: boolean;
  /** HTTPS decryption is on (Windows); the Linux client leaves it out. */
  httpsDecryptionEnabled?: boolean;
  darkTheme: boolean;
  routingMode: "bypass_china" | "gfw_list" | "direct" | "return_china" | "rule";
  /** How the engine resolves names; older backends leave it out. */
  dnsPreset?: DnsPreset;
  /** UDP to port 443 is refused so apps fall back from QUIC to TCP; older backends leave it out. */
  blockQuic?: boolean;
  /** Debug mode's direct egress: the engine runs with no node. Left out by backends that cannot do it yet. */
  directEgress?: boolean;
  /** The imported profile runs on the embedded engine (Windows). */
  embeddedEngine?: boolean;
}

export type DnsPreset = "system" | "encrypted" | "split" | "custom";

/** The "自定义" DNS choice as written, and the Fake-IP switch (Windows TUN mode and Android). */
export interface DnsText {
  servers: string;
  rules: string;
  hosts: string;
  fake_ip: boolean;
}

export interface NodeSummary {
  id: string;
  label: string;
  protocol: string;
  selected: boolean;
  /** The subscription the node comes from, when the backend lists several. */
  group?: string;
  /** A node of the subscription in use, which can be edited and removed; older backends leave it out. */
  editable?: boolean;
}

/** A node's edit form; `keepReason` says why it cannot be removed. */
export interface NodeForm {
  name: string;
  server: string;
  port: number;
  keepReason?: string;
}

export interface NativeGroupSummary {
  tag: string;
  groupType: string;
  selected?: string;
  outbounds: string[];
  /** Display names for `outbounds`; empty when they are the names. */
  outboundLabels?: string[];
}

/** Totals since the engine started and the current rates, in bytes and bytes per second. */
export interface TrafficSnapshot {
  upload_total: number;
  download_total: number;
  upload_rate: number;
  download_rate: number;
  active: number;
}

/** One connection as the engine tracks it. */
export interface ConnectionSnapshot {
  id: number;
  network: "tcp" | "udp";
  /** The requested domain, or the sniffed TLS SNI or HTTP Host. */
  host?: string | null;
  /** The local program that opened it, when the engine looked it up. */
  process?: string;
  /** `address:port` or `domain:port` as requested. */
  destination: string;
  rule: string;
  chain: string[];
  outbound: string;
  /** Unix milliseconds. */
  started_at_ms: number;
  ended_at_ms?: number | null;
  upload: number;
  download: number;
  /** `rejected` or `connect_failed`. */
  error?: string | null;
  /** Packet capture kept bytes of this connection (`capture_connection`). */
  captured?: boolean;
}

export interface ConnectionsSnapshot {
  traffic: TrafficSnapshot;
  /** Oldest first. */
  active: ConnectionSnapshot[];
  /** Newest first. */
  closed: ConnectionSnapshot[];
}

/** Packet capture (`capture`, `set_capture`, `clear_capture`): off until turned on, lasts until the engine stops. */
export interface CaptureStatus {
  enabled: boolean;
  max_total_bytes: number;
  used_bytes: number;
  connections: number;
}

/** Bytes one way: seen while capturing, kept, and whether some were not kept. */
export interface CaptureDirection {
  seen: number;
  captured: number;
  truncated: boolean;
}

export interface CaptureChunk {
  dir: "upload" | "download";
  at_ms: number;
  data_base64: string;
}

export interface CaptureBody {
  size: number;
  truncated: boolean;
  data_base64: string;
}

export interface CaptureMessage {
  headers: Array<[string, string]>;
  body: CaptureBody;
}

/** One decrypted HTTP request and its response. */
export interface CaptureExchange {
  started_at_ms: number;
  ended_at_ms?: number | null;
  method: string;
  url: string;
  status?: number | null;
  /** `answered_locally`, `streaming` or `too_large`. */
  note?: string | null;
  request: CaptureMessage;
  response: CaptureMessage;
}

/** What capture kept of one connection: raw bytes, or the HTTP messages when HTTPS decryption took it over. */
export interface CaptureConnection {
  id: number;
  network: "tcp" | "udp";
  host?: string | null;
  destination: string;
  started_at_ms: number;
  ended_at_ms?: number | null;
  decrypted: boolean;
  upload: CaptureDirection;
  download: CaptureDirection;
  chunks: CaptureChunk[];
  http: CaptureExchange[];
}

export type CaptureFormat = "pcapng" | "har" | "curl" | "code" | "postman" | "insomnia";

/** The language of a `code` export. */
export type CodeLanguage = "python" | "javascript" | "swift" | "kotlin" | "go";

export interface ExportOptions {
  /** One exchange of the connection (its index in `http`); needs the connection id. */
  exchange?: number;
  language?: CodeLanguage;
  /** Mask cookies, tokens and passwords; the engine does unless this is false. */
  redact?: boolean;
}

/** One decrypted request in the engine's request list (`capture_requests`). */
export interface CaptureRequest {
  /** Rises with every request; the list is read again from the last one seen. */
  seq: number;
  connection_id: number;
  /** Its place in the connection's `http` list. */
  index: number;
  method: string;
  url: string;
  host: string;
  status?: number | null;
  note?: string | null;
  http_version: string;
  started_at_ms: number;
  ended_at_ms?: number | null;
  request_size: number;
  response_size: number;
  response_content_type?: string | null;
}

/** What keeps the key of saved capture sessions on this device. */
export type SessionProtection = "dpapi" | "keystore" | "secret-service" | "file";

/** One saved capture session (`capture_sessions`). */
export interface CaptureSession {
  session: string;
  /** Empty when saved without a name. */
  name?: string;
  saved_at_ms: number;
  expires_at_ms: number;
  size: number;
  connections?: number;
  requests?: number;
  /** Sealed with another key (e.g. a reinstall): it cannot be opened and expires on its own. */
  unreadable?: boolean;
}

/** Every session answer says what keeps the key and how long sessions are kept. */
interface SessionAnswer {
  protection: SessionProtection;
  retention_days: number;
}

export type CaptureSessionAction =
  | { action: "list" }
  | { action: "save"; name?: string }
  | { action: "load"; session: string }
  | { action: "delete"; session: string };

export interface CaptureRequestsAnswer {
  requests?: CaptureRequest[];
  last_seq?: number;
  /** The highest seq the engine dropped (capture budget, cleared, a session loaded); it and older rows are gone. */
  dropped_before_seq?: number;
  /** `invalid_filter` with the character position of the mistake. */
  error?: { code: string; at?: number; message?: string };
}

/** An edited copy of a captured request, sent again through the local proxy. */
export interface ReplayRequest {
  method: string;
  url: string;
  headers: [string, string][];
  /** Empty for no body. */
  bodyBase64: string;
}

export interface ReplayResponse {
  status: number;
  headers: [string, string][];
  bodyBase64: string;
  /** Cut at 1 MiB. */
  truncated: boolean;
  elapsedMs: number;
}

/** A Map Local / Map Remote rule made in the debug view; saved ones become the plugin "AnixOps 调试映射". */
export interface DebugMapping {
  /** Empty for a new one. */
  id: string;
  name: string;
  enabled: boolean;
  kind: "local" | "remote";
  match: "url" | "prefix" | "regex";
  pattern: string;
  status: number;
  headers: { name: string; value: string }[];
  /** The body as base64. */
  body: string;
  target: string;
}

/** One rule set of the running engine. */
export interface RuleSetStatus {
  name: string;
  source: "inline" | "remote";
  entries: number;
  /** Unix milliseconds of the last successful download. */
  updated_at_ms?: number | null;
  last_error?: string | null;
}

/** One node's result in a batch delay test. */
export interface NodeDelay {
  nodeId: string;
  delayMs?: number | null;
  error?: string | null;
}

/** An HTTPS decryption plugin imported from a URL. */
export interface MitmPlugin {
  id: string;
  name: string;
  url: string;
  enabled: boolean;
  /** How much of the plugin the engine supports. */
  level: "full" | "partial" | "unsupported" | string;
  hostnames: string[];
  scriptCount: number;
}

/** One hour of traffic; `start` is Unix seconds. */
export interface TrafficHour {
  start: number;
  upload: number;
  download: number;
  hosts: Record<string, number>;
  policies: Record<string, number>;
}

/** Seven days of traffic, oldest hour first. */
export interface TrafficStats {
  hours: TrafficHour[];
}

/** What a saved plugin holds, as the engine reads it. */
export interface MitmPluginDetails {
  excludedHostnames: string[];
  supportedLines: number;
  partialLines: number;
  unsupportedLines: number;
  deferredLines: number;
  scripts: string[];
  resources: string[];
  report: string;
}

/** What the exit looks like from outside and how fast a mainland site answers around the proxy. */
export interface NetworkProbe {
  exitIp?: string | null;
  /** Two-letter country code of the exit. */
  exitLocation?: string | null;
  directMs?: number | null;
}

/** The engine's LAN share for phones (`lan_share` control commands). */
export interface LanShareStatus {
  running: boolean;
  port?: number | null;
  /** This computer's private IPv4 addresses, the likeliest first. */
  addresses: string[];
  /** The phone setup page on the first address. */
  url?: string | null;
  /** The made-up host that answers the setup page through the proxy. */
  page_host: string;
  https_decryption: boolean;
  certificate: boolean;
  credentials: boolean;
  accepted: number;
  rejected: number;
  active: number;
  pages: number;
}

export interface OperationResult {
  message: string;
}

export interface SubscriptionSummary {
  id: string;
  location: string;
  selected: boolean;
  lastSuccessfulUpdate?: string;
  lastAttempt?: string;
  nextAttempt?: string;
  result: string;
  addedNodeCount: number;
  removedNodeCount: number;
  changedNodeCount: number;
  errorCode?: string;
  lastUpdateError?: string;
  /** The user's name, the server's title, or the host. */
  name: string;
  nodeCount?: number;
  /** Bytes, from the server's subscription-userinfo header. */
  upload?: number;
  download?: number;
  total?: number;
  /** Unix seconds. */
  expire?: number;
  /** 0 updates by hand only. */
  updateIntervalHours: number;
  /** A link that can be downloaded again, not a file or pasted links. */
  refreshable: boolean;
}

/** Crash reports, the local metrics endpoint, the install check and the cold start (`health_status`). */
export interface HealthStatus {
  crashReports: boolean;
  crashReportsKept: number;
  /** The build sends reports to an address; otherwise they stay on this computer. */
  crashUploadConfigured: boolean;
  /** `http://127.0.0.1:9477/metrics` while the endpoint runs. */
  metricsAddress?: string | null;
  metricsError?: string | null;
  integrity: "checking" | "verified" | "unverified" | "damaged" | "invalid";
  integrityDetail?: string | null;
  coldStartMs?: number | null;
}

export type Preferences = Pick<RuntimeSnapshot, "startAfterLogin" | "autoConnect" | "autoRecoverCore" | "autoSubscriptionRefresh" | "autoSelectFastestNode" | "bilibiliWebAdBlockEnabled" | "darkTheme">;

/** The saved preferences from a snapshot with some of them changed. */
export function preferencesFrom(snapshot: RuntimeSnapshot, changes: Partial<Preferences>): Preferences {
  return {
    startAfterLogin: snapshot.startAfterLogin,
    autoConnect: snapshot.autoConnect,
    autoRecoverCore: snapshot.autoRecoverCore,
    autoSubscriptionRefresh: snapshot.autoSubscriptionRefresh,
    autoSelectFastestNode: snapshot.autoSelectFastestNode,
    bilibiliWebAdBlockEnabled: snapshot.bilibiliWebAdBlockEnabled,
    darkTheme: snapshot.darkTheme,
    ...changes,
  };
}

export const desktop = {
  snapshot: () => invoke<RuntimeSnapshot>("runtime_snapshot"),
  nodes: () => invoke<NodeSummary[]>("list_nodes"),
  nativeGroups: () => invoke<NativeGroupSummary[]>("list_native_groups"),
  subscriptions: () => invoke<SubscriptionSummary[]>("list_subscriptions"),
  connect: () => invoke<OperationResult>("connect"),
  disconnect: () => invoke<OperationResult>("disconnect"),
  restart: () => invoke<OperationResult>("restart_service"),
  validate: () => invoke<OperationResult>("validate_configuration"),
  switchNode: (nodeId: string) => invoke<OperationResult>("switch_node", { nodeId }),
  selectNativeGroupOutbound: (groupTag: string, outboundTag: string) =>
    invoke<OperationResult>("select_native_group_outbound", { groupTag, outboundTag }),
  /** Only while this window runs the engine (desktop mode on Windows); rejects otherwise. */
  traffic: () => invoke<TrafficSnapshot>("runtime_control", { request: { cmd: "traffic" } }),
  connections: () => invoke<ConnectionsSnapshot>("runtime_control", { request: { cmd: "connections" } }),
  closeConnection: (id: number) => invoke<{ closed: boolean }>("runtime_control", { request: { cmd: "close_connection", id } }),
  ruleSets: () => invoke<{ rule_sets: RuleSetStatus[] }>("runtime_control", { request: { cmd: "rule_sets" } }),
  /** Downloads the remote rule sets again in the background; answers how many were scheduled. */
  updateRuleSets: () => invoke<{ scheduled: number }>("runtime_control", { request: { cmd: "update_rule_sets" } }),
  closeAllConnections: () => invoke<{ closed: number }>("runtime_control", { request: { cmd: "close_all" } }),
  captureStatus: () => invoke<CaptureStatus>("runtime_control", { request: { cmd: "capture" } }),
  setCapture: (enabled: boolean) => invoke<CaptureStatus>("runtime_control", { request: { cmd: "set_capture", enabled } }),
  clearCapture: () => invoke<CaptureStatus>("runtime_control", { request: { cmd: "clear_capture" } }),
  /** Null once newer connections pushed it out of the budget or capture was cleared. */
  captureConnection: (id: number) =>
    invoke<{ capture: CaptureConnection | null }>("runtime_control", { request: { cmd: "capture_connection", id } }),
  /** Saves the export to Downloads (one connection with `id`) and answers where it went. */
  exportCapture: (format: CaptureFormat, id?: number, options?: ExportOptions) =>
    invoke<{ location: string }>("export_capture", { format, id, options }),
  /** Saved capture sessions, newest first; the backend names the folder and keeps the key. */
  captureSessions: () =>
    invoke<SessionAnswer & { sessions: CaptureSession[] }>("capture_session", { action: { action: "list" } satisfies CaptureSessionAction }),
  /** Seals what capture holds into a new session. */
  saveCaptureSession: (name?: string) =>
    invoke<SessionAnswer & { session: CaptureSession }>("capture_session", { action: { action: "save", name } satisfies CaptureSessionAction }),
  /** Replaces what capture holds with a saved session and turns live capture off. */
  loadCaptureSession: (session: string) =>
    invoke<SessionAnswer & CaptureStatus>("capture_session", { action: { action: "load", session } satisfies CaptureSessionAction }),
  deleteCaptureSession: (session: string) =>
    invoke<SessionAnswer & { deleted: boolean }>("capture_session", { action: { action: "delete", session } satisfies CaptureSessionAction }),
  /** Decrypted requests after `since`, the newest last; `filter` is the engine's filter expression. */
  captureRequests: (since: number, filter: string) =>
    invoke<CaptureRequestsAnswer>("runtime_control", { request: { cmd: "capture_requests", since_seq: since, limit: 2000, ...(filter ? { filter } : {}) } }),
  /** Sends a request through the engine (its local proxy, or the replay proxy on Android) without following redirects. */
  replayRequest: (request: ReplayRequest) => invoke<ReplayResponse>("replay_request", { request }),
  debugMappings: () => invoke<DebugMapping[]>("list_debug_mappings"),
  saveDebugMapping: (entry: DebugMapping) => invoke<OperationResult>("save_debug_mapping", { entry }),
  removeDebugMapping: (id: string) => invoke<OperationResult>("remove_debug_mapping", { id }),
  testDelay: (nodeId: string, url: string) =>
    invoke<OperationResult>("test_node_delay", { nodeId, url }),
  /** Tests each node on its own and reports every result; failures do not reject. */
  testDelays: (nodeIds: string[], url: string) => invoke<NodeDelay[]>("test_node_delays", { nodeIds, url }),
  /** The exit address through the local proxy (only while `connected`) and the delay of a direct mainland request. */
  networkProbe: (connected: boolean) => invoke<NetworkProbe>("network_probe", { connected }),
  /** The node's share link, or a Surge line for Snell. */
  shareLink: (nodeId: string) => invoke<string>("node_share_link", { nodeId }),
  /** Seven days of traffic by hour, host and policy; `clear` empties it first. */
  trafficStats: (clear = false) => invoke<TrafficStats>("traffic_stats", { clear }),
  nodeForm: (nodeId: string) => invoke<NodeForm>("node_form", { nodeId }),
  /** Until the subscription is updated, the node runs with this name, server and port. */
  editNode: (nodeId: string, name: string, server: string, port: string) => invoke<OperationResult>("edit_node", { nodeId, name, server, port }),
  /** Until the subscription is updated, the node is left out. */
  removeNode: (nodeId: string) => invoke<OperationResult>("remove_node", { nodeId }),
  selectFastestNode: () => invoke<OperationResult>("select_fastest_node"),
  savePreferences: (preferences: Preferences) =>
    invoke<OperationResult>("save_preferences", {
      preferences,
    }),
  diagnostics: () => invoke<OperationResult>("create_diagnostics"),
  health: () => invoke<HealthStatus>("health_status"),
  setCrashReports: (enabled: boolean) => invoke<OperationResult>("set_crash_reports", { enabled }),
  importSubscription: (location: string) =>
    invoke<OperationResult>("import_subscription", { location }),
  updateSubscription: () => invoke<OperationResult>("update_subscription"),
  selectSubscription: (id: string) => invoke<OperationResult>("select_subscription", { id }),
  removeSubscription: (id: string) => invoke<OperationResult>("remove_subscription", { id }),
  updateSubscriptionSource: (id: string) => invoke<OperationResult>("update_subscription_source", { id }),
  renameSubscription: (id: string, name: string) => invoke<OperationResult>("rename_subscription", { id, name }),
  setSubscriptionInterval: (id: string, hours: number) =>
    invoke<OperationResult>("set_subscription_interval", { id, hours }),
  checkProfileRuntime: () => invoke<OperationResult>("check_profile_runtime"),
  restoreNetworkSettings: () => invoke<OperationResult>("restore_network_settings"),
  setRoutingMode: (mode: RuntimeSnapshot["routingMode"]) => invoke<OperationResult>("set_routing_mode", { mode }),
  setDnsPreset: (preset: DnsPreset) => invoke<OperationResult>("set_dns_preset", { preset }),
  customDns: () => invoke<DnsText>("custom_dns"),
  /** Checks every line before saving; the error names the field and the line. */
  saveCustomDns: (text: DnsText) => invoke<OperationResult>("save_custom_dns", { text }),
  setBlockQuic: (enabled: boolean) => invoke<OperationResult>("set_block_quic", { enabled }),
  setDirectEgress: (enabled: boolean) => invoke<OperationResult>("set_direct_egress", { enabled }),
  refreshRoutingRules: () => invoke<OperationResult>("refresh_routing_rules"),
  /** "我的规则": the saved text and how many rule lines it holds. */
  myRules: () => invoke<{ text: string; count: number }>("my_rules"),
  /** Checks every line, saves, and gives a running engine the new rules. */
  saveMyRules: (text: string) => invoke<OperationResult>("save_my_rules", { text }),
  enableHttpsMitm: () => invoke<OperationResult>("enable_https_mitm"),
  disableHttpsMitm: () => invoke<OperationResult>("disable_https_mitm"),
  mitmPlugins: () => invoke<MitmPlugin[]>("list_mitm_plugins"),
  importMitmPlugin: (url: string) => invoke<OperationResult>("import_mitm_plugin", { url }),
  /** A plugin opened from a file (`source` is its name) or pasted (`source` is empty). */
  importMitmPluginText: (text: string, source: string) => invoke<OperationResult>("import_mitm_plugin_text", { text, source }),
  mitmPluginDetails: (id: string) => invoke<MitmPluginDetails>("mitm_plugin_details", { id }),
  setMitmPluginEnabled: (id: string, enabled: boolean) =>
    invoke<OperationResult>("set_mitm_plugin_enabled", { id, enabled }),
  removeMitmPlugin: (id: string) => invoke<OperationResult>("remove_mitm_plugin", { id }),
  /** The LAN share of the running engine; rejects while the engine does not run in this window. */
  lanShare: () => invoke<LanShareStatus>("runtime_control", { request: { cmd: "lan_share" } }),
  startLanShare: (port: number) => invoke<LanShareStatus>("runtime_control", { request: { cmd: "start_lan_share", port } }),
  stopLanShare: () => invoke<LanShareStatus>("runtime_control", { request: { cmd: "stop_lan_share" } }),
  /** An SVG QR code of `text`. */
  phoneQrCode: (text: string) => invoke<string>("phone_qr_code", { text }),
  importMitmRoot: (certificatePem: string, privateKeyPem: string) =>
    invoke<OperationResult>("import_mitm_root", { certificatePem, privateKeyPem }),
  /** SHA-256 of the HTTPS decryption root, or null before one exists. */
  mitmRootFingerprint: () => invoke<string | null>("mitm_root_fingerprint"),
  setTunMode: (enabled: boolean) => invoke<OperationResult>("set_tun_mode", { enabled }),
  installCertificate: (path: string) => invoke<OperationResult>("install_certificate", { path }),
  removeCertificate: () => invoke<OperationResult>("remove_certificate"),
  installDriver: (path: string) => invoke<OperationResult>("install_driver", { path }),
  removeDriver: () => invoke<OperationResult>("remove_driver"),
};
