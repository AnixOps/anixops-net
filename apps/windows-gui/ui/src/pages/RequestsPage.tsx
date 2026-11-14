import { FileSearch, Pause, Play, PlugZap } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { desktop, RuntimeSnapshot } from "../bridge";
import { combineFilter, QuickFilter } from "../capture-filter";
import { liveEngine, usePolling } from "../live";
import { defaultSort, RequestSort } from "../request-table";
import { EmptyState, IconButton, MoreMenu, PageTools, Run, Segmented, Switch } from "../ui";
import { t } from "../i18n";
import { captureMenu, ConnectionsView } from "./ActivityPage";
import { CaptureSessions } from "./CaptureSessions";
import { RequestInspector } from "./CaptureViews";
import { RequestFilterBar, RequestTable, useRequestFeed } from "./RequestList";

type View = "requests" | "connections" | "sessions";

/** Below this width the table and the inspector take turns instead of sitting side by side. */
const SPLIT_QUERY = "(min-width: 1100px)";

function useWideWindow(): boolean {
  const [wide, setWide] = useState(() => window.matchMedia(SPLIT_QUERY).matches);
  useEffect(() => {
    const query = window.matchMedia(SPLIT_QUERY);
    const update = () => setWide(query.matches);
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);
  return wide;
}

/**
 * Debug mode's home page (debug-mode.md): the requests as a table beside an inspector, after Proxyman. The page
 * toolbar holds what applies to the whole capture (the view, capture on or off, pause, export and clear); the
 * connections and the saved sessions are the page's other two views.
 */
export function RequestsPage({ snapshot, busy, run }: { snapshot: RuntimeSnapshot | null; busy: string | null; run: Run }) {
  const live = liveEngine(snapshot);
  const [view, setView] = useState<View>("requests");
  const [capture, reloadCapture] = usePolling(desktop.captureStatus, live);
  const [paused, setPaused] = useState(false);
  const [typed, setTyped] = useState("");
  const [quick, setQuick] = useState<QuickFilter[]>([]);
  const [sort, setSort] = useState<RequestSort>(defaultSort);
  const [selected, setSelected] = useState<number | null>(null);
  // On a narrow window: the inspector shows in place of the table.
  const [opened, setOpened] = useState(false);
  const [replayAsked, setReplayAsked] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const filterField = useRef<HTMLInputElement>(null);
  const wide = useWideWindow();

  const filter = combineFilter(quick, typed);
  // A stopped capture or a loaded session still holds requests to list.
  const holding = Boolean(capture?.enabled) || (capture?.connections ?? 0) > 0;
  const feed = useRequestFeed(live && holding, filter, paused);
  const request = feed.rows.find((row) => row.seq === selected) ?? null;
  const captured = capture?.connections ?? 0;

  const change = useCallback((task: () => Promise<unknown>) => {
    setFailure(null);
    task().then(reloadCapture).catch((error: unknown) => setFailure(String(error)));
  }, [reloadCapture]);

  // Ctrl+F finds the filter field and Ctrl+R replays the selected request, as in Proxyman.
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
        <MoreMenu label={t("更多抓包操作")} items={captureMenu(run, busy, captured, () => change(desktop.clearCapture))} />
      )}
    </PageTools>
  );

  if (!live) {
    return (
      <>
        {tools}
        <div className="page-placeholder">
          <EmptyState icon={PlugZap} title={t("还没有连接")} message={t("用连接开关连接后，经过本机的 HTTP 请求会逐条列在这里。调试模式默认直连，不需要节点。")} />
        </div>
      </>
    );
  }

  if (view === "connections") return <>{tools}<ConnectionsView snapshot={snapshot} busy={busy} run={run} debug /></>;
  if (view === "sessions") return <>{tools}<div className="page-stack"><CaptureSessions capture={capture} run={run} /></div></>;

  if (capture && !holding) {
    return (
      <>
        {tools}
        <div className="page-placeholder">
          <EmptyState icon={FileSearch} title={t("还没有开始抓包")} message={t("开始抓包后，新的请求会逐条出现在这里。要看到 HTTPS 明文，还要在“改写”里信任解密证书，并为要看的域名开启解密。")}>
            <button className="primary" onClick={() => change(() => desktop.setCapture(true))}>{t("开始抓包")}</button>
          </EmptyState>
        </div>
      </>
    );
  }

  const table = (
    <section className="request-pane" aria-label={t("请求")}>
      <RequestFilterBar ref={filterField} typed={typed} onType={setTyped} quick={quick} onQuick={setQuick} error={feed.error} count={feed.rows.length} />
      {paused && <p className="muted request-paused">{t("已暂停刷新，新的请求仍在记录。")}</p>}
      {failure && <p className="muted danger-text" role="alert">{failure}</p>}
      {feed.rows.length === 0 ? (
        <div className="request-empty">
          <p className="muted">{filter ? t("没有匹配的请求。") : t("还没有解密的请求。HTTPS 请求要先在“改写”里为域名开启解密。")}</p>
        </div>
      ) : (
        <RequestTable
          rows={feed.rows}
          selected={selected}
          onSelect={(seq) => {
            setSelected(seq);
            // A tap on a phone opens the request; a wide window shows it beside the table already.
            if (!wide) setOpened(true);
          }}
          onOpen={(seq) => {
            setSelected(seq);
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
        busy={busy}
        run={run}
        onBack={wide ? undefined : () => setOpened(false)}
        replayAsked={replayAsked}
        onReplayHandled={clearReplay}
      />
    </aside>
  );

  return (
    <>
      {tools}
      {wide
        ? <div className="request-workbench">{table}{inspector}</div>
        : <div className="request-workbench single">{opened && request ? inspector : table}</div>}
    </>
  );
}
