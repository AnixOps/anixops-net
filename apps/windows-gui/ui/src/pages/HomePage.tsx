import { Activity, AppWindow, ChevronRight, ChevronsUpDown, CloudDownload, Globe, Layers, ListTree, Play, RefreshCw, RotateCw, Search, Timer, Waypoints, Zap } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import type { Page } from "../App";
import { ConnectionSnapshot, desktop, NativeGroupSummary, NodeDelay, NodeSummary, RuntimeSnapshot, SubscriptionSummary } from "../bridge";
import { connectionName, formatRate } from "../activity-format";
import { BrandMark } from "../brand";
import { chartCeiling, chartPoints, colorFor, daysLeft, siteOf, topUsage, UsageRow } from "../dashboard";
import { liveEngine, usePolling } from "../live";
import { availableRoutingModes, ConnectPhase, connectionTitle, dnsPresetLabel, groupMemberLabel, protocolAbbreviation, routingLabel } from "../labels";
import { delayLabel, delayTone } from "../node-filter";
import { formatBytes, formatDay, updateIntervalLabel, usedFraction } from "../subscription-format";
import { useTraffic } from "../traffic-feed";
import { LetterTile, Panel, Run, Segmented, StatusDot, Switch } from "../ui";
import { t } from "../i18n";

export type Delays = Record<string, NodeDelay>;
export type SetDelays = (update: (current: Delays) => Delays) => void;

/**
 * The overview, a dashboard in the manner of Surge: the connection with the node and mode, the subscription's
 * allowance, live traffic, delays through the node, and what is using the connection right now.
 */
export function HomePage({ snapshot, nodes, phase, busy, run, toggle, goTo, delays, setDelays }: {
  snapshot: RuntimeSnapshot | null;
  nodes: NodeSummary[];
  phase: ConnectPhase;
  busy: string | null;
  run: Run;
  toggle: () => void;
  goTo: (page: Page) => void;
  delays: Delays;
  setDelays: SetDelays;
}) {
  const live = liveEngine(snapshot);
  const [list] = usePolling(desktop.connections, live);
  const connections = list ? [...list.active, ...list.closed] : [];
  const [groups, setGroups] = useState<NativeGroupSummary[]>([]);
  const [sources, setSources] = useState<SubscriptionSummary[]>([]);
  useEffect(() => {
    void desktop.nativeGroups().then(setGroups).catch(() => setGroups([]));
    void desktop.subscriptions().then(setSources).catch(() => setSources([]));
  }, [snapshot?.subscription, snapshot?.subscriptionLastUpdated]);

  const waiting = phase === "on" && !live ? t("Windows 服务模式下不提供实时数据") : t("连接后显示");
  const programs = topUsage(connections, (item) => item.process?.split(/[\\/]/).pop(), 5);

  return (
    <div className="dash">
      <Hero snapshot={snapshot} nodes={nodes} phase={phase} busy={busy} run={run} toggle={toggle} goTo={goTo} delays={delays} />
      <SubscriptionPanel snapshot={snapshot} source={sources.find((item) => item.selected) ?? sources[0]} nodeCount={nodes.length} busy={busy} run={run} goTo={goTo} />
      <TrafficPanel waiting={waiting} goTo={goTo} />
      <DelayPanel snapshot={snapshot} busy={busy} run={run} goTo={goTo} setDelays={setDelays} />
      <UsagePanel
        icon={ListTree}
        title={t("流量最多的网站")}
        rows={topUsage(connections, (item) => siteOf(connectionName(item)), 5)}
        detail={(row) => [row.via, t("{count} 个连接", { count: row.connections })].join(" · ")}
        waiting={waiting}
        live={live}
      />
      {programs.length > 0 || !live
        ? <UsagePanel icon={AppWindow} title={t("正在联网的程序")} rows={programs} detail={(row) => t("{count} 个连接", { count: row.connections })} waiting={waiting} live={live} />
        : <UsagePanel
          icon={Waypoints}
          title={t("出口分布")}
          rows={topUsage(connections, (item) => item.chain[item.chain.length - 1] ?? item.outbound, 5)}
          detail={(row) => t("{count} 个连接", { count: row.connections })}
          waiting={waiting}
          live={live}
        />}
      {groups.length > 0
        ? <GroupsPanel groups={groups} setGroups={setGroups} busy={busy} run={run} goTo={goTo} />
        : <RecentPanel connections={list ? [...list.active].reverse().concat(list.closed) : []} waiting={waiting} live={live} goTo={goTo} />}
    </div>
  );
}

