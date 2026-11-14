import { ArrowDown, ArrowUp, Search } from "lucide-react";
import { forwardRef, type KeyboardEvent, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { CaptureRequest, desktop } from "../bridge";
import { combineFilter, mergeRequests, nextSince, QuickFilter, quickFilters } from "../capture-filter";
import { durationMs, noteText, timeOfDay } from "../capture-format";
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

export interface RequestFeed {
  /** Oldest first, as the engine numbers them. */
  rows: CaptureRequest[];
  error: string | null;
}

/**
 * The decrypted requests of every connection, read from the engine once a second while capture holds any and the
 * list is not paused. A new filter starts the list over.
 */
export function useRequestFeed(capturing: boolean, filter: string, paused: boolean): RequestFeed {
  const [rows, setRows] = useState<CaptureRequest[]>([]);
  const [error, setError] = useState<string | null>(null);
  const visible = usePageVisible();
  // The poll reads the latest list without restarting its timer.
  const state = useRef<{ rows: CaptureRequest[]; lastSeq: number }>({ rows: [], lastSeq: 0 });

  useEffect(() => {
    state.current = { rows: [], lastSeq: 0 };
    setRows([]);
    setError(null);
  }, [filter]);

  useEffect(() => {
    if (!capturing || !visible || paused) return;
    let cancelled = false;
    // Typing waits for a pause before the first read.
    const delay = window.setTimeout(() => void poll(), 300);
    const timer = window.setInterval(() => void poll(), 1000);
    async function poll() {
      try {
        const since = nextSince(state.current.rows, state.current.lastSeq);
        const answer = await desktop.captureRequests(since, filter);
        if (cancelled) return;
        if (answer.error) {
          setError(filterError(answer.error));
          return;
        }
        setError(null);
        const next = mergeRequests(state.current.rows, answer);
        state.current = { rows: next, lastSeq: Math.max(state.current.lastSeq, answer.last_seq ?? 0) };
        setRows(next);
      } catch (failure) {
        if (cancelled) return;
        const text = String(failure);
        setError(text.includes("invalid_filter") ? t("筛选表达式有误") : text);
      }
    }
    return () => {
      cancelled = true;
      window.clearTimeout(delay);
      window.clearInterval(timer);
    };
  }, [capturing, visible, paused, filter]);

  return { rows, error };
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
  selected: number | null;
  onSelect: (seq: number) => void;
  /** Enter or a double click; on a narrow window this shows the request in place of the table. */
  onOpen: (seq: number) => void;
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

  const index = selected == null ? -1 : sorted.findIndex((row) => row.seq === selected);

  // Keeps the selected row in view when the keyboard moves it.
  const reveal = (at: number) => {
    const element = body.current;
    if (!element) return;
    const top = at * ROW;
    if (top < element.scrollTop) element.scrollTop = top;
    else if (top + ROW > element.scrollTop + element.clientHeight) element.scrollTop = top + ROW - element.clientHeight;
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
    onSelect(sorted[next].seq);
    reveal(next);
  };

  const first = Math.max(0, Math.floor(view.top / ROW) - OVERSCAN);
  const last = Math.min(sorted.length, Math.ceil((view.top + view.height) / ROW) + OVERSCAN);
  const header = columns();

  return (
    <div
      className="request-table"
      role="grid"
      aria-label={t("请求列表")}
      aria-rowcount={sorted.length + 1}
      aria-activedescendant={selected != null ? `request-${selected}` : undefined}
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
      <div className="request-body" ref={body} onScroll={(event) => setView({ top: event.currentTarget.scrollTop, height: event.currentTarget.clientHeight })}>
        <div style={{ height: sorted.length * ROW, position: "relative" }}>
          {sorted.slice(first, last).map((request, offset) => (
            <RequestRow
              key={request.seq}
              request={request}
              top={(first + offset) * ROW}
              rowIndex={first + offset + 2}
              selected={request.seq === selected}
              onSelect={() => onSelect(request.seq)}
              onOpen={() => onOpen(request.seq)}
            />
          ))}
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
      id={`request-${request.seq}`}
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
      <span role="gridcell" className="col-type">{[request.http_version === "HTTP/2" ? "h2" : null, shortType(request.response_content_type)].filter(Boolean).join(" · ")}</span>
      <span role="gridcell" className="col-size">{request.response_size > 0 ? formatBytes(request.response_size) : ""}</span>
      <span role="gridcell" className="col-duration">{took != null ? `${took} ms` : ""}</span>
    </div>
  );
}

/** "application/json; charset=utf-8" → "json". */
function shortType(type: string | null | undefined): string | null {
  if (!type) return null;
  const main = type.split(";")[0].trim().toLowerCase();
  const sub = main.split("/")[1] ?? main;
  return sub.replace(/^x-/, "").replace(/\+.*$/, "") || null;
}
