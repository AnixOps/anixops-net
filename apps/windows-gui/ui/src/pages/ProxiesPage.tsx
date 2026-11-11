import { Activity, Bot, Check, CloudDownload, FileText, Gauge, Globe, Layers, Pencil, Plus, RefreshCw, Search, Share2, Trash2, X, Zap } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { desktop, NativeGroupSummary, NodeDelay, NodeSummary, RuntimeSnapshot, SubscriptionSummary } from "../bridge";
import { memberNode } from "../dashboard";
import { groupMemberLabel, protocolAbbreviation } from "../labels";
import { delayLabel, delayTone, filterNodes, groupNodes, NodeSort, sortNodes } from "../node-filter";
import { Card, EmptyState, IconButton, MoreMenu, PageTools, Panel, Run, StatusPill, Tile, TileColor } from "../ui";
import { t } from "../i18n";
import { formatBytes, formatDay, updateIntervalChoices, updateIntervalLabel, usedFraction } from "../subscription-format";
import type { Delays, SetDelays } from "./HomePage";

/** What the right pane shows: every node, one policy group's members, or the subscriptions. */
type View = { kind: "nodes" } | { kind: "group"; tag: string } | { kind: "subscriptions" };

/**
 * Two panes: on the left the node list, the profile's policy groups and the subscriptions; on the right the nodes
 * of the chosen one as cards colored by delay, or the subscription settings.
 */
export function ProxiesPage({ snapshot, nodes, busy, run, delays, setDelays }: {
  snapshot: RuntimeSnapshot | null;
  nodes: NodeSummary[];
  busy: string | null;
  run: Run;
  delays: Delays;
  setDelays: SetDelays;
}) {
  const [view, setView] = useState<View>({ kind: "nodes" });
  const [query, setQuery] = useState("");
  const [protocol, setProtocol] = useState("All");
  const [sort, setSort] = useState<NodeSort>("default");
  const [groups, setGroups] = useState<NativeGroupSummary[]>([]);
  const [sources, setSources] = useState<SubscriptionSummary[]>([]);
  const refreshSources = useCallback(() => void desktop.subscriptions().then(setSources).catch(() => setSources([])), []);
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

  if (nodes.length === 0 && sources.length === 0) {
    return <Subscriptions snapshot={snapshot} sources={sources} refreshSources={refreshSources} empty busy={busy} run={run} />;
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
        <button className="secondary" disabled={Boolean(busy) || testable.length === 0} onClick={() => testDelays(testable)}><Gauge size={14} />{t("全部测速")}</button>
        <button className="primary" disabled={Boolean(busy) || nodes.length === 0} onClick={() => void run("fastest node", desktop.selectFastestNode)}><Zap size={14} />{t("选择最快")}</button>
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

      {shown.kind === "subscriptions" && <Subscriptions snapshot={snapshot} sources={sources} refreshSources={refreshSources} empty={false} busy={busy} run={run} />}

      {shown.kind === "nodes" && (
        <Panel
          className="main-pane"
          title={<>{t("全部节点")}<span className="count">{[t("{count} 个节点", { count: visible.length }), available > 0 ? t("{count} 个可用", { count: available }) : null].filter(Boolean).join(" · ")}</span></>}
          action={<>
            <select className="pop-up" aria-label={t("按协议筛选")} value={protocol} onChange={(event) => setProtocol(event.target.value)}>
              {protocols.map((value) => <option key={value} value={value}>{value === "All" ? t("全部协议") : protocolAbbreviation(value)}</option>)}
            </select>
            <select className="pop-up" aria-label={t("节点排序")} value={sort} onChange={(event) => setSort(event.target.value as NodeSort)}>
              <option value="default">{t("默认顺序")}</option>
              <option value="name">{t("按名称")}</option>
              <option value="delay">{t("按延迟")}</option>
            </select>
          </>}
        >
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
                    onShare={() => void run("share", () => copyShareLink(node))}
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
function NodeCard({ label, protocol, delay, selected, disabled, onPick, onTest, onShare }: {
  label: string;
  protocol?: string;
  delay?: NodeDelay;
  selected: boolean;
  disabled: boolean;
  onPick?: () => void;
  onTest?: () => void;
  onShare?: () => void;
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
        {onShare && <IconButton label={t("复制 {node} 的分享链接", { node: label })} disabled={disabled} onClick={onShare}><Share2 size={13} /></IconButton>}
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

function Subscriptions({ snapshot, sources, refreshSources, empty, busy, run }: {
  snapshot: RuntimeSnapshot | null;
  sources: SubscriptionSummary[];
  refreshSources: () => void;
  empty: boolean;
  busy: string | null;
  run: Run;
}) {
  const [location, setLocation] = useState("");

  const importSource = () => void run("subscription", async () => {
    const result = await desktop.importSubscription(location.trim());
    setLocation("");
    refreshSources();
    return result;
  });

  const refreshDetail = snapshot?.subscriptionError
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
      <button className="primary" disabled={Boolean(busy) || !location.trim()} onClick={importSource}><CloudDownload size={14} />{t("导入")}</button>
    </div>
  );

  if (empty && sources.length === 0) {
    return (
      <Card>
        <EmptyState icon={Globe} title={t("还没有节点")} message={t("粘贴订阅链接，或填写本地配置文件的路径。导入后可以在这里切换节点和测速。")}>
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
              canUpdate={source.refreshable && (source.selected || snapshot?.platform === "linux")}
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
    source.lastUpdateError,
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
        {fraction != null && (
          <span className="usage-bar" role="progressbar" aria-label={t("已用流量")} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(fraction * 100)}>
            <span className={fraction > 0.9 ? "danger" : undefined} style={{ width: `${fraction * 100}%` }} />
          </span>
        )}
      </span>
      {source.refreshable && (
        <select
          className="pop-up"
          aria-label={t("自动更新")}
          value={source.updateIntervalHours}
          disabled={Boolean(busy)}
          onChange={(event) => void run("subscription", then(() => desktop.setSubscriptionInterval(source.id, Number(event.target.value))))}
        >
          {updateIntervalChoices(source.updateIntervalHours).map((hours) => <option key={hours} value={hours}>{updateIntervalLabel(hours)}</option>)}
        </select>
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