function Hero({ snapshot, nodes, phase, busy, run, toggle, goTo, delays }: {
  snapshot: RuntimeSnapshot | null;
  nodes: NodeSummary[];
  phase: ConnectPhase;
  busy: string | null;
  run: Run;
  toggle: () => void;
  goTo: (page: Page) => void;
  delays: Delays;
}) {
  const { traffic } = useTraffic();
  const node = nodes.find((item) => item.id === snapshot?.selectedNode);
  const nodeName = node?.label ?? snapshot?.selectedNode;
  const unconfigured = snapshot !== null && !snapshot.subscription && nodes.length === 0;
  const moving = phase === "connecting" || phase === "disconnecting";
  const delay = node ? delays[node.id] : undefined;

  const subtitle = (() => {
    if (unconfigured) return t("先导入订阅或配置文件");
    switch (phase) {
      case "on":
        return [
          snapshot?.tunModeEnabled ? t("TUN 模式") : snapshot?.runtimeMode === "service" ? t("Windows 服务") : t("系统代理"),
          routingLabel(snapshot?.routingMode),
          traffic ? t("{count} 个连接", { count: traffic.active ?? 0 }) : null,
        ].filter(Boolean).join(" · ");
      case "connecting":
        return t("正在建立安全隧道");
      case "disconnecting":
        return t("正在恢复系统代理设置");
      case "failed":
        return snapshot?.configurationError ?? snapshot?.lastError ?? t("查看“活动”了解原因");
      case "loading":
        return " ";
      default:
        return nodeName ? t("打开开关即可连接 · Ctrl+Shift+C") : t("先在“代理”里选择一个节点");
    }
  })();
  const dotTone = phase === "on" ? "success" : phase === "failed" ? "danger" : moving ? "warning" : "neutral";
  const sections = [...new Set(nodes.map((item) => item.group ?? ""))];

  return (
    <Panel className={`span-8 hero phase-${phase}`}>
      <div className="hero-main">
        <span className="hero-mark"><BrandMark size={52} /></span>
        <div className="hero-text" aria-live="polite">
          <h2><StatusDot tone={dotTone} />{connectionTitle(snapshot?.connection, phase)}</h2>
          <p className={phase === "failed" ? "danger-text" : undefined}>{subtitle}</p>
        </div>
        {unconfigured
          ? <button className="primary" onClick={() => goTo("proxies")}><CloudDownload size={15} />{t("导入订阅")}</button>
          : <Switch
            label={phase === "on" ? t("断开，当前已连接") : t("连接，当前未连接")}
            size="large"
            tone="go"
            checked={phase === "on" || phase === "connecting"}
            busy={moving}
            disabled={phase === "loading" || moving}
            onChange={toggle}
          />}
      </div>
      <div className="hero-controls">
        <label className="node-select" title={t("切换节点")}>
          <Globe size={13} aria-hidden="true" />
          <span className="truncate">{nodeName ?? t("未选择")}</span>
          {node && <span className="protocol-badge">{protocolAbbreviation(node.protocol)}</span>}
          {delay && <span className={`delay ${delayTone(delay)}`}>{delayLabel(delay)}</span>}
          <ChevronsUpDown size={12} aria-hidden="true" />
          <select
            aria-label={t("切换节点")}
            value={node?.id ?? ""}
            disabled={Boolean(busy) || nodes.length === 0}
            onChange={(event) => void run("switch", () => desktop.switchNode(event.target.value))}
          >
            {!node && <option value="" disabled>{t("未选择")}</option>}
            {sections.length > 1
              ? sections.map((section) => (
                <optgroup key={section} label={section || t("节点")}>
                  {nodes.filter((item) => (item.group ?? "") === section).map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}
                </optgroup>
              ))
              : nodes.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}
          </select>
        </label>
        <Segmented
          label={t("分流模式")}
          className="fill"
          options={availableRoutingModes(snapshot).map((item) => ({ value: item.value, label: t(item.label), hint: t(item.hint) }))}
          value={snapshot?.routingMode}
          disabled={Boolean(busy) || !snapshot}
          onChange={(value) => void run("routing", () => desktop.setRoutingMode(value))}
        />
      </div>
    </Panel>
  );
}

