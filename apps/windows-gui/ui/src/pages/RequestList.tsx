import { ArrowDown, ArrowUp, Search } from "lucide-react";
import { forwardRef, type KeyboardEvent, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { CaptureRequest, desktop, REQUEST_PAGE } from "../bridge";
import {
  combineFilter,
  droppedMore,
  engineRestarted,
  keepOnly,
  mergeRequests,
  nextRev,
  nextSince,
  QuickFilter,
  quickFilters,
  readAllKept,
  requestKey,
  RESYNC_INTERVAL_MS,
} from "../capture-filter";
import { durationMs, noteText, streamKind, timeOfDay } from "../capture-format";
import { compareRequests, requestPath, RequestSort, statusTone } from "../request-table";
import { usePageVisible } from "../live";
import { formatBytes } from "../subscription-format";
import { t } from "../i18n";

/** Why the engine refused the expression: the position of the mistake when it says. */
function filterError(error: { code: string; at?: number; message?: string }): string {
  if (error.code === "invalid_filter") {
    return error.at != null ? t("筛选表达式在第 {at} 个字符处有误", { at: error.at + 1 }) : t("筛选表达式有误");
  }
  return error.message || error.code;
}

/** What the poll knows between reads. */
interface FeedState {
  rows: CaptureRequest[];
  lastSeq: number;
  lastRev: number | null;
  /** The engine's `dropped_before_seq` as last seen. */
  dropped: number;
  /** The engine dropped rows since the last full read; the next poll reads everything it holds. */
  resyncDue: boolean;
  /** When the last full read was, so a capture at its budget is not read in full every second. */
  resyncAt: number;
}

function emptyFeed(): FeedState {
  return { rows: [], lastSeq: 0, lastRev: null, dropped: 0, resyncDue: false, resyncAt: 0 };
}

export interface RequestFeed {
  /** Oldest first, by when they started; a request shows up while it is still under way. */
  rows: CaptureRequest[];
  error: string | null;
  /** Counts the times the list started over because the engine restarted; a selection from before means nothing now. */
  generation: number;
}

/**
 * The decrypted requests of every connection, read from the engine once a second while capture holds any and the
 * list is not paused. A new filter starts the list over.
 */
export function useRequestFeed(capturing: boolean, filter: string, paused: boolean): RequestFeed {
  const [rows, setRows] = useState<CaptureRequest[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [generation, setGeneration] = useState(0);
  const visible = usePageVisible();
  // The poll reads the latest list without restarting its timer.
  const state = useRef<FeedState>(emptyFeed());

  useEffect(() => {
    state.current = emptyFeed();
    setRows([]);
    setError(null);
  }, [filter]);

  useEffect(() => {
    if (!capturing || !visible || paused) return;
    let cancelled = false;
    // One read at a time: a read that starts while another waits could answer after it with older copies of the
    // same rows. `mergeRequests` also keeps the newest copy, so neither alone decides the order.
    let reading = false;
    // Typing waits for a pause before the first read.
    const delay = window.setTimeout(() => void poll(), 300);
    const timer = window.setInterval(() => void poll(), 1000);
    async function poll() {
      if (reading) return;
      reading = true;
      try {
        const since = nextSince(state.current.rows, state.current.lastSeq);
        const answer = await desktop.captureRequests(since, state.current.lastRev ?? 0, filter);
        if (cancelled) return;
        if (answer.error) {
          setError(filterError(answer.error));
          return;
        }
        setError(null);
        if (engineRestarted(state.current, answer)) {
          // A new engine (a changed setting, a reconnect): its counters start again, so the list does too. The
          // next read takes everything it holds.
          state.current = emptyFeed();
          setRows([]);
          setGeneration((count) => count + 1);
          return;
        }
        const fresh = state.current.lastSeq === 0 && state.current.lastRev == null;
        const next = mergeRequests(state.current.rows, answer);
        state.current = {
          ...state.current,
          rows: next,
          lastSeq: Math.max(state.current.lastSeq, answer.last_seq ?? 0),
          lastRev: nextRev(state.current.lastRev, answer, REQUEST_PAGE),
          dropped: Math.max(state.current.dropped, answer.dropped_before_seq ?? 0),
          // The first read of a list takes everything the engine holds, so only a later rise needs a full read.
          resyncDue: state.current.resyncDue || (!fresh && droppedMore(state.current.dropped, answer)),
        };
        setRows(next);
        if (state.current.resyncDue && Date.now() - state.current.resyncAt >= RESYNC_INTERVAL_MS) await resync();
      } catch (failure) {
        if (cancelled) return;
        const text = String(failure);
        setError(text.includes("invalid_filter") ? t("筛选表达式有误") : text);
      } finally {
        reading = false;
      }
    }
    // The engine dropped something: reads every row it still holds and lets go of the others, whatever their seq.
    async function resync() {
      // Every attempt counts, so a read that comes back incomplete (or fails) waits the interval before the next.
      state.current = { ...state.current, resyncAt: Date.now() };
      const full = await readAllKept((sinceSeq, sinceRev) => desktop.captureRequests(sinceSeq, sinceRev, filter), REQUEST_PAGE);
      if (cancelled || !full.complete || full.answers.some((answer) => engineRestarted(state.current, answer))) return;
      let rows = state.current.rows;
      const kept = new Set<string>();
      for (const answer of full.answers) {
        rows = mergeRequests(rows, answer);
        for (const request of answer.requests ?? []) kept.add(requestKey(request));
      }
      rows = keepOnly(rows, kept);
      const last = full.answers[full.answers.length - 1];
      state.current = {
        ...state.current,
        rows,
        lastSeq: Math.max(state.current.lastSeq, last?.last_seq ?? 0),
        dropped: Math.max(state.current.dropped, last?.dropped_before_seq ?? 0),
        resyncDue: false,
        resyncAt: Date.now(),
      };
      setRows(rows);
    }
    return () => {
      cancelled = true;
      window.clearTimeout(delay);
      window.clearInterval(timer);
    };
  }, [capturing, visible, paused, filter]);

  return { rows, error, generation };
}

/** The filter field and the quick filters above the table; both make one engine expression (`combineFilter`). */
export const RequestFilterBar = forwardRef<HTMLInputElement, {
  typed: string;
  onType: (text: string) => void;
  quick: QuickFilter[];
  onQuick: (quick: QuickFilter[]) => void;
  error: string | null;
  count: number;
}>(function RequestFilterBar({ typed, onType, quick, onQuick, error, count }, ref) {
  const toggle = (id: QuickFilter) => onQuick(quick.includes(id) ? quick.filter((item) => item !== id) : [...quick, id]);
  return (
    <div className="request-filters">
      <div className="request-filter-line">
        <label className={`search ${error ? "invalid" : ""}`}>
          <Search size={14} aria-hidden="true" />
          <input
            ref={ref}
            aria-label={t("筛选请求")}
            aria-invalid={Boolean(error)}
            placeholder={t("筛选：网址、host:、status:、type:…")}
            title={t("筛选示例：host:api.example.com status:4xx type:json -method:OPTIONS，多组条件用 OR 连接。")}
            spellCheck={false}
            value={typed}
            onChange={(event) => onType(event.target.value)}
          />
        </label>
        <span className="count" aria-live="polite">{t("{count} 个请求", { count })}</span>
      </div>
      <div className="quick-filters" role="group" aria-label={t("快速筛选")}>
        {quickFilters.map((item) => (
          <button
            key={item.id}
            className={`chip-button ${quick.includes(item.id) ? "selected" : ""}`}
            aria-pressed={quick.includes(item.id)}
            title={item.condition}
            onClick={() => toggle(item.id)}
          >
            {item.label()}
          </button>
        ))}
      </div>
      {error && <p className="muted danger-text" role="alert">{error}</p>}
    </div>
  );
});

/** Height of one table row in pixels; the table draws only the rows in view. */
const ROW = 28;
/** Height of the sticky header row (styles.css `.request-head`), which covers the top of the scrolled rows. */
const HEAD = 28;
/** Rows drawn above and below the visible ones, so fast scrolling does not show blanks. */
const OVERSCAN = 12;

type Column = { id: RequestSort["by"] | "method" | "type"; label: string; className: string; sortable: boolean };

const columns = (): Column[] => [
  { id: "seq", label: t("时间"), className: "col-time", sortable: true },
  { id: "method", label: t("方法"), className: "col-method", sortable: false },
  { id: "status", label: t("状态"), className: "col-status", sortable: true },
  { id: "host", label: t("主机"), className: "col-host", sortable: true },
  { id: "path", label: t("路径"), className: "col-path", sortable: true },
  { id: "type", label: t("类型"), className: "col-type", sortable: false },
  { id: "size", label: t("大小"), className: "col-size", sortable: true },
  { id: "duration", label: t("耗时"), className: "col-duration", sortable: true },
];

/**
 * The request table: one row per request, sortable by the column headers, newest first by default. Arrow keys, j/k,
 * Home and End move the selection while the table has focus; Enter opens the selected request.
 */
export function RequestTable({ rows, selected, onSelect, onOpen, sort, onSort }: {
  rows: CaptureRequest[];
  /** The selected request's `requestKey`. */
  selected: string | null;
  onSelect: (key: string) => void;
  /** Enter or a double click; on a narrow window this shows the request in place of the table. */
  onOpen: (key: string) => void;
  sort: RequestSort;
  onSort: (sort: RequestSort) => void;
}) {
  const sorted = useMemo(() => [...rows].sort((a, b) => compareRequests(a, b, sort)), [rows, sort]);
  const body = useRef<HTMLDivElement>(null);
  const [view, setView] = useState({ top: 0, height: 600 });

  useLayoutEffect(() => {
    const element = body.current;
    if (!element) return;
    const measure = () => setView({ top: element.scrollTop, height: element.clientHeight });
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const index = selected == null ? -1 : sorted.findIndex((row) => requestKey(row) === selected);

  // Keeps the selected row in view when the keyboard moves it. The table scrolls as a whole under its sticky
  // header, so a row is in view between the header's foot and the table's bottom edge.
  const reveal = (at: number) => {
    const element = body.current;
    if (!element) return;
    const top = at * ROW;
    if (top < element.scrollTop) element.scrollTop = top;
    else if (HEAD + top + ROW > element.scrollTop + element.clientHeight) element.scrollTop = HEAD + top + ROW - element.clientHeight;
  };

  const onKey = (event: KeyboardEvent<HTMLDivElement>) => {
    if (sorted.length === 0) return;
    const page = Math.max(1, Math.floor(view.height / ROW) - 1);
    let next: number | null = null;
    switch (event.key) {
      case "ArrowDown":
      case "j":
        next = Math.min(sorted.length - 1, index + 1);
        break;
      case "ArrowUp":
      case "k":
        next = Math.max(0, index < 0 ? 0 : index - 1);
        break;
      case "PageDown":
        next = Math.min(sorted.length - 1, Math.max(0, index) + page);
        break;
      case "PageUp":
        next = Math.max(0, index - page);
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = sorted.length - 1;
        break;
      case "Enter":
        if (selected != null) {
          event.preventDefault();
          onOpen(selected);
        }
        return;
      default:
        return;
    }
    event.preventDefault();
    onSelect(requestKey(sorted[next]));
    reveal(next);
  };

  const first = Math.max(0, Math.floor(view.top / ROW) - OVERSCAN);
  const last = Math.min(sorted.length, Math.ceil((view.top + view.height) / ROW) + OVERSCAN);
  const header = columns();

  return (
    // The grid is also the scrolling box (the header sticks to its top): the element that takes focus and the
    // keyboard is the one that scrolls, so the list can be scrolled from the keyboard as well as moved through.
    <div
      className="request-table"
      ref={body}
      onScroll={(event) => setView({ top: event.currentTarget.scrollTop, height: event.currentTarget.clientHeight })}
      role="grid"
      aria-label={t("请求列表")}
      aria-rowcount={sorted.length + 1}
      aria-activedescendant={selected != null ? rowId(selected) : undefined}
      tabIndex={0}
      onKeyDown={onKey}
    >
      <div className="request-head" role="row" aria-rowindex={1}>
        {header.map((column) => {
          const active = sort.by === column.id;
          const order = active ? (sort.descending ? "descending" : "ascending") : column.sortable ? "none" : undefined;
          return (
            <div key={column.id} role="columnheader" aria-sort={order} className={column.className}>
              {column.sortable ? (
                <button
                  tabIndex={-1}
                  onClick={() => onSort({ by: column.id as RequestSort["by"], descending: active ? !sort.descending : column.id === "seq" || column.id === "size" || column.id === "duration" })}
                >
                  {column.label}
                  {active && (sort.descending ? <ArrowDown size={11} aria-hidden="true" /> : <ArrowUp size={11} aria-hidden="true" />)}
                </button>
              ) : column.label}
            </div>
          );
        })}
      </div>
      <div className="request-body">
        <div style={{ height: sorted.length * ROW, position: "relative" }}>
          {sorted.slice(first, last).map((request, offset) => {
            const key = requestKey(request);
            return (
              <RequestRow
                key={key}
                request={request}
                top={(first + offset) * ROW}
                rowIndex={first + offset + 2}
                selected={key === selected}
                onSelect={() => onSelect(key)}
                onOpen={() => onOpen(key)}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
}

function RequestRow({ request, top, rowIndex, selected, onSelect, onOpen }: {
  request: CaptureRequest;
  top: number;
  rowIndex: number;
  selected: boolean;
  onSelect: () => void;
  onOpen: () => void;
}) {
  const took = durationMs(request.started_at_ms, request.ended_at_ms);
  const tone = statusTone(request);
  const note = noteText(request.note);
  const path = requestPath(request);
  return (
    <div
      id={rowId(requestKey(request))}
      role="row"
      aria-rowindex={rowIndex}
      aria-selected={selected}
      className={`request-row ${selected ? "selected" : ""}`}
      style={{ top, height: ROW }}
      onMouseDown={onSelect}
      onDoubleClick={onOpen}
      title={note ? `${request.url}\n${note}` : request.url}
    >
      <span role="gridcell" className="col-time">{timeOfDay(request.started_at_ms)}</span>
      <span role="gridcell" className="col-method">{request.method}</span>
      <span role="gridcell" className={`col-status ${tone}`}>
        {request.status != null ? request.status : tone === "pending" ? "…" : t("失败")}
      </span>
      <span role="gridcell" className="col-host">{request.host}</span>
      <span role="gridcell" className="col-path">{path}</span>
      <span role="gridcell" className="col-type">{typeText(request)}</span>
      <span role="gridcell" className="col-size">{request.response_size > 0 ? formatBytes(request.response_size) : ""}</span>
      <span role="gridcell" className="col-duration">{took != null ? `${took} ms` : ""}</span>
    </div>
  );
}

/** The DOM id of a request's row, for the table's active descendant. */
function rowId(key: string): string {
  return `request-${key.replace(":", "-")}`;
}

/** h2, the kind of body, SSE / WS with how many messages, and a rewrite that did not run, in one short cell. */
function typeText(request: CaptureRequest): string {
  const stream = streamKind(request);
  return [
    request.http_version === "HTTP/2" ? "h2" : null,
    shortType(request.response_content_type),
    stream ? (request.message_count ? `${stream} ${request.message_count}` : stream) : null,
    request.rules_skipped ? t("改写未生效") : null,
  ].filter(Boolean).join(" · ");
}

/** "application/json; charset=utf-8" → "json". */
function shortType(type: string | null | undefined): string | null {
  if (!type) return null;
  const main = type.split(";")[0].trim().toLowerCase();
  const sub = main.split("/")[1] ?? main;
  return sub.replace(/^x-/, "").replace(/\+.*$/, "") || null;
}
