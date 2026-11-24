import { Activity, Bot, Check, CloudDownload, FileText, Gauge, Globe, Layers, Pencil, Plus, RefreshCw, Route, ScanLine, Search, Share2, Trash2, X, Zap } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { desktop, inProcessEngine, NativeGroupSummary, NodeDelay, NodeForm, NodeSummary, RuntimeSnapshot, SubscriptionSummary } from "../bridge";
import { memberNode } from "../dashboard";
import { subscriptionFailureLine } from "../error-text";
import { groupMemberLabel, protocolAbbreviation } from "../labels";
import { delayLabel, delayTone, filterNodes, groupNodes, NodeSort, sortNodes } from "../node-filter";
import { nodeFormProblem } from "../node-form";
import { Card, EmptyState, Field, IconButton, MenuItem, MoreMenu, PageTools, Panel, Row, Run, Segmented, StatusPill, Tile, TileColor } from "../ui";
import { Select } from "../controls";
import type { DebugExit } from "../mode";
import { t } from "../i18n";
import { cancelQrScan, scanQrCode } from "../qr-scan";
import { formatBytes, formatDay, updateIntervalChoices, updateIntervalLabel, usedFraction } from "../subscription-format";
import type { Delays, SetDelays } from "./HomePage";

/** What the right pane shows: every node, one policy group's members, or the subscriptions. */
type View = { kind: "nodes" } | { kind: "group"; tag: string } | { kind: "subscriptions" };

/**
 * Two panes: on the left the node list, the profile's policy groups and the subscriptions; on the right the nodes
 * of the chosen one as cards colored by delay, or the subscription settings.
 */
/** Debug mode's egress choice, shown above the nodes when the page is "Egress". */
export interface Egress {
  exit: DebugExit;
  setExit: (exit: DebugExit) => void;
  /** The backend can run with no node; older backends always use the nodes. */
  supported: boolean;
}

type ProxiesProps = {
  snapshot: RuntimeSnapshot | null;
  nodes: NodeSummary[];
  busy: string | null;
  run: Run;
  delays: Delays;
  setDelays: SetDelays;
  /** A link an `anixops://import` link asked to import, put in the form for the user to confirm. */
  pendingImport?: string | null;
  takePendingImport?: () => void;
};

/** Subscriptions, policy groups and nodes; in debug mode the "Egress" page, with where captured traffic leaves on top. */
export function ProxiesPage({ egress, ...props }: ProxiesProps & { egress?: Egress }) {
  if (!egress) return <ProxiesContent {...props} />;
  return (
    <div className="page-stack">
      <EgressCard egress={egress} nodes={props.nodes} />
      <ProxiesContent {...props} />
    </div>
  );
}

function EgressCard({ egress, nodes }: { egress: Egress; nodes: NodeSummary[] }) {
  if (!egress.supported) {
    return <Card title={t("出口")} description={t("这个客户端不能直连出口，调试网络经过选中的节点发出流量。")} />;
  }
  const description = egress.exit === "direct"
    ? t("抓到的流量直接从本设备发出，无需节点。")
    : nodes.length === 0
      ? t("暂无节点，当前直连。导入后按下方节点和规则转发。")
      : t("按所选节点和订阅规则发出。");
  return (
    <Card title={t("出口")} description={description}>
      <Row icon={Route} color="sky" label={t("抓到的流量从哪里发出")}>
        <Segmented
          label={t("出口")}
          options={[{ value: "direct", label: t("直连") }, { value: "node", label: t("经过节点") }]}
          value={egress.exit}
          onChange={egress.setExit}
        />
      </Row>
    </Card>
  );
}