/** The subscription in use: how much of its allowance is gone, when it expires and when it was last updated. */
function SubscriptionPanel({ snapshot, source, nodeCount, busy, run, goTo }: {
  snapshot: RuntimeSnapshot | null;
  source: SubscriptionSummary | undefined;
  nodeCount: number;
  busy: string | null;
  run: Run;
  goTo: (page: Page) => void;
}) {
  if (!source) {
    return (
      <Panel className="span-4" icon={CloudDownload} title={t("订阅")}>
        <div className="panel-empty">
          <p>{t("还没有订阅。导入后节点会出现在“代理”里。")}</p>
          <button className="secondary compact" onClick={() => goTo("proxies")}>{t("导入订阅")}</button>
        </div>
      </Panel>
    );
  }
  const used = source.upload == null && source.download == null ? undefined : (source.upload ?? 0) + (source.download ?? 0);
  const fraction = usedFraction(used, source.total ?? undefined);
  const left = source.expire != null ? daysLeft(source.expire, Date.now()) : undefined;
  const nodesLabel = t("{count} 个节点", { count: source.nodeCount ?? nodeCount });
  const updated = source.lastSuccessfulUpdate ?? snapshot?.subscriptionLastUpdated;

  return (
    <Panel
      className="span-4"
      icon={CloudDownload}
      title={<span className="truncate">{source.name}</span>}
      action={source.refreshable && (
        <button className="link-button" disabled={Boolean(busy)} onClick={() => void run("subscription", desktop.updateSubscription)}>{t("更新")}<RefreshCw size={11} /></button>
      )}
    >
      <div className="allowance">
        <Ring fraction={fraction} />
        <div className="allowance-text">
          {used != null
            ? <strong>{source.total ? [formatBytes(used), formatBytes(source.total)].join(" / ") : formatBytes(used)}</strong>
            : <strong>{nodesLabel}</strong>}
          <small>
            {used != null && source.total
              ? [t("剩余 {left}", { left: formatBytes(Math.max(0, source.total - used)) }), nodesLabel].join(" · ")
              : used != null ? [t("已用流量"), nodesLabel].join(" · ") : t("订阅没有提供流量信息")}
          </small>
        </div>
      </div>
      <dl className="facts">
        <div>
          <dt>{t("到期")}</dt>
          <dd className={left != null && left < 3 ? "danger-text" : undefined}>
            {source.expire == null ? t("长期有效") : left! < 0 ? t("已过期") : [formatDay(source.expire), t("还有 {days} 天", { days: left! })].join(" · ")}
          </dd>
        </div>
        <div>
          <dt>{t("上次更新")}</dt>
          <dd className="truncate">{[updated ?? t("尚未成功更新"), source.refreshable ? updateIntervalLabel(source.updateIntervalHours) : t("本地文件")].join(" · ")}</dd>
        </div>
      </dl>
    </Panel>
  );
}

/** A ring filled to `fraction` with the percentage inside; an empty ring when the share is unknown. */
function Ring({ fraction }: { fraction: number | undefined }) {
  const length = 2 * Math.PI * 24;
  const high = (fraction ?? 0) > 0.9;
  return (
    <svg className="ring" width="60" height="60" viewBox="0 0 60 60" role="img" aria-label={fraction == null ? t("已用流量") : t("已用 {percent}%", { percent: Math.round(fraction * 100) })}>
      <circle cx="30" cy="30" r="24" fill="none" stroke="var(--fill-2)" strokeWidth="7" />
      {fraction != null && fraction > 0 && (
        <circle cx="30" cy="30" r="24" fill="none" stroke={high ? "var(--danger)" : "var(--accent)"} strokeWidth="7" strokeLinecap="round" strokeDasharray={`${length * fraction} ${length}`} transform="rotate(-90 30 30)" />
      )}
      <text x="30" y="34" textAnchor="middle" fontSize="12" fontWeight="600" fill="currentColor">{fraction == null ? "—" : `${Math.round(fraction * 100)}%`}</text>
    </svg>
  );
}

