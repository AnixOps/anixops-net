import { FileSearch, PanelBottom, PanelRight, Pause, Play, PlugZap, X } from "lucide-react";
import { CSSProperties, KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent, useCallback, useEffect, useRef, useState } from "react";
import { CaptureRequest, desktop, RuntimeSnapshot } from "../bridge";
import { combineFilter, QuickFilter, selectedRequest } from "../capture-filter";
import { liveEngine, usePolling } from "../live";
import { defaultSort, RequestSort } from "../request-table";
import { defaultShare, Dock, inspectorSize, parseDock, parseShare, shareFor } from "../split-pane";
import { EmptyState, IconButton, MoreMenu, PageTools, Run, Segmented, Switch } from "../ui";
import { t } from "../i18n";
import { errorLine } from "../error-text";
import { captureMenu, ConnectionsView, exportMenu } from "./ActivityPage";
import { CaptureSessions } from "./CaptureSessions";
import { DebugSetupCard } from "./DecryptHosts";
import { RequestInspector } from "./CaptureViews";
import { RequestFilterBar, RequestTable, useRequestFeed } from "./RequestList";

type View = "requests" | "connections" | "sessions";

/**
 * The window sizes that have room for the table and the inspector at once: beside each other, or one above the
 * other. In a smaller window they take turns.
 */
const SPLIT_QUERY: Record<Dock, string> = {
  right: "(min-width: 1100px)",
  bottom: "(min-width: 720px) and (min-height: 600px)",
};