function ProxiesContent({ snapshot, nodes, busy, run, delays, setDelays, pendingImport, takePendingImport }: ProxiesProps) {
  const [view, setView] = useState<View>({ kind: "nodes" });
  const [query, setQuery] = useState("");
  const [protocol, setProtocol] = useState("All");
  const [sort, setSort] = useState<NodeSort>("default");
  const [editing, setEditing] = useState<{ node: NodeSummary; form: NodeForm } | null>(null);
  const [groups, setGroups] = useState<NativeGroupSummary[]>([]);
  const [sources, setSources] = useState<SubscriptionSummary[]>([]);
  const refreshSources = useCallback(() => void desktop.subscriptions().then(setSources).catch(() => setSources([])), []);
  // An `anixops://import` link opens the subscriptions with the link filled in.
  useEffect(() => {
    if (pendingImport) setView({ kind: "subscriptions" });
  }, [pendingImport]);
  useEffect(() => {
    refreshSources();
    void desktop.nativeGroups().then(setGroups).catch(() => setGroups([]));
  }, [refreshSources, snapshot?.subscription, snapshot?.subscriptionLastUpdated]);

  const testDelays = (targets: NodeSummary[]) => void run("delay", async () => {
    const results = await desktop.testDelays(targets.map((node) => node.id), delayUrl);
    setDelays((current) => ({ ...current, ...Object.fromEntries(results.map((result) => [result.nodeId, result])) }));
    if (targets.length === 1) {
      const result = results[0];
      return { message: result?.delayMs != null ? t("{node}：{ms} ms", { node: targets[0].label, ms: result.delayMs }) : t("{node} 测速失败", { node: targets[0].label }) };
    }
    return { message: t("{ok} / {total} 个节点有响应", { ok: results.filter((result) => result.delayMs != null).length, total: results.length }) };
  });

  const edit = (node: NodeSummary) => void desktop.nodeForm(node.id).then(
    (form) => setEditing({ node, form }),
    (error: unknown) => run("node", () => Promise.reject(error)),
  );
  const nodeMenu = (node: NodeSummary): MenuItem[] => [
    { label: t("复制分享链接"), icon: Share2, onSelect: () => void run("share", () => copyShareLink(node)) },
    ...(node.editable ? [
      { label: t("编辑节点"), icon: Pencil, onSelect: () => edit(node) },
      { label: t("删除节点"), icon: Trash2, danger: true, confirm: t("确认删除 {node}", { node: node.label }), onSelect: () => void run("node", () => desktop.removeNode(node.id)) },
    ] : []),
  ];

  if (nodes.length === 0 && sources.length === 0) {
    return <Subscriptions snapshot={snapshot} sources={sources} refreshSources={refreshSources} empty busy={busy} run={run} pending={pendingImport} takePending={takePendingImport} />;
  }

  const group = view.kind === "group" ? groups.find((item) => item.tag === view.tag) : undefined;
  const shown: View = view.kind === "group" && !group ? { kind: "nodes" } : view;
  const protocols = ["All", ...new Set(nodes.map((node) => node.protocol))];
  const visible = filterNodes(nodes, query, protocol);
  const sections = groupNodes(visible).map((section) => ({ ...section, nodes: sortNodes(section.nodes, sort, delays) }));
  const needle = query.trim().toLowerCase();
  const members = (group?.outbounds ?? [])
    .map((outbound) => {
      const label = groupMemberLabel(group!, outbound) ?? outbound;
      return { outbound, label, node: memberNode(nodes, outbound, label) };
    })
    .filter((member) => !needle || member.label.toLowerCase().includes(needle));
  const testable = shown.kind === "group" ? members.flatMap((member) => member.node ? [member.node] : []) : visible;
  const current = nodes.find((node) => node.selected);
  const available = visible.filter((node) => delays[node.id]?.delayMs != null).length;

  return (
    <div className="panes">
      <PageTools>
        {shown.kind !== "subscriptions" && (
          <label className="search">
            <Search size={14} aria-hidden="true" />
            <input aria-label={t("搜索节点")} placeholder={t("搜索节点")} value={query} onChange={(event) => setQuery(event.target.value)} />
          </label>
        )}
        {/* Node actions belong to the node views only; the subscriptions view has its own primary action, import. */}
        {shown.kind !== "subscriptions" && <>
          <button className="secondary" disabled={Boolean(busy) || testable.length === 0} onClick={() => testDelays(testable)}><Gauge size={14} />{t("全部测速")}</button>
          <button className="primary" disabled={Boolean(busy) || nodes.length === 0} onClick={() => void run("fastest node", desktop.selectFastestNode)}><Zap size={14} />{t("选择最快")}</button>
        </>}
      </PageTools>

      <Panel className="side-pane">
        <nav aria-label={t("节点和订阅")}>
          <h3 className="pane-caption">{t("节点")}</h3>
          <PaneItem
            icon={Globe}
            color="sky"
            title={t("全部节点")}
            detail={[current?.label, t("{count} 个节点", { count: nodes.length })].filter(Boolean).join(" · ")}
            active={shown.kind === "nodes"}
            onClick={() => setView({ kind: "nodes" })}
          />
          {groups.length > 0 && <h3 className="pane-caption">{t("策略组")}</h3>}
          {groups.map((item) => (
            <PaneItem
              key={item.tag}
              {...groupLook(item)}
              title={item.tag}
              detail={[groupMemberLabel(item, item.selected), groupTypeLabel(item.groupType)].filter(Boolean).join(" · ")}
              active={shown.kind === "group" && shown.tag === item.tag}
              onClick={() => setView({ kind: "group", tag: item.tag })}
            />
          ))}
          <h3 className="pane-caption">{t("订阅")}</h3>
          {sources.map((source) => (
            <PaneItem
              key={source.id}
              icon={source.refreshable ? CloudDownload : FileText}
              color={source.refreshable ? "indigo" : "gray"}
              title={source.name}
              detail={sourceDetail(source)}
              badge={source.selected ? t("使用中") : undefined}
              active={false}
              onClick={() => setView({ kind: "subscriptions" })}
            />
          ))}
          <button className={`pane-item add ${shown.kind === "subscriptions" ? "active" : ""}`} onClick={() => setView({ kind: "subscriptions" })}>
            <Plus size={14} />{sources.length > 0 ? t("管理和导入订阅") : t("导入订阅")}
          </button>
        </nav>
      </Panel>

      {shown.kind === "subscriptions" && <Subscriptions snapshot={snapshot} sources={sources} refreshSources={refreshSources} empty={false} busy={busy} run={run} pending={pendingImport} takePending={takePendingImport} />}

      {shown.kind === "nodes" && (
        <Panel
          className="main-pane"
          title={<>{t("全部节点")}<span className="count">{[t("{count} 个节点", { count: visible.length }), available > 0 ? t("{count} 个可用", { count: available }) : null].filter(Boolean).join(" · ")}</span></>}
          action={<>
            <Select label={t("按协议筛选")} value={protocol} onChange={setProtocol} choices={protocols.map((value) => ({ value, label: value === "All" ? t("全部协议") : protocolAbbreviation(value) }))} />
            <Select<NodeSort>
              label={t("节点排序")}
              value={sort}
              onChange={setSort}
              choices={[
                { value: "default", label: t("默认顺序") },
                { value: "name", label: t("按名称") },
                { value: "delay", label: t("按延迟") },
              ]}
            />
          </>}
        >
          {editing && <NodeEditor key={editing.node.id} node={editing.node} form={editing.form} busy={busy} run={run} close={() => setEditing(null)} />}
          {sections.map((section) => (
            <div className="node-section" key={section.group ?? ""}>
              {sections.length > 1 && section.group && <h3 className="subhead">{section.group}<span className="count">{section.nodes.length}</span></h3>}
              <div className="node-grid" role="list" aria-label={section.group ?? t("节点")}>
                {section.nodes.map((node) => (
                  <NodeCard
                    key={node.id}
                    label={node.label}
                    protocol={node.protocol}
                    delay={delays[node.id]}
                    selected={node.selected}
                    disabled={Boolean(busy)}
                    onPick={() => void run("switch", () => desktop.switchNode(node.id))}
                    onTest={() => testDelays([node])}
                    menu={nodeMenu(node)}
                  />
                ))}
              </div>
            </div>
          ))}
          {visible.length === 0 && <p className="panel-empty">{t("没有符合条件的节点。")}</p>}
        </Panel>
      )}

      {shown.kind === "group" && group && (
        <Panel
          className="main-pane"
          title={<>{group.tag}<span className="count">{[groupTypeLabel(group.groupType), t("{count} 个成员", { count: group.outbounds.length })].join(" · ")}</span></>}
        >
          <div className="node-grid" role="list" aria-label={group.tag}>
            {members.map(({ outbound, label, node }) => (
              <NodeCard
                key={outbound}
                label={label}
                protocol={node?.protocol}
                delay={node ? delays[node.id] : undefined}
                selected={group.selected === outbound}
                disabled={Boolean(busy)}
                onPick={group.groupType === "selector" ? () => void run("native selector", async () => {
                  const result = await desktop.selectNativeGroupOutbound(group.tag, outbound);
                  setGroups((list) => list.map((item) => item.tag === group.tag ? { ...item, selected: outbound } : item));
                  return result;
                }) : undefined}
                onTest={node ? () => testDelays([node]) : undefined}
              />
            ))}
          </div>
          {members.length === 0 && <p className="panel-empty">{t("没有符合条件的节点。")}</p>}
          {group.groupType !== "selector" && <p className="panel-note">{t("这个策略组自动选择出口，不能手动切换。")}</p>}
        </Panel>
      )}
    </div>
  );
}