/** Live rates, totals and the last minute as a chart. */
function TrafficPanel({ waiting, goTo }: { waiting: string; goTo: (page: Page) => void }) {
  const { traffic, up, down, peak } = useTraffic();
  const ceiling = chartCeiling(Math.max(...down, ...up, 0));
  return (
    <Panel
      className="span-8"
      icon={Activity}
      title={t("实时流量")}
      action={<button className="link-button" onClick={() => goTo("activity")}>{t("连接记录")}<ChevronRight size={12} /></button>}
    >
      <div className="traffic-top">
        <Rate label={t("下载")} color="var(--chart-1)" value={traffic ? formatRate(traffic.download_rate) : null} />
        <Rate label={t("上传")} color="var(--chart-2)" value={traffic ? formatRate(traffic.upload_rate) : null} />
        {traffic && (
          <dl className="traffic-totals">
            <div><dt>{t("本次")}</dt><dd>↓ {formatBytes(traffic.download_total)}</dd><dd>↑ {formatBytes(traffic.upload_total)}</dd></div>
            <div><dt>{t("活跃连接")}</dt><dd>{traffic.active ?? 0}</dd><dt>{t("峰值")}</dt><dd>{formatRate(peak)}</dd></div>
          </dl>
        )}
      </div>
      <div className="chart-box">
        <svg className="chart" viewBox="0 0 600 120" preserveAspectRatio="none" aria-hidden="true">
          <defs>
            <linearGradient id="chart-down" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stopColor="var(--chart-1)" stopOpacity=".28" /><stop offset="1" stopColor="var(--chart-1)" stopOpacity="0" /></linearGradient>
            <linearGradient id="chart-up" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stopColor="var(--chart-2)" stopOpacity=".24" /><stop offset="1" stopColor="var(--chart-2)" stopOpacity="0" /></linearGradient>
          </defs>
          {[30, 60, 90].map((y) => <line key={y} x1="0" x2="600" y1={y} y2={y} stroke="var(--separator)" strokeDasharray="2 4" vectorEffect="non-scaling-stroke" />)}
          <Area values={down} ceiling={ceiling} color="var(--chart-1)" fill="url(#chart-down)" />
          <Area values={up} ceiling={ceiling} color="var(--chart-2)" fill="url(#chart-up)" />
        </svg>
        {traffic ? <span className="chart-scale">{formatRate(ceiling)}</span> : <span className="chart-waiting">{waiting}</span>}
      </div>
      <div className="chart-axis"><span>{t("60 秒前")}</span><span>{t("30 秒")}</span><span>{t("现在")}</span></div>
    </Panel>
  );
}

function Rate({ label, color, value }: { label: string; color: string; value: string | null }) {
  const [amount, unit] = value ? value.split(" ") : ["—", ""];
  return (
    <div className="rate-big">
      <small><i style={{ background: color }} />{label}</small>
      <strong>{amount}{unit && <em>{unit}</em>}</strong>
    </div>
  );
}

function Area({ values, ceiling, color, fill }: { values: number[]; ceiling: number; color: string; fill: string }) {
  if (values.length < 2) return null;
  const points = chartPoints(values, 600, 120, ceiling);
  const line = `M${points.map(([x, y]) => `${x.toFixed(1)} ${y.toFixed(1)}`).join(" L")}`;
  return (
    <>
      <path d={`${line} L600 120 L${points[0][0].toFixed(1)} 120 Z`} fill={fill} />
      <path d={line} fill="none" stroke={color} strokeWidth="1.75" strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
    </>
  );
}

