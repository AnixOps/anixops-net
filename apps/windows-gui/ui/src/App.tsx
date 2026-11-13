import { Activity, ArrowDown, ArrowUp, CheckCircle2, CircleAlert, Globe, House, Puzzle, Route, Settings, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { formatRate } from "./activity-format";
import { desktop, NodeDelay, NodeSummary, RuntimeSnapshot } from "./bridge";
import { Wordmark } from "./brand";
import { connectionTitle, connectPhase, routingLabel } from "./labels";
import { ActivityPage } from "./pages/ActivityPage";
import { ExtensionsPage } from "./pages/ExtensionsPage";
import { HomePage } from "./pages/HomePage";
import { ProxiesPage } from "./pages/ProxiesPage";
import { RulesPage } from "./pages/RulesPage";
import { SettingsPage } from "./pages/SettingsPage";
import { keepIfUnchanged, liveEngine, usePageVisible } from "./live";
import { TrafficSource, useTraffic } from "./traffic-feed";
import { useAppearance } from "./theme";
import { StatusDot, Switch, Tile, TileColor, ToolbarSlot } from "./ui";
import { t, useLanguage } from "./i18n";
import { TermsConsent, useTermsConsent } from "./terms";

export type Page = "home" | "proxies" | "rules" | "extensions" | "activity" | "settings";

const navigation: Array<{ page: Page; label: string; icon: LucideIcon; color: TileColor }> = [
  { page: "home", label: "概览", icon: House, color: "indigo" },
  { page: "proxies", label: "代理", icon: Globe, color: "sky" },
  { page: "rules", label: "规则", icon: Route, color: "orange" },
  { page: "extensions", label: "扩展", icon: Puzzle, color: "purple" },
  { page: "activity", label: "活动", icon: Activity, color: "pink" },
];

const settingsItem = { page: "settings" as const, label: "设置", icon: Settings, color: "gray" as const };

// The dashboards use the whole window, lists read better wider, and settings-style pages keep a narrow column.
const pageWidths: Partial<Record<Page, "full" | "wide">> = { home: "full", proxies: "full", activity: "wide" };

interface Notice {
  text: string;
  tone: "success" | "danger";
}

export function App() {
  const [page, setPage] = useState<Page>("home");
  const [snapshot, setSnapshot] = useState<RuntimeSnapshot | null>(null);
  const [nodes, setNodes] = useState<NodeSummary[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  // Delay results stay for the session, so the overview can show the current node's delay.
  const [delays, setDelays] = useState<Record<string, NodeDelay>>({});
  const [notice, setNotice] = useState<Notice | null>(null);
  const [toolbar, setToolbar] = useState<HTMLElement | null>(null);
  const [appearance, setAppearance] = useAppearance();
  const [language, setLanguage] = useLanguage();
  const [needsTerms, acceptTerms] = useTermsConsent();
  const noticeTimer = useRef<number>();

  const show = useCallback((text: string, tone: Notice["tone"]) => {
    window.clearTimeout(noticeTimer.current);
    setNotice({ text, tone });
    // Results fade on their own; errors stay until dismissed.
    if (tone === "success") noticeTimer.current = window.setTimeout(() => setNotice(null), 4000);
  }, []);
  const showError = useCallback((error: unknown) => show(error instanceof Error ? error.message : String(error), "danger"), [show]);

  const visible = usePageVisible();

  const refresh = useCallback(async () => {
    const [nextSnapshot, nextNodes] = await Promise.all([desktop.snapshot(), desktop.nodes()]);
    // An unchanged answer keeps the old objects, so the open page does not redraw every two seconds.
    setSnapshot((previous) => keepIfUnchanged(previous, nextSnapshot));
    setNodes((previous) => keepIfUnchanged(previous, nextNodes));
  }, []);

  // Polls only while the window is showing; coming back from the tray refreshes at once.
  useEffect(() => {
    if (!visible) return;
    void refresh().catch(showError);
    const timer = window.setInterval(() => void refresh().catch(showError), 2000);
    return () => window.clearInterval(timer);
  }, [refresh, showError, visible]);

  const run = useCallback(async (operation: string, task: () => Promise<{ message: string }>) => {
    setBusy(operation);
    try {
      const result = await task();
      show(result.message, "success");
      await refresh();
    } catch (error) {
      showError(error);
    } finally {
      setBusy(null);
    }
  }, [refresh, show, showError]);

  const phase = connectPhase(snapshot?.connection, busy);
  const toggleConnection = useCallback(() => {
    if (busy || phase === "loading" || phase === "connecting" || phase === "disconnecting") return;
    if (phase === "on") void run("disconnect", desktop.disconnect);
    else void run("connect", desktop.connect);
  }, [busy, phase, run]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "c") {
        event.preventDefault();
        toggleConnection();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggleConnection]);

  const title = t([...navigation, settingsItem].find((item) => item.page === page)?.label ?? "");
  const dotTone = phase === "on" ? "success" : phase === "failed" ? "danger" : phase === "connecting" || phase === "disconnecting" ? "warning" : "neutral";
  const node = nodes.find((item) => item.id === snapshot?.selectedNode);
  const sideDetail = phase === "on"
    ? [node?.label ?? snapshot?.selectedNode, routingLabel(snapshot?.routingMode)].filter(Boolean).join(" · ")
    : phase === "off" ? "Ctrl+Shift+C" : "";
  const moving = phase === "connecting" || phase === "disconnecting";
  const width = pageWidths[page] ?? "narrow";

  const navButton = ({ page: item, label, icon, color }: typeof navigation[number]) => (
    <button
      className={`nav-item ${page === item ? "active" : ""}`}
      key={item}
      aria-current={page === item ? "page" : undefined}
      onClick={() => setPage(item)}
    >
      <Tile icon={icon} color={color} /><span>{t(label)}</span>
      {item === "proxies" && nodes.length > 0 && <span className="nav-count">{nodes.length}</span>}
      {item === "activity" && <ActiveCount />}
    </button>
  );

  // Nothing else is usable until the user agreement is accepted (TERMS.md).
  if (needsTerms) return <TermsConsent onAccept={acceptTerms} />;

  return (
    <div className="app-shell">
      <TrafficSource enabled={liveEngine(snapshot)} />
      <aside className="sidebar">
        <div className="sidebar-brand"><Wordmark /></div>
        <nav aria-label={t("主导航")}>
          {navigation.map(navButton)}
          <div className="nav-gap" />
          {navButton(settingsItem)}
        </nav>
        <div className="sidebar-foot">
          <SidebarRates />
          <div className="sidebar-status">
            <StatusDot tone={dotTone} />
            <button className="sidebar-status-text" onClick={() => setPage("home")}>
              <strong>{connectionTitle(snapshot?.connection, phase)}</strong>
              {sideDetail && <small className="truncate">{sideDetail}</small>}
            </button>
            <Switch
              label={phase === "on" ? t("断开") : t("连接")}
              tone="go"
              checked={phase === "on" || phase === "connecting"}
              busy={moving}
              disabled={phase === "loading" || moving || (snapshot !== null && !snapshot.subscription && nodes.length === 0)}
              onChange={toggleConnection}
            />
          </div>
        </div>
      </aside>
      <main className="workspace">
        <header className={`topbar ${width}`}>
          <h1>{title}</h1>
          <div className="topbar-tools" ref={setToolbar} />
        </header>
        <div className={`page ${width}`} key={page}>
          <ToolbarSlot.Provider value={toolbar}>
            {page === "home" && <HomePage snapshot={snapshot} nodes={nodes} phase={phase} busy={busy} run={run} toggle={toggleConnection} goTo={setPage} delays={delays} setDelays={setDelays} />}
            {page === "proxies" && <ProxiesPage snapshot={snapshot} nodes={nodes} busy={busy} run={run} delays={delays} setDelays={setDelays} />}
            {page === "rules" && <RulesPage snapshot={snapshot} busy={busy} run={run} />}
            {page === "extensions" && <ExtensionsPage snapshot={snapshot} busy={busy} run={run} />}
            {page === "activity" && <ActivityPage snapshot={snapshot} busy={busy} run={run} />}
            {page === "settings" && <SettingsPage snapshot={snapshot} busy={busy} run={run} appearance={appearance} setAppearance={setAppearance} language={language} setLanguage={setLanguage} />}
          </ToolbarSlot.Provider>
        </div>
        {notice && (
          <div className={`toast ${notice.tone}`} role={notice.tone === "danger" ? "alert" : "status"}>
            {notice.tone === "danger" ? <CircleAlert size={16} /> : <CheckCircle2 size={16} />}
            <span>{notice.text}</span>
            <button aria-label={t("关闭提示")} onClick={() => setNotice(null)}><X size={14} /></button>
          </div>
        )}
      </main>
    </div>
  );
}

/** The live upload and download rates above the connection switch; nothing while disconnected. */
function SidebarRates() {
  const { traffic } = useTraffic();
  if (!traffic) return null;
  return (
    <div className="sidebar-rates" aria-label={t("实时速度")}>
      <span><ArrowUp size={11} />{formatRate(traffic.upload_rate)}</span>
      <span><ArrowDown size={11} />{formatRate(traffic.download_rate)}</span>
    </div>
  );
}

/** How many connections are open, next to "Activity" in the sidebar. */
function ActiveCount() {
  const { traffic } = useTraffic();
  return traffic?.active ? <span className="nav-count">{traffic.active}</span> : null;
}