function PaneItem({ icon, color, title, detail, badge, active, onClick }: {
  icon: LucideIcon;
  color: TileColor;
  title: string;
  detail?: string;
  badge?: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button className={`pane-item ${active ? "active" : ""}`} aria-current={active ? "true" : undefined} onClick={onClick}>
      <Tile icon={icon} color={color} />
      <span className="pane-text"><strong>{title}</strong>{detail && <small>{detail}</small>}</span>
      {badge && <span className="chip">{badge}</span>}
    </button>
  );
}

/** One node as a card: its name, protocol and delay; the whole card picks it. */
function NodeCard({ label, protocol, delay, selected, disabled, onPick, onTest, menu }: {
  label: string;
  protocol?: string;
  delay?: NodeDelay;
  selected: boolean;
  disabled: boolean;
  onPick?: () => void;
  onTest?: () => void;
  /** Share, edit and remove (actions S4: one icon action, the rest in the menu). */
  menu?: MenuItem[];
}) {
  return (
    <div className={`node-card ${selected ? "selected" : ""} ${onPick ? "" : "fixed"}`} role="listitem">
      <button className="node-card-pick" aria-pressed={selected} disabled={disabled || selected || !onPick} onClick={onPick} title={label}>
        <strong>{label}</strong>
        <span className="node-card-meta">
          {protocol && <span className="protocol-badge">{protocolAbbreviation(protocol)}</span>}
          <span className={`delay ${delay ? delayTone(delay) : ""}`} title={delay?.error ?? undefined}>{delay ? delayLabel(delay) : ""}</span>
        </span>
      </button>
      {selected && <Check className="node-card-check" size={14} strokeWidth={2.5} aria-label={t("当前节点")} />}
      <span className="node-card-actions">
        {onTest && <IconButton label={t("测试 {node} 的延迟", { node: label })} disabled={disabled} onClick={onTest}><Activity size={13} /></IconButton>}
        {menu && <MoreMenu label={t("{node} 的更多操作", { node: label })} disabled={disabled} items={menu} />}
      </span>
    </div>
  );
}