const delayTargets: Array<{ key: string; label: string; icon: LucideIcon; url: string }> = [
  { key: "node", label: "节点", icon: Globe, url: "https://www.gstatic.com/generate_204" },
  { key: "google", label: "Google", icon: Search, url: "https://www.google.com/generate_204" },
  { key: "youtube", label: "YouTube", icon: Play, url: "https://www.youtube.com/generate_204" },
  { key: "cloudflare", label: "Cloudflare", icon: Zap, url: "https://cp.cloudflare.com/generate_204" },
];

// The last results, kept while the window lives so coming back to the overview does not test again at once.
let delayCache: { node: string; at: number; results: Record<string, NodeDelay> } | null = null;
const delayMaxAge = 120_000;

/** Delays through the current node to a few well-known sites, and the switches for how traffic is taken over. */
function DelayPanel({ snapshot, busy, run, goTo, setDelays }: {
  snapshot: RuntimeSnapshot | null;
  busy: string | null;
  run: Run;
  goTo: (page: Page) => void;
  setDelays: SetDelays;
}) {
  const nodeId = snapshot?.selectedNode;
  const [results, setResults] = useState<Record<string, NodeDelay>>(() => (delayCache && delayCache.node === nodeId ? delayCache.results : {}));
  const [testing, setTesting] = useState(false);

  const test = useCallback(async (node: string) => {
    setTesting(true);
    const answers = await Promise.all(delayTargets.map((target) => desktop.testDelays([node], target.url)
      .then((list): NodeDelay => list[0] ?? { nodeId: node })
      .catch((error): NodeDelay => ({ nodeId: node, error: String(error) }))));
    const next = Object.fromEntries(delayTargets.map((target, index) => [target.key, answers[index]]));
    delayCache = { node, at: Date.now(), results: next };
    setResults(next);
    setDelays((current) => ({ ...current, [node]: next.node }));
    setTesting(false);
  }, [setDelays]);

  useEffect(() => {
    if (!nodeId) return;
    if (delayCache && delayCache.node === nodeId && Date.now() - delayCache.at < delayMaxAge) {
      setResults(delayCache.results);
      return;
    }
    setResults({});
    void test(nodeId);
  }, [nodeId, test]);

  const windows = snapshot !== null && snapshot.platform !== "linux";
  const decrypting = Boolean(snapshot?.httpsDecryptionEnabled);

  return (
    <Panel
      className="span-4"
      icon={Timer}
      title={t("延迟")}
      action={<button className="link-button" disabled={!nodeId || testing} onClick={() => nodeId && void test(nodeId)}>{t("重新测试")}<RotateCw size={11} className={testing ? "spin" : undefined} /></button>}
    >
      <div className="delays">
        {delayTargets.map(({ key, label, icon: Icon }) => {
          const result = results[key];
          const ms = result?.delayMs;
          return (
            <div className="delay-row" key={key} title={result?.error ?? undefined}>
              <span className="delay-name"><Icon size={13} aria-hidden="true" />{t(label)}</span>
              <span className="delay-bar"><span className={result && ms != null ? delayTone(result) : undefined} style={{ width: `${ms != null ? Math.max(4, Math.min(100, ms / 4)) : 0}%` }} /></span>
              <span className={result && ms == null ? "delay-value failed" : "delay-value"}>
                {!result ? "—" : ms == null ? t("超时") : <>{ms} <em>ms</em></>}
              </span>
            </div>
          );
        })}
      </div>
      <dl className="facts">
        {windows && snapshot?.runtimeMode !== "service" && (
          <div>
            <dt>{t("TUN 模式")}</dt>
            <dd><Switch label={t("TUN 模式")} checked={Boolean(snapshot?.tunModeEnabled)} disabled={Boolean(busy)} onChange={() => void run("TUN", () => desktop.setTunMode(!snapshot?.tunModeEnabled))} /></dd>
          </div>
        )}
        {windows && (
          <div>
            <dt><button className="plain-link" onClick={() => goTo("extensions")}>{t("HTTPS 解密")}<ChevronRight size={12} /></button></dt>
            <dd><Switch label={t("HTTPS 解密")} checked={decrypting} disabled={Boolean(busy)} onChange={() => void run("mitm", decrypting ? desktop.disableHttpsMitm : desktop.enableHttpsMitm)} /></dd>
          </div>
        )}
        <div>
          <dt><button className="plain-link" onClick={() => goTo("rules")}>DNS<ChevronRight size={12} /></button></dt>
          <dd>{dnsPresetLabel(snapshot?.dnsPreset)}</dd>
        </div>
      </dl>
    </Panel>
  );
}

