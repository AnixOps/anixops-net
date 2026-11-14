import { ArrowDown, ArrowUp, BadgeCheck, ChevronRight, CircleDot, ClipboardList, Download, FileCode, ListTree, RefreshCw, Search, Trash2, TriangleAlert, Unplug, X } from "lucide-react";
import { useState } from "react";
import { ConnectionSnapshot, desktop, RuntimeSnapshot } from "../bridge";
import { connectionDetail, connectionName, formatRate, matchesConnection } from "../activity-format";
import { captureSummary } from "../capture-format";
import { listPage, liveEngine, usePolling } from "../live";
import { formatBytes } from "../subscription-format";
import { Card, EmptyState, IconButton, MoreMenu, Row, Run, Segmented, Switch } from "../ui";
import { t } from "../i18n";
import { CaptureDetail, exportCapture } from "./CaptureViews";

/** Session state and the connection list; debug mode ("Requests") puts the connections, with capture, first. */
export function ActivityPage({ snapshot, busy, run, debug = false }: { snapshot: RuntimeSnapshot | null; busy: string | null; run: Run; debug?: boolean }) {
  const refresh = snapshot?.subscriptionLastAttempt
    ? [snapshot.subscriptionRefreshResult || t("等待中"), snapshot.subscriptionLastAttempt, snapshot.subscriptionRefreshErrorCode].filter(Boolean).join(" · ")
    : t("还没有自动更新记录");

  const [detail, setDetail] = useState<ConnectionSnapshot | null>(null);
  if (detail && liveEngine(snapshot)) {
    return <CaptureDetail connection={detail} busy={busy} run={run} onBack={() => setDetail(null)} replayable={snapshot?.platform !== "android"} />;
  }

  const session = (
    <Card title={t("当前会话")} description={t("只有独立核实过的运行状态才会显示为已连接。")}>
      <Row icon={CircleDot} color={snapshot?.connection === "Connected" ? "green" : "gray"} label={t("运行状态")} value={snapshot?.connectionLabel ?? t("读取中")} />
      <Row icon={TriangleAlert} color={snapshot?.lastError ? "red" : "gray"} label={t("最近错误")} value={snapshot?.lastError ?? t("无")} />
      <Row icon={BadgeCheck} color={snapshot?.configurationError ? "red" : "gray"} label={t("配置校验")} value={snapshot?.configurationError ?? t("通过")} />
      <Row icon={RefreshCw} color={snapshot?.subscriptionError ? "red" : "gray"} label={t("订阅自动更新")} value={snapshot?.subscriptionError ?? refresh} />
      <Row icon={ClipboardList} color="indigo" label={t("诊断报告")} hint={t("从受管的运行状态和日志生成一份有大小限制的报告，不包含节点密码。")}>
        <button className="secondary compact" disabled={Boolean(busy)} onClick={() => void run("diagnostics", desktop.diagnostics)}>{t("生成诊断报告")}</button>
      </Row>
    </Card>
  );
  const connections = <Connections snapshot={snapshot} busy={busy} run={run} onOpen={setDetail} />;

  return (
    <div className="page-stack">
      {debug ? <>{connections}{session}</> : <>{session}{connections}</>}
    </div>
  );
}

/**
 * Live traffic and the engine's connection list, read once a second while connected. With packet capture on, new
 * connections keep their first bytes and open to show them.
 */
