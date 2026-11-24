import { Copy, RefreshCw, ScrollText, Search, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { desktop, LogEntry } from "../bridge";
import { t } from "../i18n";
import { listPage } from "../live";
import { filterLogs, LevelFilter, logModules, logText } from "../log-view";
import { Card, EmptyState, IconButton, MoreMenu, Run, Segmented } from "../ui";

const moduleNames: Record<string, string> = { app: "应用", gui: "窗口", mitm: "HTTPS 解密" };

/** The app's own log, filtered by level, module and text, newest first; read again every five seconds while connected. */
export function LogsCard({ live, busy, run }: { live: boolean; busy: string | null; run: Run }) {
  const [entries, setEntries] = useState<LogEntry[] | null>(null);
  const [level, setLevel] = useState<LevelFilter>("all");
  const [module, setModule] = useState("");
  const [query, setQuery] = useState("");
  const [limit, setLimit] = useState(listPage);
  const load = useCallback(() => void desktop.appLogs().then(setEntries).catch(() => setEntries(null)), []);
  useEffect(() => {
    load();
    if (!live) return;
    const timer = window.setInterval(load, 5_000);
    return () => window.clearInterval(timer);
  }, [live, load]);

  const all = entries ?? [];
  const modules = logModules(all);
  const shown = filterLogs(all, level, module, query);
  const clear = () => void run("logs", async () => {
    setEntries(await desktop.appLogs(true));
    return { message: t("已清空日志") };
  });
  const copy = () => void run("logs", async () => {
    await navigator.clipboard.writeText(logText(shown));
    return { message: t("已拷贝 {count} 行日志", { count: shown.length }) };
  });

  return (
    <Card
      title={t("日志")}
      description={t("应用的运行记录：连接、断开、订阅更新和出错原因，网址仅保留域名。")}
      actions={<>
        <IconButton label={t("刷新日志")} onClick={load}><RefreshCw size={14} /></IconButton>
        <MoreMenu
          label={t("更多日志操作")}
          disabled={Boolean(busy)}
          items={[
            { label: t("拷贝筛选出的日志"), icon: Copy, disabled: shown.length === 0, onSelect: copy },
            { label: t("清空日志"), icon: Trash2, danger: true, disabled: all.length === 0, confirm: t("确认清空全部日志"), onSelect: clear },
          ]}
        />
      </>}
    >
      <div className="row toolbar">
        <Segmented
          label={t("日志级别")}
          options={[{ value: "all", label: t("全部") }, { value: "warn", label: t("警告") }, { value: "error", label: t("错误") }]}
          value={level}
          onChange={(next) => { setLevel(next); setLimit(listPage); }}
        />
        {modules.length > 1 && (
          <Segmented
            label={t("日志模块")}
            options={[{ value: "", label: t("全部模块") }, ...modules.map((name) => ({ value: name, label: moduleNames[name] ? t(moduleNames[name]) : name }))]}
            value={module}
            onChange={(next) => { setModule(next); setLimit(listPage); }}
          />
        )}
        <label className="search">
          <Search size={14} aria-hidden="true" />
          <input aria-label={t("筛选日志")} placeholder={t("筛选日志内容")} value={query} onChange={(event) => { setQuery(event.target.value); setLimit(listPage); }} />
        </label>
        <span className="count">{shown.length}</span>
      </div>
      {shown.length === 0 ? (
        <EmptyState icon={ScrollText} title={t("日志")} message={all.length === 0 ? t("暂无日志") : t("没有匹配的日志")} />
      ) : (
        <>
          <ul className="log-lines">
            {shown.slice(0, limit).map((entry, index) => (
              <li key={`${entry.timeMs}-${index}`} className={entry.level}>
                <time>{entry.timeMs ? new Date(entry.timeMs).toLocaleString() : "-"}</time>
                <span className="log-module">{moduleNames[entry.module] ? t(moduleNames[entry.module]) : entry.module}</span>
                <span className="log-message">{entry.message}</span>
              </li>
            ))}
          </ul>
          {shown.length > limit && (
            <button className="link-button more" onClick={() => setLimit((current) => current + listPage)}>
              {t("再显示 {count} 条", { count: Math.min(listPage, shown.length - limit) })}
            </button>
          )}
        </>
      )}
    </Card>
  );
}