function groupLook(group: NativeGroupSummary): { icon: LucideIcon; color: TileColor } {
  if (/ai|gpt|openai|claude/i.test(group.tag)) return { icon: Bot, color: "teal" };
  if (group.groupType === "urltest") return { icon: Zap, color: "green" };
  return { icon: Layers, color: "orange" };
}

function groupTypeLabel(type: string): string {
  return type === "selector" ? t("手动选择") : type === "urltest" ? t("自动测速") : type;
}

/** `42 nodes · 128 GB / 500 GB`, as much of it as the subscription reports. */
function sourceDetail(source: SubscriptionSummary): string {
  const used = source.upload == null && source.download == null ? undefined : (source.upload ?? 0) + (source.download ?? 0);
  return [
    source.nodeCount != null ? t("{count} 个节点", { count: source.nodeCount }) : null,
    used != null ? (source.total ? [formatBytes(used), formatBytes(source.total)].join(" / ") : formatBytes(used)) : source.refreshable ? null : t("本地文件"),
  ].filter(Boolean).join(" · ");
}

const delayUrl = "https://www.gstatic.com/generate_204";

/** The edit form of a node of the subscription in use, above the node list. */
function NodeEditor({ node, form, busy, run, close }: {
  node: NodeSummary;
  form: NodeForm;
  busy: string | null;
  run: Run;
  close: () => void;
}) {
  const [name, setName] = useState(form.name);
  const [server, setServer] = useState(form.server);
  const [port, setPort] = useState(String(form.port || ""));
  const problem = nodeFormProblem(server, port);
  const save = () => void run("node", async () => {
    const result = await desktop.editNode(node.id, name, server, port);
    close();
    return result;
  });
  return (
    <Card title={t("编辑节点")} description={t("改动只保存在本机，更新订阅后恢复为订阅里的内容。")} className="node-editor">
      <Field label={t("名称")}><input value={name} onChange={(event) => setName(event.target.value)} placeholder={node.label} /></Field>
      <Field label={t("服务器地址")}><input value={server} onChange={(event) => setServer(event.target.value)} spellCheck={false} /></Field>
      <Field label={t("端口")}><input value={port} onChange={(event) => setPort(event.target.value)} inputMode="numeric" /></Field>
      {problem && <p className="danger-text">{problem}</p>}
      <div className="button-row">
        <button className="secondary" disabled={Boolean(busy)} onClick={close}>{t("取消")}</button>
        <button className="primary" disabled={Boolean(busy) || Boolean(problem)} onClick={save}>{t("保存")}</button>
      </div>
    </Card>
  );
}


