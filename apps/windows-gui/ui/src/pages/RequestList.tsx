import { ChevronRight, FileSearch, Search } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { CaptureRequest, desktop } from "../bridge";
import { combineFilter, mergeRequests, nextSince, QuickFilter, quickFilters } from "../capture-filter";
import { durationMs, noteText } from "../capture-format";
import { usePageVisible } from "../live";
import { formatBytes } from "../subscription-format";
import { Card, EmptyState } from "../ui";
import { t } from "../i18n";

/** Rows shown at first; more on request. */
const PAGE = 200;

/** Why the engine refused the expression: the position of the mistake when it says. */
function filterError(error: { code: string; at?: number; message?: string }): string {
  if (error.code === "invalid_filter") {
    return error.at != null ? t("筛选表达式在第 {at} 个字符处有误", { at: error.at + 1 }) : t("筛选表达式有误");
  }
  return error.message || error.code;
}

/**
 * The decrypted requests of every connection, newest first, read from the engine once a second while capture is on.
 * The filter is the engine's expression (host:, status:, type: …); the quick filters add conditions to it.
 */
export function RequestList({ capturing, onOpen }: { capturing: boolean; onOpen: (request: CaptureRequest) => void }) {
  const [typed, setTyped] = useState("");
  const [quick, setQuick] = useState<QuickFilter[]>([]);
  const [rows, setRows] = useState<CaptureRequest[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [limit, setLimit] = useState(PAGE);
  const visible = usePageVisible();
  const filter = combineFilter(quick, typed);
  // The poll reads the latest list without restarting its timer.
  const state = useRef<{ rows: CaptureRequest[]; lastSeq: number }>({ rows: [], lastSeq: 0 });

  useEffect(() => {
    // A new filter starts the list over.
    state.current = { rows: [], lastSeq: 0 };
    setRows([]);
    setError(null);
    setLimit(PAGE);
  }, [filter]);

  useEffect(() => {
    if (!capturing || !visible) return;
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
  }, [capturing, visible, filter]);

  const toggle = (id: QuickFilter) => setQuick((current) => (current.includes(id) ? current.filter((item) => item !== id) : [...current, id]));
  const newest = [...rows].reverse();

  return (
    <Card
      title={t("请求")}
      description={t("只列出已解密的 HTTP 请求。筛选示例：host:api.example.com status:4xx type:json -method:OPTIONS，多组条件用 OR 连接。")}
    >
      <div className="row toolbar request-toolbar">
        <label className={`search ${error ? "invalid" : ""}`}>
          <Search size={14} aria-hidden="true" />
          <input
            aria-label={t("筛选请求")}
            aria-invalid={Boolean(error)}
            placeholder={t("筛选：网址、host:、status:、type:…")}
            spellCheck={false}
            value={typed}
            onChange={(event) => setTyped(event.target.value)}
          />
        </label>
        <span className="count">{rows.length}</span>
      </div>
      <div className="row quick-filters" role="group" aria-label={t("快速筛选")}>
        {quickFilters.map((item) => (
          <button
            key={item.id}
            className={`chip-button ${quick.includes(item.id) ? "selected" : ""}`}
            aria-pressed={quick.includes(item.id)}
            onClick={() => toggle(item.id)}
          >
            {item.label()}
          </button>
        ))}
      </div>
      {error && <p className="muted danger-text">{error}</p>}
      {!capturing ? (
        <EmptyState icon={FileSearch} title={t("还没有开始抓包")} message={t("打开下方连接卡片里的“抓包”，并为要看的域名开启 HTTPS 解密。")} />
      ) : newest.length === 0 ? (
        <p className="muted">{filter ? t("没有匹配的请求。") : t("还没有解密的请求。")}</p>
      ) : (
        <>
          <ul className="list requests">
            {newest.slice(0, limit).map((request) => (
              <li key={request.seq} className="openable">
                <button className="list-main list-open" onClick={() => onOpen(request)}>
                  <strong className="truncate" title={request.url}><span className="method">{request.method}</span>{request.url}</strong>
                  <small>{requestHint(request)}</small>
                </button>
                <ChevronRight size={15} className="chevron" aria-hidden="true" />
              </li>
            ))}
          </ul>
          {newest.length > limit && (
            <button className="link-button more" onClick={() => setLimit((current) => current + PAGE)}>
              {t("再显示 {count} 条", { count: Math.min(PAGE, newest.length - limit) })}
            </button>
          )}
        </>
      )}
    </Card>
  );
}

function requestHint(request: CaptureRequest): string {
  const took = durationMs(request.started_at_ms, request.ended_at_ms);
  return [
    request.status != null ? String(request.status) : t("等待响应"),
    request.http_version === "HTTP/2" ? "h2" : null,
    took != null ? `${took} ms` : null,
    request.response_size > 0 ? formatBytes(request.response_size) : null,
    request.response_content_type,
    noteText(request.note),
  ].filter(Boolean).join(" · ");
}