/** A short ranked list: sites, programs or outbounds with what they moved. */
function UsagePanel({ icon, title, rows, detail, waiting, live }: {
  icon: LucideIcon;
  title: string;
  rows: UsageRow[];
  detail: (row: UsageRow) => string;
  waiting: string;
  live: boolean;
}) {
  return (
    <Panel className="span-4 list-panel" icon={icon} title={title}>
      {rows.length === 0
        ? <p className="panel-empty">{live ? t("还没有连接") : waiting}</p>
        : rows.map((row) => (
          <div className="usage-row" key={row.key}>
            <LetterTile name={row.key} color={colorFor(row.key)} />
            <span className="usage-text"><strong>{row.key}</strong><small>{detail(row)}</small></span>
            <span className="usage-value">{formatBytes(row.bytes)}</span>
          </div>
        ))}
    </Panel>
  );
}

/** The first policy groups with their current choice; manual ones can be switched here. */
function GroupsPanel({ groups, setGroups, busy, run, goTo }: {
  groups: NativeGroupSummary[];
  setGroups: (update: (current: NativeGroupSummary[]) => NativeGroupSummary[]) => void;
  busy: string | null;
  run: Run;
  goTo: (page: Page) => void;
}) {
  return (
    <Panel
      className="span-4 list-panel"
      icon={Layers}
      title={t("策略组")}
      action={<button className="link-button" onClick={() => goTo("proxies")}>{t("全部")}<ChevronRight size={12} /></button>}
    >
      {groups.slice(0, 5).map((group) => (
        <div className="usage-row" key={group.tag}>
          <span className="usage-text"><strong>{group.tag}</strong></span>
          {group.groupType === "selector"
            ? (
              <select
                className="pop-up"
                aria-label={group.tag}
                value={group.selected ?? ""}
                disabled={Boolean(busy)}
                onChange={(event) => {
                  const outbound = event.target.value;
                  void run("native selector", async () => {
                    const result = await desktop.selectNativeGroupOutbound(group.tag, outbound);
                    setGroups((current) => current.map((item) => item.tag === group.tag ? { ...item, selected: outbound } : item));
                    return result;
                  });
                }}
              >
                {group.selected === undefined && <option value="" disabled>{t("未选择")}</option>}
                {group.outbounds.map((outbound) => <option key={outbound} value={outbound}>{groupMemberLabel(group, outbound)}</option>)}
              </select>
            )
            : <><span className="usage-value truncate">{groupMemberLabel(group, group.selected) ?? "—"}</span><span className="chip quiet">{group.groupType === "urltest" ? t("自动") : group.groupType}</span></>}
        </div>
      ))}
    </Panel>
  );
}

/** The newest connections, for profiles without policy groups. */
function RecentPanel({ connections, waiting, live, goTo }: { connections: ConnectionSnapshot[]; waiting: string; live: boolean; goTo: (page: Page) => void }) {
  return (
    <Panel
      className="span-4 list-panel"
      icon={Layers}
      title={t("最近的连接")}
      action={<button className="link-button" onClick={() => goTo("activity")}>{t("全部")}<ChevronRight size={12} /></button>}
    >
      {connections.length === 0
        ? <p className="panel-empty">{live ? t("还没有连接") : waiting}</p>
        : connections.slice(0, 5).map((connection) => (
          <div className="usage-row" key={connection.id}>
            <span className="usage-text"><strong>{connectionName(connection)}</strong><small>{[connection.rule, connection.chain[connection.chain.length - 1] ?? connection.outbound].filter(Boolean).join(" · ")}</small></span>
            <span className="usage-value">{formatBytes(connection.upload + connection.download)}</span>
          </div>
        ))}
    </Panel>
  );
}
