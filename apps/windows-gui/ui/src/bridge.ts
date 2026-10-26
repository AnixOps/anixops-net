import { invoke } from "@tauri-apps/api/core";

export type Tone = "success" | "warning" | "danger" | "neutral";

export interface StatusFact {
  label: string;
  detail?: string;
  tone: Tone;
}

export interface RuntimeSnapshot {
  /** Sent by the Linux client; the Windows client leaves it out. */
  platform?: "windows" | "linux";
  connection: string;
  connectionLabel: string;
  service: StatusFact;
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
  scriptRuntimeConfigured: boolean;
  darkTheme: boolean;
  runtimeMode: "desktop" | "service";
  routingMode: "bypass_china" | "gfw_list" | "direct" | "return_china" | "rule";
  /** The imported profile runs on the embedded engine (Windows). */
  embeddedEngine?: boolean;
}

export interface NodeSummary {
  id: string;
  label: string;
  protocol: string;
  selected: boolean;
  /** The subscription the node comes from, when the backend lists several. */
  group?: string;
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
}

export interface ConnectionsSnapshot {
  traffic: TrafficSnapshot;
  /** Oldest first. */
  active: ConnectionSnapshot[];
  /** Newest first. */
  closed: ConnectionSnapshot[];
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

export type Preferences = Pick<RuntimeSnapshot, "startAfterLogin" | "autoConnect" | "autoRecoverCore" | "autoSubscriptionRefresh" | "autoSelectFastestNode" | "bilibiliWebAdBlockEnabled" | "darkTheme" | "runtimeMode">;

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
    runtimeMode: snapshot.runtimeMode,
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
  testDelay: (nodeId: string, url: string) =>
    invoke<OperationResult>("test_node_delay", { nodeId, url }),
  /** Tests each node on its own and reports every result; failures do not reject. */
  testDelays: (nodeIds: string[], url: string) => invoke<NodeDelay[]>("test_node_delays", { nodeIds, url }),
  /** The node's share link, or a Surge line for Snell. */
  shareLink: (nodeId: string) => invoke<string>("node_share_link", { nodeId }),
  selectFastestNode: () => invoke<OperationResult>("select_fastest_node"),
  savePreferences: (preferences: Preferences) =>
    invoke<OperationResult>("save_preferences", {
      preferences,
    }),
  diagnostics: () => invoke<OperationResult>("create_diagnostics"),
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
  installService: () => invoke<OperationResult>("install_service"),
  startService: () => invoke<OperationResult>("start_service"),
  stopService: () => invoke<OperationResult>("stop_service"),
  restoreNetworkSettings: () => invoke<OperationResult>("restore_network_settings"),
  setRoutingMode: (mode: RuntimeSnapshot["routingMode"]) => invoke<OperationResult>("set_routing_mode", { mode }),
  refreshRoutingRules: () => invoke<OperationResult>("refresh_routing_rules"),
  configureScriptRuntime: (scriptRuntimeJson: string) =>
    invoke<OperationResult>("configure_script_runtime", { scriptRuntimeJson }),
  clearScriptRuntime: () => invoke<OperationResult>("clear_script_runtime"),
  enableHttpsMitm: () => invoke<OperationResult>("enable_https_mitm"),
  disableHttpsMitm: () => invoke<OperationResult>("disable_https_mitm"),
  installCertificate: (path: string) => invoke<OperationResult>("install_certificate", { path }),
  removeCertificate: () => invoke<OperationResult>("remove_certificate"),
  installDriver: (path: string) => invoke<OperationResult>("install_driver", { path }),
  removeDriver: () => invoke<OperationResult>("remove_driver"),
};
