import { Activity, Check, CloudDownload, Globe, Pencil, RefreshCw, Search, Trash2, X, Zap } from "lucide-react";
import { useEffect, useState } from "react";
import { desktop, NodeSummary, RuntimeSnapshot, SubscriptionSummary } from "../bridge";
import { protocolAbbreviation } from "../labels";
import { filterNodes } from "../node-filter";
import { Card, EmptyState, IconButton, Run, StatusPill } from "../ui";
import { t } from "../i18n";
import { formatBytes, formatDay, updateIntervalChoices, updateIntervalLabel, usedFraction } from "../subscription-format";

export function ProxiesPage({ snapshot, nodes, busy, run }: {
  snapshot: RuntimeSnapshot | null;
  nodes: NodeSummary[];
  busy: string | null;
  run: Run;
}) {
  const [query, setQuery] = useState("");
  const [protocol, setProtocol] = useState("All");
  const protocols = ["All", ...new Set(nodes.map((node) => node.protocol))];
  const visible = filterNodes(nodes, query, protocol);

  return (
    <div className="page-stack">
      {nodes.length > 0 && (
        <Card
          title={t("节点")}
          description={t("切换节点会先校验生成的配置，再应用到当前连接。")}
          actions={<button className="secondary" disabled={Boolean(busy)} onClick={() => void run("fastest node", desktop.selectFastestNode)}><Zap size={15} />{t("选择最快")}</button>}
        >
          <div className="toolbar">
            <label className="search">
              <Search size={15} aria-hidden="true" />
              <input aria-label={t("搜索节点")} placeholder={t("搜索节点")} value={query} onChange={(event) => setQuery(event.target.value)} />
            </label>
            <div className="chips" role="radiogroup" aria-label={t("按协议筛选")}>
              {protocols.map((value) => (
                <button key={value} role="radio" aria-checked={protocol === value} className={`chip-button ${protocol === value ? "selected" : ""}`} onClick={() => setProtocol(value)}>
                  {value === "All" ? t("全部") : protocolAbbreviation(value)}
                </button>
              ))}
            </div>
            <span className="count">{visible.length} / {nodes.length}</span>
          </div>
          <div className="node-grid">
            {visible.map((node) => (
              <div className={`node-card ${node.selected ? "selected" : ""}`} key={node.id}>
                <button
                  className="node-pick"
                  aria-pressed={node.selected}
                  disabled={Boolean(busy) || node.selected}
                  onClick={() => void run("switch", () => desktop.switchNode(node.id))}
                >
                  <span className="node-name">{node.label}</span>
                  <span className="node-meta"><span className="protocol-badge">{protocolAbbreviation(node.protocol)}</span>{node.selected && <span className="current-dot" aria-label={t("当前节点")} />}</span>
                </button>
                <IconButton label={t("测试 {node} 的延迟", { node: node.label })} disabled={Boolean(busy)} onClick={() => void run("delay", () => desktop.testDelay(node.id, delayUrl))}>
                  <Activity size={15} />
                </IconButton>
              </div>
            ))}
          </div>
          {visible.length === 0 && <p className="muted center">{t("没有符合条件的节点。")}</p>}
        </Card>
      )}
      <Subscriptions snapshot={snapshot} empty={nodes.length === 0} busy={busy} run={run} />
    </div>
  );
}

const delayUrl = "https://www.gstatic.com/generate_204";

function Subscriptions({ snapshot, empty, busy, run }: { snapshot: RuntimeSnapshot | null; empty: boolean; busy: string | null; run: Run }) {
  const [location, setLocation] = useState("");
  const [sources, setSources] = useState<SubscriptionSummary[]>([]);
  const refreshSources = () => void desktop.subscriptions().then(setSources).catch(() => setSources([]));
  useEffect(refreshSources, [snapshot?.subscription]);

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
      <button className="primary" disabled={Boolean(busy) || !location.trim()} onClick={importSource}><CloudDownload size={16} />{t("导入")}</button>
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
      title={t("订阅")}
      description={refreshDetail ?? t("支持 HTTPS 订阅链接和本地配置文件。")}
      actions={<>
        <button className="secondary" disabled={Boolean(busy) || !snapshot?.subscription?.startsWith("http")} onClick={() => void run("subscription", desktop.updateSubscription)}>{t("更新当前订阅")}</button>
        <button className="secondary" disabled={Boolean(busy) || !snapshot?.subscription} onClick={() => void run("selector", desktop.checkProfileRuntime)}>{t("检查配置")}</button>
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
      <CloudDownload size={16} className="list-icon" />
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
          className="interval-select"
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
      <IconButton label={t("重命名 {name}", { name: source.name })} disabled={Boolean(busy)} onClick={() => { setName(source.name); setRenaming(true); }}>
        <Pencil size={15} />
      </IconButton>
      <button className="secondary compact" disabled={Boolean(busy) || source.selected} onClick={() => void run("subscription", then(() => desktop.selectSubscription(source.id)))}>{t("使用")}</button>
      <IconButton label={t("删除 {name}", { name: source.name })} disabled={Boolean(busy)} onClick={() => void run("subscription", then(() => desktop.removeSubscription(source.id)))}>
        <Trash2 size={15} />
      </IconButton>
    </li>
  );
}