async function copyShareLink(node: NodeSummary) {
  const link = await desktop.shareLink(node.id);
  try {
    await navigator.clipboard.writeText(link);
  } catch {
    // Some webviews refuse the clipboard after an await; show the link instead.
    return { message: link };
  }
  return { message: t("已复制 {node} 的分享链接", { node: node.label }) };
}

function Subscriptions({ snapshot, sources, refreshSources, empty, busy, run, pending, takePending }: {
  snapshot: RuntimeSnapshot | null;
  sources: SubscriptionSummary[];
  refreshSources: () => void;
  empty: boolean;
  busy: string | null;
  run: Run;
  pending?: string | null;
  takePending?: () => void;
}) {
  const [location, setLocation] = useState("");
  useEffect(() => {
    if (!pending) return;
    setLocation(pending);
    takePending?.();
  }, [pending, takePending]);

  const importSource = () => void run("subscription", async () => {
    const result = await desktop.importSubscription(location.trim());
    setLocation("");
    refreshSources();
    return result;
  });

  // Android scans a subscription or share link QR code into the field; "导入" then saves it, as with a pasted link.
  const android = snapshot?.platform === "android";
  const [scanning, setScanning] = useState(false);
  const scan = () => void run("scan", async () => {
    setScanning(true);
    try {
      const outcome = await scanQrCode();
      if ("denied" in outcome) return { message: t("没有相机权限，请在系统设置里允许本应用使用相机") };
      if ("cancelled" in outcome) return { message: t("已取消扫码") };
      setLocation(outcome.content);
      return { message: t("已识别二维码，核对后点“导入”") };
    } finally {
      setScanning(false);
    }
  });

  const refreshDetail = (snapshot?.subscriptionError ? subscriptionFailureLine(snapshot.subscriptionRefreshErrorCode, snapshot.subscriptionError) : undefined)
    ?? (snapshot?.subscriptionLastAttempt
      ? [
        t("上次尝试 {time}", { time: snapshot.subscriptionLastAttempt }),
        t("新增 {count}", { count: snapshot.subscriptionAddedNodeCount }),
        t("移除 {count}", { count: snapshot.subscriptionRemovedNodeCount }),
        t("变更 {count}", { count: snapshot.subscriptionChangedNodeCount }),
        snapshot.subscriptionNextAttempt ? t("下次 {time}", { time: snapshot.subscriptionNextAttempt }) : null,
      ].filter(Boolean).join(" · ")
      : null);

  const form = (
    <div className="import-row">
      <input
        aria-label={t("订阅链接或配置文件路径")}
        placeholder={t("https:// 订阅链接，或本地配置文件路径")}
        value={location}
        onChange={(event) => setLocation(event.target.value)}
        onKeyDown={(event) => { if (event.key === "Enter" && location.trim() && !busy) importSource(); }}
      />
      {android && <button className="secondary" disabled={Boolean(busy)} onClick={scan}><ScanLine size={14} />{t("扫码")}</button>}
      <button className="primary" disabled={Boolean(busy) || !location.trim()} onClick={importSource}><CloudDownload size={14} />{t("导入")}</button>
      {scanning && createPortal(
        <div className="qr-scan-overlay" role="dialog" aria-label={t("扫描二维码")}>
          <div className="qr-scan-frame" />
          <p>{t("把订阅或节点的二维码放进框内")}</p>
          <button className="secondary" onClick={() => void cancelQrScan().catch(() => undefined)}>{t("取消")}</button>
        </div>,
        document.body,
      )}
    </div>
  );

  if (empty && sources.length === 0) {
    return (
      <Card>
        <EmptyState icon={Globe} title={t("暂无节点")} message={t("粘贴订阅链接，或填写本地配置文件的路径。")}>
          {form}
        </EmptyState>
      </Card>
    );
  }

  return (
    <Card
      className="main-pane"
      title={t("订阅")}
      description={refreshDetail ?? t("支持 HTTPS 订阅链接和本地配置文件。")}
      actions={<>
        <button className="link-button" disabled={Boolean(busy) || !snapshot?.subscription} onClick={() => void run("selector", desktop.checkProfileRuntime)}>{t("检查配置")}</button>
        <button className="link-button" disabled={Boolean(busy) || !snapshot?.subscription?.startsWith("http")} onClick={() => void run("subscription", desktop.updateSubscription)}>{t("更新当前订阅")}</button>
      </>}
    >
      {form}
      {sources.length > 0 && (
        <ul className="list sources">
          {sources.map((source) => (
            <SubscriptionRow
              key={source.id}
              source={source}
              canUpdate={source.refreshable && (source.selected || inProcessEngine(snapshot))}
              busy={busy}
              run={run}
              changed={refreshSources}
            />
          ))}
        </ul>
      )}
    </Card>
  );
}

