import { ArrowDown, ArrowUp, Bot, CloudDownload, Globe, Layers, LockKeyhole, Network, RefreshCw, Sigma } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useEffect, useState } from "react";
import type { Page } from "../App";
import { desktop, NativeGroupSummary, NodeSummary, RuntimeSnapshot, TrafficSnapshot } from "../bridge";
import { formatRate } from "../activity-format";
import { BrandMark } from "../brand";
import { liveEngine, usePolling } from "../live";
import { availableRoutingModes, ConnectPhase, connectionTitle, groupMemberLabel, routingLabel, routingModes } from "../labels";
import { formatBytes } from "../subscription-format";
import { Card, Row, Run, Segmented, StatusDot, Switch, ToggleRow } from "../ui";
import { t } from "../i18n";

/** How many one-second samples the speed graphs keep. */
const historyLength = 40;

export function HomePage({ snapshot, nodes, phase, busy, run, toggle, goTo }: {
  snapshot: RuntimeSnapshot | null;
  nodes: NodeSummary[];
  phase: ConnectPhase;
  busy: string | null;
  run: Run;
  toggle: () => void;
  goTo: (page: Page) => void;
}) {
  const [groups, setGroups] = useState<NativeGroupSummary[]>([]);
  const [traffic] = usePolling(desktop.traffic, liveEngine(snapshot));
  const history = useHistory(traffic);
  useEffect(() => {
    void desktop.nativeGroups().then(setGroups).catch(() => setGroups([]));
  }, [snapshot?.subscription]);

  const node = nodes.find((item) => item.id === snapshot?.selectedNode);
  const nodeName = node?.label ?? snapshot?.selectedNode;
  const unconfigured = snapshot !== null && !snapshot.subscription && nodes.length === 0;
  const linux = snapshot?.platform === "linux";
  const desktopMode = snapshot?.runtimeMode !== "service";
  const moving = phase === "connecting" || phase === "disconnecting";

  const subtitle = (() => {
    if (unconfigured) return t("先导入订阅或配置文件");
    switch (phase) {
      case "on":
        return [nodeName, routingLabel(snapshot?.routingMode), snapshot?.tunModeEnabled ? t("TUN 模式") : snapshot?.runtimeMode === "service" ? t("Windows 服务") : t("系统代理")].filter(Boolean).join(" · ");
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

  const mode = routingModes.find((item) => item.value === snapshot?.routingMode);
  const dotTone = phase === "on" ? "success" : phase === "failed" ? "danger" : moving ? "warning" : "neutral";
  const live = phase === "on" && traffic !== null;

  return (
    <div className="page-stack">
      <section className={`hero ${phase}`}>
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
        <div className="stats" aria-label={t("实时速度")}>
          <Stat icon={ArrowUp} label={t("上传")} value={live ? formatRate(traffic.upload_rate) : null} series={history.up} color="var(--chart-2)" />
          <Stat icon={ArrowDown} label={t("下载")} value={live ? formatRate(traffic.download_rate) : null} series={history.down} color="var(--chart-1)" />
          <Stat icon={Sigma} label={t("本次流量")} value={live ? formatBytes(traffic.upload_total + traffic.download_total) : null} color="var(--chart-3)" />
        </div>
      </section>

      <Card title={t("出站模式")} description={mode ? t(mode.hint) : undefined}>
        <div className="row">
          <Segmented
            label={t("分流模式")}
            className="fill"
            options={availableRoutingModes(snapshot).map((item) => ({ value: item.value, label: t(item.label), hint: t(item.hint) }))}
            value={snapshot?.routingMode}
            disabled={Boolean(busy) || !snapshot}
            onChange={(value) => void run("routing", () => desktop.setRoutingMode(value))}
          />
        </div>
      </Card>

      <Card title={t("线路")}>
        <Row icon={Globe} color="sky" label={t("节点")} value={nodeName ?? t("未选择")} onClick={() => goTo("proxies")} />
        {groups.slice(0, 4).map((group) => (
          <Row
            key={group.tag}
            icon={/ai|gpt|openai|claude/i.test(group.tag) ? Bot : Layers}
            color={/ai|gpt|openai|claude/i.test(group.tag) ? "teal" : "orange"}
            label={group.tag}
            hint={group.groupType === "selector" ? t("手动选择") : group.groupType === "urltest" ? t("自动测速") : group.groupType}
            value={groupMemberLabel(group, group.selected)}
            onClick={() => goTo("rules")}
          />
        ))}
        {snapshot?.subscription
          ? <Row
            icon={CloudDownload}
            color="indigo"
            label={t("订阅")}
            hint={[t("{count} 个节点", { count: nodes.length }), snapshot.subscriptionLastUpdated ? t("上次更新 {time}", { time: snapshot.subscriptionLastUpdated }) : t("尚未成功更新")].join(" · ")}
          >
            {snapshot.subscription.startsWith("http") && (
              <button className="secondary compact" disabled={Boolean(busy)} onClick={() => void run("subscription", desktop.updateSubscription)}><RefreshCw size={13} />{t("更新")}</button>
            )}
          </Row>
          : <Row icon={CloudDownload} color="indigo" label={t("订阅")} hint={t("还没有订阅。导入后节点会出现在“代理”里。")} onClick={() => goTo("proxies")} />}
      </Card>

      {!linux && (
        <Card title={t("接管方式")}>
          {desktopMode && (
            <ToggleRow
              icon={Network}
              color="purple"
              label={t("TUN 模式")}
              hint={t("用虚拟网卡接管全部流量，不走系统代理的游戏和命令行程序也会经过代理。")}
              checked={Boolean(snapshot?.tunModeEnabled)}
              disabled={Boolean(busy) || !snapshot}
              onChange={() => void run("TUN", () => desktop.setTunMode(!snapshot?.tunModeEnabled))}
            />
          )}
          <Row icon={LockKeyhole} color="pink" label={t("HTTPS 解密与插件")} value={snapshot?.httpsDecryptionEnabled ? t("已开启") : t("未开启")} onClick={() => goTo("extensions")} />
        </Card>
      )}
    </div>
  );
}

function Stat({ icon: Icon, label, value, series, color }: {
  icon: LucideIcon;
  label: string;
  value: string | null;
  series?: number[];
  color: string;
}) {
  return (
    <div className="stat">
      <small><Icon size={12} />{label}</small>
      <strong>{value ?? "—"}</strong>
      {series && <Sparkline values={value === null ? [] : series} color={color} />}
    </div>
  );
}

/** A small area graph of the last samples, scaled to its own peak. */
function Sparkline({ values, color }: { values: number[]; color: string }) {
  if (values.length < 2) return <svg className="sparkline" aria-hidden="true" />;
  const peak = Math.max(...values, 1);
  const step = 100 / (historyLength - 1);
  const offset = (historyLength - values.length) * step;
  const points = values.map((value, index) => `${(offset + index * step).toFixed(2)} ${(23 - (value / peak) * 21).toFixed(2)}`);
  const line = `M${points.join(" L")}`;
  return (
    <svg className="sparkline" viewBox="0 0 100 24" preserveAspectRatio="none" aria-hidden="true">
      <path d={`${line} L100 24 L${offset.toFixed(2)} 24 Z`} fill={color} opacity=".14" />
      <path d={line} fill="none" stroke={color} strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

/** The recent upload and download rates, one sample per traffic answer; cleared when traffic stops. */
function useHistory(traffic: TrafficSnapshot | null) {
  const [history, setHistory] = useState<{ up: number[]; down: number[] }>({ up: [], down: [] });
  useEffect(() => {
    if (!traffic) {
      setHistory({ up: [], down: [] });
      return;
    }
    setHistory((current) => ({
      up: [...current.up, traffic.upload_rate].slice(-historyLength),
      down: [...current.down, traffic.download_rate].slice(-historyLength),
    }));
  }, [traffic]);
  return history;
}