function useMedia(media: string): boolean {
  const [matches, setMatches] = useState(() => window.matchMedia(media).matches);
  useEffect(() => {
    const query = window.matchMedia(media);
    const update = () => setMatches(query.matches);
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, [media]);
  return matches;
}

const dockKey = "anixops.inspector-dock";
const shareKey = (dock: Dock) => `anixops.inspector-share.${dock}`;

function stored(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function store(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // The layout still applies for this session.
  }
}

/**
 * Where the inspector docks and how much of the page it takes, remembered between starts. Each dock keeps its own
 * share, so a wide side pane does not become a tall bottom pane.
 */
function useInspectorLayout() {
  const [dock, setDock] = useState<Dock>(() => parseDock(stored(dockKey)));
  const [shares, setShares] = useState<Record<Dock, number>>(() => ({
    right: parseShare("right", stored(shareKey("right"))),
    bottom: parseShare("bottom", stored(shareKey("bottom"))),
  }));
  const chooseDock = (next: Dock) => {
    setDock(next);
    store(dockKey, next);
  };
  const setShare = (next: number) => {
    setShares((previous) => ({ ...previous, [dock]: next }));
    store(shareKey(dock), String(next));
  };
  return { dock, share: shares[dock], chooseDock, setShare };
}

/**
 * Debug mode's home page (debug-mode.md): the requests as a table beside an inspector. The page
 * toolbar holds what applies to the whole capture (the view, capture on or off, pause, export and clear); the
 * connections and the saved sessions are the page's other two views.
 */
export function RequestsPage({ snapshot, busy, run, onOpenRewrite }: {
  snapshot: RuntimeSnapshot | null;
  busy: string | null;
  run: Run;
  /** Goes to "Rewrite", where the certificate, decryption and decrypted domains are set up. */
  onOpenRewrite: () => void;
}) {
  const live = liveEngine(snapshot);
  const [view, setView] = useState<View>("requests");
  // Read disconnected too: the backend then answers from the capture viewer, which holds the session saved at the
  // disconnect (or one opened from the saved sessions).
  const [capture, reloadCapture] = usePolling(desktop.captureStatus, true);
  const [paused, setPaused] = useState(false);
  const [typed, setTyped] = useState("");
  const [quick, setQuick] = useState<QuickFilter[]>([]);
  const [sort, setSort] = useState<RequestSort>(defaultSort);
  /** The selected request's `requestKey`. */
  const [selected, setSelected] = useState<string | null>(null);
  // On a narrow window: the inspector shows in place of the table.
  const [opened, setOpened] = useState(false);
  const [replayAsked, setReplayAsked] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const filterField = useRef<HTMLInputElement>(null);
  const layout = useInspectorLayout();
  const { dock } = layout;
  // The window has room for both panes in the chosen arrangement.
  const wide = useMedia(SPLIT_QUERY[dock]);
  const workbench = useRef<HTMLDivElement | null>(null);
  const [room, setRoom] = useState(0);
  const measureRoom = useCallback(() => {
    const box = workbench.current?.getBoundingClientRect();
    if (box) setRoom(dock === "right" ? box.width : box.height);
  }, [dock]);
  // The workbench only exists once requests are listed, so the observer is attached by a ref callback.
  const observer = useRef<ResizeObserver | null>(null);
  const attachWorkbench = useCallback((element: HTMLDivElement | null) => {
    observer.current?.disconnect();
    workbench.current = element;
    if (!element) return;
    observer.current = new ResizeObserver(measureRoom);
    observer.current.observe(element);
    measureRoom();
  }, [measureRoom]);

  const dragDivider = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.buttons !== 1) return;
    const box = workbench.current?.getBoundingClientRect();
    if (!box) return;
    const total = dock === "right" ? box.width : box.height;
    const fromFarEdge = dock === "right" ? box.right - event.clientX : box.bottom - event.clientY;
    layout.setShare(shareFor(dock, fromFarEdge, total));
  };
  const nudgeDivider = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const grow = dock === "right" ? "ArrowLeft" : "ArrowUp";
    const shrink = dock === "right" ? "ArrowRight" : "ArrowDown";
    if (event.key !== grow && event.key !== shrink && event.key !== "Home") return;
    event.preventDefault();
    if (event.key === "Home") {
      layout.setShare(defaultShare[dock]);
      return;
    }
    const size = inspectorSize(dock, layout.share, room) + (event.key === grow ? 24 : -24);
    layout.setShare(shareFor(dock, size, room));
  };

  const filter = combineFilter(quick, typed);
  // A stopped capture or a loaded session still holds requests to list.
  const holding = Boolean(capture?.enabled) || (capture?.connections ?? 0) > 0;
  // Disconnected with a saved session open: the lists show it, and it can be exported.
  const saved = !live && holding;
  const feed = useRequestFeed(holding, filter, paused);
  // After an engine restart the same key can name another request: the selection and the open detail go.
  useEffect(() => {
    if (feed.generation > 0) setSelected(null);
  }, [feed.generation]);
  // The selected request as last seen in the list: a new filter (read again from the start), the list's own limit or
  // the engine's budget can take the row out of the list while its inspector has an open replay or mapping editor.
  // The inspector keeps showing this copy until another request is chosen.
  const held = useRef<CaptureRequest | null>(null);
  const { request, missing } = selectedRequest(feed.rows, selected, held.current);
  held.current = request;
  const captured = capture?.connections ?? 0;

  const change = useCallback((task: () => Promise<unknown>) => {
    setFailure(null);
    task().then(reloadCapture).catch((error: unknown) => setFailure(errorLine(error)));
  }, [reloadCapture]);
  // Clearing the capture removes every request: the selection goes with them.
  const clearCapture = useCallback(() => {
    setSelected(null);
    change(desktop.clearCapture);
  }, [change]);

  // Ctrl+F finds the filter field and Ctrl+R replays the selected request.
  useEffect(() => {
    if (view !== "requests") return;
    const onKey = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.shiftKey || event.altKey) return;
      const key = event.key.toLowerCase();
      if (key === "f") {
        event.preventDefault();
        filterField.current?.focus();
        filterField.current?.select();
      } else if (key === "r" && selected != null && !(event.target instanceof HTMLElement && event.target.closest("input, textarea, select"))) {
        event.preventDefault();
        setReplayAsked(true);
        setOpened(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [view, selected]);

  const clearReplay = useCallback(() => setReplayAsked(false), []);
  // A replay asked for a request that never loaded does not carry over to the next one.
  useEffect(clearReplay, [selected, clearReplay]);

  const tools = (
    <PageTools>
      <Segmented
        label={t("显示内容")}
        options={[
          { value: "requests", label: t("请求") },
          { value: "connections", label: t("连接") },
          { value: "sessions", label: t("已保存") },
        ]}
        value={view}
        onChange={setView}
      />
      {live && capture && (
        <label className="switch-label">
          <span>{t("抓包")}</span>
          <Switch
            label={capture.enabled ? t("停止抓包") : t("开始抓包")}
            checked={capture.enabled}
            onChange={() => change(() => desktop.setCapture(!capture.enabled))}
          />
        </label>
      )}
      {live && view === "requests" && (
        <IconButton label={paused ? t("继续刷新") : t("暂停刷新")} onClick={() => setPaused((current) => !current)}>
          {paused ? <Play size={15} /> : <Pause size={15} />}
        </IconButton>
      )}
      {live && (
        <MoreMenu label={t("更多抓包操作")} items={captureMenu(run, busy, captured, clearCapture)} />
      )}
      {saved && (
        <MoreMenu
          label={t("更多抓包操作")}
          items={[
            ...exportMenu(run, busy, captured),
            { label: t("关闭会话"), icon: X, onSelect: clearCapture },
          ]}
        />
      )}
    </PageTools>
  );

  // Where the empty page already offers "Start capture" as its main button, the checklist does not repeat it.
  const setupCard = (offerCapture: boolean, compact = false) => (
    <DebugSetupCard
      compact={compact}
      snapshot={snapshot}
      capture={capture}
      onStartCapture={offerCapture ? () => change(() => desktop.setCapture(true)) : undefined}
      onOpenRewrite={onOpenRewrite}
    />
  );

  if (!live && view === "sessions") {
    return <>{tools}<div className="page-stack"><CaptureSessions capture={capture} live={false} run={run} onLoaded={() => setSelected(null)} /></div></>;
  }

  if (!live && !holding) {
    return (
      <>
        {tools}
        <div className="page-placeholder">
          <div className="setup-stack">
            <EmptyState icon={PlugZap} title={t("还没有连接")} message={t("用连接开关连接后，经过本机的 HTTP 请求会逐条列在这里。调试模式默认直连，不需要节点。断开前抓到的内容会自动保存，可以在“已保存”里打开。")} />
            {setupCard(true)}
          </div>
        </div>
      </>
    );
  }

  const savedBanner = saved && (
    <p className="muted saved-capture-note" role="status">
      {t("隧道已断开，下面是断开前抓到的内容，已加密保存 7 天，可以在“更多”里打包导出。")}
    </p>
  );
  if (view === "connections") return <>{tools}{savedBanner}<ConnectionsView snapshot={snapshot} busy={busy} run={run} debug saved={saved} /></>;
  if (view === "sessions") return <>{tools}<div className="page-stack"><CaptureSessions capture={capture} live={live} run={run} onLoaded={() => setSelected(null)} /></div></>;

  if (capture && !holding) {
    return (
      <>
        {tools}
        <div className="page-placeholder">
          <div className="setup-stack">
            <EmptyState icon={FileSearch} title={t("还没有开始抓包")} message={t("开始抓包后，新的请求会逐条出现在这里。要看到 HTTPS 明文，还要在“改写”里信任解密证书，并为要看的域名开启解密。")}>
              <button className="primary" onClick={() => change(() => desktop.setCapture(true))}>{t("开始抓包")}</button>
            </EmptyState>
            {setupCard(false)}
          </div>
        </div>
      </>
    );
  }

  const table = (
    <section className="request-pane" aria-label={t("请求")}>
      <RequestFilterBar ref={filterField} typed={typed} onType={setTyped} quick={quick} onQuick={setQuick} error={feed.error} count={feed.rows.length} />
      {paused && <p className="muted request-paused">{t("已暂停刷新，新的请求仍在记录。")}</p>}
      {feed.rows.length === 0 ? (
        <div className="request-empty">
          <p className="muted">{filter ? t("没有匹配的请求。") : t("还没有解密的请求。HTTPS 请求要先在“改写”里为域名开启解密。")}</p>
        </div>
      ) : (
        <RequestTable
          rows={feed.rows}
          selected={selected}
          onSelect={(key) => {
            setSelected(key);
            // A tap on a phone opens the request; a wide window shows it beside the table already.
            if (!wide) setOpened(true);
          }}
          onOpen={(key) => {
            setSelected(key);
            setOpened(true);
          }}
          sort={sort}
          onSort={setSort}
        />
      )}
    </section>
  );
  const inspector = (
    <aside className="inspector-pane" aria-label={t("请求详情")}>
      <RequestInspector
        request={request}
        missing={missing}
        busy={busy}
        run={run}
        onBack={wide ? undefined : () => setOpened(false)}
        replayAsked={replayAsked}
        onReplayHandled={clearReplay}
        layoutItems={[dock === "right"
          ? { label: t("检查器停靠到下方"), icon: PanelBottom, onSelect: () => layout.chooseDock("bottom") }
          : { label: t("检查器停靠到右侧"), icon: PanelRight, onSelect: () => layout.chooseDock("right") }]}
      />
    </aside>
  );
  const size = inspectorSize(dock, layout.share, room);
  const divider = (
    <div
      className="split-divider"
      role="separator"
      tabIndex={0}
      aria-orientation={dock === "right" ? "vertical" : "horizontal"}
      aria-label={t("调整检查器大小")}
      aria-valuemin={0}
      aria-valuemax={Math.round(room)}
      aria-valuenow={size}
      title={t("拖动调整大小，双击恢复默认")}
      onPointerDown={(event) => event.currentTarget.setPointerCapture(event.pointerId)}
      onPointerMove={dragDivider}
      onDoubleClick={() => layout.setShare(defaultShare[dock])}
      onKeyDown={nudgeDivider}
    />
  );

  return (
    <>
      {tools}
      {failure && <p className="muted danger-text" role="alert">{failure}</p>}
      {savedBanner}
      {live && <div className="setup-banner">{setupCard(true, true)}</div>}
      {wide
        ? (
          <div className={`request-workbench dock-${dock}`} ref={attachWorkbench} style={{ "--inspector-size": `${size}px` } as CSSProperties}>
            {table}{divider}{inspector}
          </div>
        )
        : <div className="request-workbench single">{opened && request ? inspector : table}</div>}
    </>
  );
}