/** One subscription group: its name, nodes, traffic, expiry and update settings. */
function SubscriptionRow({ source, canUpdate, busy, run, changed }: {
  source: SubscriptionSummary;
  canUpdate: boolean;
  busy: string | null;
  run: Run;
  changed: () => void;
}) {
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState(source.name);
  const used = source.upload == null && source.download == null ? undefined : (source.upload ?? 0) + (source.download ?? 0);
  const fraction = usedFraction(used, source.total ?? undefined);
  const then = (action: () => Promise<{ message: string }>) => async () => {
    const result = await action();
    changed();
    return result;
  };
  const saveName = () => {
    setRenaming(false);
    if (name.trim() && name.trim() !== source.name) void run("subscription", then(() => desktop.renameSubscription(source.id, name.trim())));
  };
  const status = [
    source.nodeCount != null ? t("{count} 个节点", { count: source.nodeCount }) : null,
    source.lastSuccessfulUpdate ? t("更新于 {time}", { time: source.lastSuccessfulUpdate }) : source.lastAttempt ? t("尝试于 {time}", { time: source.lastAttempt }) : t("从未更新"),
    source.lastUpdateError ? subscriptionFailureLine(source.errorCode, source.lastUpdateError) : null,
  ].filter(Boolean).join(" · ");
  const traffic = [
    used != null ? (source.total ? t("已用 {used} / {total}", { used: formatBytes(used), total: formatBytes(source.total) }) : t("已用 {used}", { used: formatBytes(used) })) : null,
    source.expire != null ? t("到期 {date}", { date: formatDay(source.expire) }) : null,
  ].filter(Boolean).join(" · ");
  const expired = source.expire != null && source.expire * 1000 < Date.now();

  return (
    <li className="subscription-row">
      <Tile icon={source.refreshable ? CloudDownload : FileText} color={source.refreshable ? "indigo" : "gray"} />
      <span className="list-main">
        {renaming ? (
          <span className="rename-row">
            <input
              aria-label={t("名称")}
              autoFocus
              value={name}
              onChange={(event) => setName(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") saveName();
                if (event.key === "Escape") { setName(source.name); setRenaming(false); }
              }}
            />
            <IconButton label={t("保存")} onClick={saveName}><Check size={15} /></IconButton>
            <IconButton label={t("取消")} onClick={() => { setName(source.name); setRenaming(false); }}><X size={15} /></IconButton>
          </span>
        ) : (
          <strong className="truncate" title={source.location}>{source.name}</strong>
        )}
        <small>{status}</small>
        {traffic && <small className={expired || (fraction ?? 0) > 0.9 ? "danger-text" : undefined}>{traffic}</small>}
        {source.needsReimport && <small className="warning-text">{!source.refreshable ? t("由旧版本导入：重新导入后证书固定才会生效") : canUpdate ? t("由旧版本导入：更新后证书固定才会生效") : t("由旧版本导入：使用此订阅后证书固定才会生效")}</small>}
        {fraction != null && (
          <span className="usage-bar" role="progressbar" aria-label={t("已用流量")} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(fraction * 100)}>
            <span className={fraction > 0.9 ? "danger" : undefined} style={{ width: `${fraction * 100}%` }} />
          </span>
        )}
      </span>
      {source.refreshable && (
        <Select
          label={t("自动更新")}
          value={String(source.updateIntervalHours)}
          disabled={Boolean(busy)}
          onChange={(hours) => void run("subscription", then(() => desktop.setSubscriptionInterval(source.id, Number(hours))))}
          choices={updateIntervalChoices(source.updateIntervalHours).map((hours) => ({ value: String(hours), label: updateIntervalLabel(hours) }))}
        />
      )}
      <StatusPill label={source.selected ? t("使用中") : source.result === "failed" ? t("更新失败") : t("已保存")} tone={source.selected ? "success" : source.result === "failed" ? "danger" : "neutral"} />
      {canUpdate && (
        <IconButton label={t("更新 {name}", { name: source.name })} disabled={Boolean(busy)} onClick={() => void run("subscription", then(() => desktop.updateSubscriptionSource(source.id)))}>
          <RefreshCw size={15} />
        </IconButton>
      )}
      {!source.selected && (
        <button className="secondary compact" disabled={Boolean(busy)} onClick={() => void run("subscription", then(() => desktop.selectSubscription(source.id)))}>{t("使用")}</button>
      )}
      <MoreMenu
        label={t("更多")}
        disabled={Boolean(busy)}
        items={[
          { label: t("重命名"), icon: Pencil, onSelect: () => { setName(source.name); setRenaming(true); } },
          {
            label: t("删除订阅"),
            icon: Trash2,
            danger: true,
            confirm: t("确认删除 {name}", { name: source.name }),
            onSelect: () => void run("subscription", then(() => desktop.removeSubscription(source.id))),
          },
        ]}
      />
    </li>
  );
}
