import { ArrowDown, ArrowUp, ClipboardList, ListTree, Search, X } from "lucide-react";
import { useState } from "react";
import { ConnectionSnapshot, desktop, RuntimeSnapshot } from "../bridge";
import { connectionDetail, connectionName, formatRate, matchesConnection } from "../activity-format";
import { liveEngine, usePolling } from "../live";
import { formatBytes } from "../subscription-format";
import { Card, Detail, EmptyState, IconButton, Run, Segmented } from "../ui";
import { t } from "../i18n";

export function ActivityPage({ snapshot, busy, run }: { snapshot: RuntimeSnapshot | null; busy: string | null; run: Run }) {
  const refresh = snapshot?.subscriptionLastAttempt
    ? [snapshot.subscriptionRefreshResult || t("等待中"), snapshot.subscriptionLastAttempt, snapshot.subscriptionRefreshErrorCode].filter(Boolean).join(" · ")
    : t("还没有自动更新记录");

  return (
    <div className="page-stack">
      <Card title={t("当前会话")} description={t("只有独立核实过的运行状态才会显示为已连接。")}>
        <dl className="details">
          <Detail term={t("运行状态")} value={snapshot?.connectionLabel ?? t("读取中")} />
          <Detail term={t("最近错误")} value={snapshot?.lastError ?? t("无")} />
          <Detail term={t("配置校验")} value={snapshot?.configurationError ?? t("通过")} />
          <Detail term={t("订阅自动更新")} value={snapshot?.subscriptionError ?? refresh} />
        </dl>
      </Card>

      <Card
        title={t("诊断报告")}
        description={t("从受管的运行状态和日志生成一份有大小限制的报告，不包含节点密码。")}
        actions={<button className="primary" disabled={Boolean(busy)} onClick={() => void run("diagnostics", desktop.diagnostics)}><ClipboardList size={16} />{t("生成诊断报告")}</button>}
      />

      <Connections snapshot={snapshot} />
    </div>
  );
}

/** Live traffic and the engine's connection list, read once a second while connected. */
function Connections({ snapshot }: { snapshot: RuntimeSnapshot | null }) {
  const live = liveEngine(snapshot);
  const [list, reload] = usePolling(desktop.connections, live);
  const [shown, setShown] = useState<"active" | "closed">("active");
  const [query, setQuery] = useState("");
  const [failure, setFailure] = useState<string | null>(null);

  const close = (task: () => Promise<unknown>) => {
    setFailure(null);
    task().then(reload).catch((error: unknown) => setFailure(String(error)));
  };

  if (!list) {
    const message = !live
      ? snapshot?.runtimeMode === "service" ? t("连接记录只在桌面模式下可用，服务模式的内核在另一个进程里运行。") : t("连接后，这里会实时显示每条连接命中的规则和策略。")
      : t("正在读取连接记录");
    return <Card><EmptyState icon={ListTree} title={t("连接")} message={message} /></Card>;
  }

  const connections = (shown === "active" ? [...list.active].reverse() : list.closed).filter((item) => matchesConnection(item, query));
  const traffic = list.traffic;

  return (
    <Card
      title={t("连接")}
      description={t("上传 {up} · 下载 {down}，自本次连接起", { up: formatBytes(traffic.upload_total), down: formatBytes(traffic.download_total) })}
      actions={<>
        <span className="rate"><ArrowUp size={14} />{formatRate(traffic.upload_rate)}</span>
        <span className="rate"><ArrowDown size={14} />{formatRate(traffic.download_rate)}</span>
        <button className="secondary" disabled={list.active.length === 0} onClick={() => close(desktop.closeAllConnections)}>{t("全部断开")}</button>
      </>}
    >
      <div className="toolbar">
        <Segmented
          label={t("连接列表")}
          options={[{ value: "active", label: t("活跃 {count}", { count: list.active.length }) }, { value: "closed", label: t("已关闭 {count}", { count: list.closed.length }) }]}
          value={shown}
          onChange={setShown}
        />
        <label className="search">
          <Search size={15} aria-hidden="true" />
          <input aria-label={t("筛选连接")} placeholder={t("筛选主机、规则、策略或程序")} value={query} onChange={(event) => setQuery(event.target.value)} />
        </label>
        <span className="count">{connections.length}</span>
      </div>
      {failure && <p className="muted">{failure}</p>}
      {connections.length === 0
        ? <p className="muted">{query ? t("没有匹配的连接。") : shown === "active" ? t("现在没有活跃连接。") : t("还没有关闭的连接。")}</p>
        : <ul className="list connections">
          {connections.map((connection) => (
            <ConnectionRow key={connection.id} connection={connection} onClose={shown === "active" ? () => close(() => desktop.closeConnection(connection.id)) : undefined} />
          ))}
        </ul>}
    </Card>
  );
}

function ConnectionRow({ connection, onClose }: { connection: ConnectionSnapshot; onClose?: () => void }) {
  const detail = connectionDetail(connection);
  return (
    <li>
      <span className="list-main">
        <strong className="truncate" title={connection.destination}>{connectionName(connection)}</strong>
        <small title={detail}>{detail}</small>
      </span>
      {onClose && <IconButton label={t("断开这条连接")} onClick={onClose}><X size={15} /></IconButton>}
    </li>
  );
}