function Connections({ snapshot, busy, run, onOpen }: { snapshot: RuntimeSnapshot | null; busy: string | null; run: Run; onOpen: (connection: ConnectionSnapshot) => void }) {
  const live = liveEngine(snapshot);
  const [list, reload] = usePolling(desktop.connections, live);
  const [capture, reloadCapture] = usePolling(desktop.captureStatus, live);
  const [shown, setShown] = useState<"active" | "closed">("active");
  const [query, setQuery] = useState("");
  const [limit, setLimit] = useState(listPage);
  const [failure, setFailure] = useState<string | null>(null);

  const close = (task: () => Promise<unknown>) => {
    setFailure(null);
    task().then(() => { reload(); reloadCapture(); }).catch((error: unknown) => setFailure(String(error)));
  };

  if (!list) {
    const message = !live
      ? snapshot?.runtimeMode === "service" ? t("连接记录只在桌面模式下可用，服务模式的内核在另一个进程里运行。") : t("连接后，这里会实时显示每条连接命中的规则和策略。")
      : t("正在读取连接记录");
    return <Card><EmptyState icon={ListTree} title={t("连接")} message={message} /></Card>;
  }

  const connections = (shown === "active" ? [...list.active].reverse() : list.closed).filter((item) => matchesConnection(item, query));
  const traffic = list.traffic;

  const totals = t("上传 {up} · 下载 {down}，自本次连接起", { up: formatBytes(traffic.upload_total), down: formatBytes(traffic.download_total) });
  const captured = capture?.connections ?? 0;

  return (
    <Card
      title={t("连接")}
      description={capture?.enabled ? `${captureSummary(capture)} · ${totals}` : totals}
      actions={<>
        <span className="rate"><ArrowUp size={14} />{formatRate(traffic.upload_rate)}</span>
        <span className="rate"><ArrowDown size={14} />{formatRate(traffic.download_rate)}</span>
        {capture && (
          <label className="switch-label">
            <span>{t("抓包")}</span>
            <Switch
              label={capture.enabled ? t("停止抓包") : t("开始抓包")}
              checked={capture.enabled}
              onChange={() => close(() => desktop.setCapture(!capture.enabled))}
            />
          </label>
        )}
        <MoreMenu
          label={t("更多连接操作")}
          items={[
            { label: t("导出全部 pcapng"), icon: Download, disabled: Boolean(busy) || captured === 0, onSelect: () => exportCapture(run, "pcapng") },
            { label: t("导出全部 HAR"), icon: FileCode, disabled: Boolean(busy) || captured === 0, onSelect: () => exportCapture(run, "har") },
            {
              label: t("清空抓包"),
              icon: Trash2,
              danger: true,
              disabled: captured === 0,
              confirm: t("确认清空 {count} 个连接的抓包", { count: captured }),
              onSelect: () => close(desktop.clearCapture),
            },
            {
              label: t("全部断开"),
              icon: Unplug,
              danger: true,
              disabled: list.active.length === 0,
              confirm: t("确认断开 {count} 条连接", { count: list.active.length }),
              onSelect: () => close(desktop.closeAllConnections),
            },
          ]}
        />
      </>}
    >
      <div className="row toolbar">
        <Segmented
          label={t("连接列表")}
          options={[{ value: "active", label: t("活跃 {count}", { count: list.active.length }) }, { value: "closed", label: t("已关闭 {count}", { count: list.closed.length }) }]}
          value={shown}
          onChange={(next) => { setShown(next); setLimit(listPage); }}
        />
        <label className="search">
          <Search size={14} aria-hidden="true" />
          <input aria-label={t("筛选连接")} placeholder={t("筛选主机、规则、策略或程序")} value={query} onChange={(event) => { setQuery(event.target.value); setLimit(listPage); }} />
        </label>
        <span className="count">{connections.length}</span>
      </div>
      {failure && <p className="muted danger-text">{failure}</p>}
      {connections.length === 0
        ? <p className="muted">{query ? t("没有匹配的连接。") : shown === "active" ? t("现在没有活跃连接。") : t("还没有关闭的连接。")}</p>
        : <>
          <ul className="list connections">
            {connections.slice(0, limit).map((connection) => (
              <ConnectionRow
                key={connection.id}
                connection={connection}
                onOpen={connection.captured ? () => onOpen(connection) : undefined}
                onClose={shown === "active" ? () => close(() => desktop.closeConnection(connection.id)) : undefined}
              />
            ))}
          </ul>
          {connections.length > limit && (
            <button className="link-button more" onClick={() => setLimit((current) => current + listPage)}>
              {t("再显示 {count} 条", { count: Math.min(listPage, connections.length - limit) })}
            </button>
          )}
        </>}
    </Card>
  );
}

/** One connection; one with captured bytes opens to show them. */
function ConnectionRow({ connection, onOpen, onClose }: { connection: ConnectionSnapshot; onOpen?: () => void; onClose?: () => void }) {
  const detail = connectionDetail(connection);
  const main = (
    <>
      <strong className="truncate" title={connection.destination}>{connectionName(connection)}</strong>
      <small title={detail}>{detail}</small>
    </>
  );
  return (
    <li className={onOpen ? "openable" : undefined}>
      {onOpen
        ? <button className="list-main list-open" onClick={onOpen} aria-label={t("查看抓包内容：{name}", { name: connectionName(connection) })}>{main}</button>
        : <span className="list-main">{main}</span>}
      {onClose && <IconButton label={t("断开这条连接")} onClick={onClose}><X size={15} /></IconButton>}
      {onOpen && <ChevronRight size={15} className="chevron" aria-hidden="true" />}
    </li>
  );
}
