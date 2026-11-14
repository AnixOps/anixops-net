import { Activity, ArrowDown, ArrowUp, CheckCircle2, CircleAlert, Globe, House, Puzzle, Route, Settings, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { formatRate } from "./activity-format";
import { desktop, NodeDelay, NodeSummary, RuntimeSnapshot } from "./bridge";
import { Wordmark } from "./brand";
import { connectionTitle, connectPhase, routingLabel } from "./labels";
import { ActivityPage } from "./pages/ActivityPage";
import { RequestsPage } from "./pages/RequestsPage";
import { ExtensionsPage } from "./pages/ExtensionsPage";
import { HomePage } from "./pages/HomePage";
import { ProxiesPage } from "./pages/ProxiesPage";
import { RulesPage } from "./pages/RulesPage";
import { SettingsPage } from "./pages/SettingsPage";
import { keepIfUnchanged, liveEngine, usePageVisible } from "./live";
import { TrafficSource, useTraffic } from "./traffic-feed";
import { useAppearance } from "./theme";
import { StatusDot, Switch, Tile, TileColor, ToolbarSlot } from "./ui";
import { currentLanguage, t, useLanguage } from "./i18n";
import { TermsConsent, useTermsConsent } from "./terms";
import { AppMode, initialMode, useDebugExit, useMode, wantsDirectEgress } from "./mode";
import { ModeChoice } from "./mode-choice";

export type Page = "home" | "proxies" | "rules" | "extensions" | "activity" | "settings";

type NavItem = { page: Page; label: string; icon: LucideIcon; color: TileColor };

const navigation: NavItem[] = [
  { page: "home", label: "概览", icon: House, color: "indigo" },
  { page: "proxies", label: "代理", icon: Globe, color: "sky" },
  { page: "rules", label: "规则", icon: Route, color: "orange" },
  { page: "extensions", label: "扩展", icon: Puzzle, color: "purple" },
  { page: "activity", label: "活动", icon: Activity, color: "pink" },
];

// Debug mode (docs/debug-mode.md), the same on every platform: Requests · Rewrite, then Egress · Settings. The app opens
// on the requests; plugins, mappings and the decryption certificate are "Rewrite"; subscriptions, nodes and routing
// rules are kept under "Egress".
const debugNavigation: NavItem[] = [
  { page: "activity", label: "请求", icon: Activity, color: "pink" },
  { page: "extensions", label: "改写", icon: Puzzle, color: "purple" },
];

const egressItem: NavItem = { page: "proxies", label: "出口", icon: Globe, color: "sky" };

const settingsItem: NavItem = { page: "settings", label: "设置", icon: Settings, color: "gray" };

/** The sidebar of a mode: the main pages, then the ones below the gap. */
function sidebarOf(mode: AppMode): { main: NavItem[]; foot: NavItem[] } {
  return mode === "debug" ? { main: debugNavigation, foot: [egressItem, settingsItem] } : { main: navigation, foot: [settingsItem] };
}

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
  const [storedMode, chooseMode] = useMode();
  const [debugExit, setDebugExit] = useDebugExit();
  const noticeTimer = useRef<number>();

  const show = useCallback((text: string, tone: Notice["tone"]) => {
    window.clearTimeout(noticeTimer.current);
    setNotice({ text, tone });
    // Results fade on their own; errors stay until dismissed.
    if (tone === "success") noticeTimer.current = window.setTimeout(() => setNotice(null), 4000);
  }, []);
  const showError = useCallback((error: unknown) => show(error instanceof Error ? error.message : String(error), "danger"), [show]);

  const visible = usePageVisible();

  // System notifications are written by the backend, in the language the window shows.
  useEffect(() => {
    void desktop.notificationSettings(currentLanguage()).catch(() => undefined);
  }, [language]);

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

  // Someone with a subscription or nodes came for the proxy and is not asked; a new user picks once.
  const hasProfile = snapshot ? Boolean(snapshot.subscription) || nodes.length > 0 : null;
  const resolved = initialMode(storedMode, hasProfile);
  const mode: AppMode = resolved === "debug" ? "debug" : "proxy";
  useEffect(() => {
    if (storedMode === null && resolved === "proxy") chooseMode("proxy");
  }, [storedMode, resolved, chooseMode]);
  const sidebar = sidebarOf(mode);
  const pages = [...sidebar.main, ...sidebar.foot];
  // A page the mode has no entry for (the overview in debug mode) gives way to the mode's first page.
  useEffect(() => {
    if (resolved !== "wait" && resolved !== "ask" && !pages.some((item) => item.page === page)) setPage(pages[0].page);
  }, [resolved, page, pages]);
  const switchMode = (next: AppMode) => {
    chooseMode(next);
    setPage(sidebarOf(next).main[0].page);
  };

  // Debug mode with the direct exit runs the engine with no node; the backend keeps the choice, so it is sent only
  // when it differs, once per wanted value.
  const supportsDirect = snapshot?.directEgress !== undefined;
  const wantDirect = wantsDirectEgress(mode, debugExit, nodes.length > 0);
  const sentEgress = useRef<boolean | null>(null);
  useEffect(() => {
    if (!supportsDirect || resolved === "wait" || resolved === "ask" || snapshot?.directEgress === wantDirect || sentEgress.current === wantDirect) return;
    sentEgress.current = wantDirect;
    void run("egress", () => desktop.setDirectEgress(wantDirect));
  }, [supportsDirect, resolved, snapshot?.directEgress, wantDirect, run]);
  // Importing nodes in debug mode means they are wanted as the exit.
  const knownNodes = useRef<number | null>(null);
  useEffect(() => {
    if (!snapshot) return;
    if (knownNodes.current === 0 && nodes.length > 0 && mode === "debug") setDebugExit("node");
    knownNodes.current = nodes.length;
  }, [snapshot, nodes.length, mode, setDebugExit]);
  const egress = mode === "debug" ? { exit: debugExit, setExit: setDebugExit, supported: supportsDirect } : undefined;
  const canRunWithoutNodes = supportsDirect && wantDirect;

  const title = t(pages.find((item) => item.page === page)?.label ?? "");
  const dotTone = phase === "on" ? "success" : phase === "failed" ? "danger" : phase === "connecting" || phase === "disconnecting" ? "warning" : "neutral";
  const node = nodes.find((item) => item.id === snapshot?.selectedNode);
  const sideDetail = phase === "on"
    ? [node?.label ?? snapshot?.selectedNode, routingLabel(snapshot?.routingMode)].filter(Boolean).join(" · ")
    : phase === "off" ? "Ctrl+Shift+C" : "";
  const moving = phase === "connecting" || phase === "disconnecting";
  // Debug mode's requests are a table beside an inspector that fills the window below the title bar.
  const width = mode === "debug" && page === "activity" ? "split" : pageWidths[page] ?? "narrow";

  const navButton = ({ page: item, label, icon, color }: NavItem) => (
    <button
      className={`nav-item ${page === item ? "active" : ""}`}
      key={item}
      aria-current={page === item ? "page" : undefined}
      onClick={() => setPage(item)}
    >
      <Tile icon={icon} color={color} /><span>{t(label)}</span>
      {item === "proxies" && mode === "proxy" && nodes.length > 0 && <span className="nav-count">{nodes.length}</span>}
      {item === "activity" && <ActiveCount />}
    </button>
  );

  // Nothing else is usable until the user agreement is accepted (TERMS.md).
  if (needsTerms) return <TermsConsent onAccept={acceptTerms} />;
  if (resolved === "ask") return <ModeChoice onChoose={switchMode} />;

  return (
    <div className="app-shell">
      <TrafficSource enabled={liveEngine(snapshot)} />
      <aside className="sidebar">
        <div className="sidebar-brand"><Wordmark /></div>
        <nav aria-label={t("主导航")}>
          {sidebar.main.map(navButton)}
          <div className="nav-gap" />
          {sidebar.foot.map(navButton)}
        </nav>
        <div className="sidebar-foot">
          <SidebarRates />
          <div className="sidebar-status">
            <StatusDot tone={dotTone} />
            <button className="sidebar-status-text" onClick={() => setPage(sidebar.main[0].page)}>
              <strong>{connectionTitle(snapshot?.connection, phase)}</strong>
              {sideDetail && <small className="truncate">{sideDetail}</small>}
            </button>
            <Switch
              label={phase === "on" ? t("断开") : t("连接")}
              tone="go"
              checked={phase === "on" || phase === "connecting"}
              busy={moving}
              disabled={phase === "loading" || moving || (snapshot !== null && !snapshot.subscription && nodes.length === 0 && !canRunWithoutNodes)}
              onChange={toggleConnection}
            />
          </div>
        </div>
      </aside>
      <main className={`workspace ${width === "split" ? "split" : ""}`}>
        <header className={`topbar ${width}`}>
          <h1>{title}</h1>
          <div className="topbar-tools" ref={setToolbar} />
        </header>
        <div className={`page ${width}`} key={page}>
          <ToolbarSlot.Provider value={toolbar}>
            {page === "home" && <HomePage snapshot={snapshot} nodes={nodes} phase={phase} busy={busy} run={run} toggle={toggleConnection} goTo={setPage} delays={delays} setDelays={setDelays} />}
            {page === "proxies" && <ProxiesPage snapshot={snapshot} nodes={nodes} busy={busy} run={run} delays={delays} setDelays={setDelays} egress={egress} />}
            {page === "proxies" && mode === "debug" && <div className="egress-rules"><RulesPage snapshot={snapshot} busy={busy} run={run} /></div>}
            {page === "rules" && mode === "proxy" && <RulesPage snapshot={snapshot} busy={busy} run={run} />}
            {page === "extensions" && <ExtensionsPage snapshot={snapshot} busy={busy} run={run} />}
            {page === "activity" && (mode === "debug" ? <RequestsPage snapshot={snapshot} busy={busy} run={run} /> : <ActivityPage snapshot={snapshot} busy={busy} run={run} />)}
            {page === "settings" && <SettingsPage snapshot={snapshot} busy={busy} run={run} appearance={appearance} setAppearance={setAppearance} language={language} setLanguage={setLanguage} mode={mode} setMode={switchMode} />}
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
