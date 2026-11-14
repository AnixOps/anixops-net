import { Activity, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { desktop, TrafficStats } from "../bridge";
import { t } from "../i18n";
import { formatBytes } from "../subscription-format";
import { buckets, hoursSince, periodStart, shares, StatsPeriod, totals } from "../traffic-stats";
import { Card, EmptyState, MoreMenu, Row, Run, Segmented } from "../ui";

type StatsView = "time" | "hosts" | "policies";

/** Traffic of today or the last seven days, over time, by host or by policy, as the iOS statistics page shows it. */
export function TrafficStatsCard({ live, busy, run }: { live: boolean; busy: string | null; run: Run }) {
  const [stats, setStats] = useState<TrafficStats | null>(null);
  const [period, setPeriod] = useState<StatsPeriod>("today");
  const [view, setView] = useState<StatsView>("time");
  useEffect(() => {
    const load = () => void desktop.trafficStats().then(setStats).catch(() => setStats(null));
    load();
    if (!live) return;
    const timer = window.setInterval(load, 10_000);
    return () => window.clearInterval(timer);
  }, [live]);

  const hours = hoursSince(stats, periodStart(period));
  const sum = totals(hours);
  const bars = buckets(hours, period);
  const peak = Math.max(1, ...bars.map((bar) => bar.bytes));
  const rows = view === "time" ? [] : shares(hours, view);
  const clear = () => void run("traffic statistics", async () => {
    setStats(await desktop.trafficStats(true));
    return { message: t("已清空流量统计") };
  });

  return (
    <Card
      title={t("流量统计")}
      description={t("每 5 秒从连接列表累计，只记主机名和字节数，保存在本机，保留 7 天。")}
      actions={<MoreMenu
        label={t("更多统计操作")}
        disabled={Boolean(busy)}
        items={[{ label: t("清空统计"), icon: Trash2, danger: true, disabled: (stats?.hours.length ?? 0) === 0, confirm: t("确认清空全部流量统计"), onSelect: clear }]}
      />}
    >
      <div className="stats-controls">
        <Segmented label={t("时间范围")} options={[{ value: "today", label: t("今天") }, { value: "week", label: t("7 天") }]} value={period} onChange={setPeriod} />
        <Segmented label={t("分组")} options={[{ value: "time", label: period === "today" ? t("按小时") : t("按天") }, { value: "hosts", label: t("按主机") }, { value: "policies", label: t("按策略") }]} value={view} onChange={setView} />
      </div>
      <Row label={formatBytes(sum.upload + sum.download)} hint={t("上传 {up} · 下载 {down}", { up: formatBytes(sum.upload), down: formatBytes(sum.download) })} />
      {hours.length === 0 ? (
        <EmptyState icon={Activity} title={t("流量统计")} message={stats ? t("这段时间没有流量") : t("连接后开始统计")} />
      ) : view === "time" ? (
        <div className={`stats-bars ${period}`} role="img" aria-label={t("流量统计")}>
          {bars.map((bar) => (
            <span key={bar.start} title={`${new Date(bar.start * 1000).toLocaleString()} · ${formatBytes(bar.bytes)}`}>
              <span style={{ height: `${(bar.bytes / peak) * 100}%` }} />
            </span>
          ))}
        </div>
      ) : (
        <div className="rows">
          {rows.map((row) => (
            <Row key={row.name} label={<span className="truncate">{row.name === "*" ? t("其他主机") : row.name}</span>} value={formatBytes(row.bytes)} />
          ))}
        </div>
      )}
    </Card>
  );
}
