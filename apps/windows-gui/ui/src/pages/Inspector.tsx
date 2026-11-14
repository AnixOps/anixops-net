import { ChevronLeft, FileSearch } from "lucide-react";
import { ReactNode, useState } from "react";
import type { CaptureExchange, CaptureMessage, CaptureRequest } from "../bridge";
import { queryPairs, waterfall } from "../body-view";
import { bodySize, durationMs, header, headerLines, noteText, timeOfDay } from "../capture-format";
import { requestPath, statusTone } from "../request-table";
import { formatBytes } from "../subscription-format";
import { CopyButton, EmptyState, Segmented, Tabs } from "../ui";
import { t } from "../i18n";
import { BodyViewer, PairTable } from "./BodyViewer";

export type InspectorTab = "overview" | "request" | "response" | "timeline";
type Part = "headers" | "query" | "body";

/** What the inspector's top line shows; known from the table row before the request's content has loaded. */
interface Summary {
  method: string;
  url: string;
  status?: number | null;
  note?: string | null;
  started_at_ms: number;
  ended_at_ms?: number | null;
}

/**
 * One request and its response in tabs, after Proxyman's inspector: an overview, the request, the response and
 * where the request sits in time on its connection. The top line and the overview come from the table row, so they
 * are there at once; headers and bodies fill in when the content has been read. The tab and the headers/body choice
 * stay as the user moves through the table.
 */
export function ExchangeInspector({ request, exchange, siblings, waiting, onBack, backLabel, actions, exportCard }: {
  /** The table row; absent when the inspector is opened from a connection's detail. */
  request?: CaptureRequest;
  /** The content; null while it is being read or when capture no longer holds it. */
  exchange: CaptureExchange | null;
  /** The requests of the same connection, for the timeline. */
  siblings?: CaptureExchange[];
  /** Shown in place of the content while `exchange` is null. */
  waiting?: string;
  onBack?: () => void;
  backLabel?: string;
  actions?: ReactNode;
  exportCard?: ReactNode;
}) {
  const [tab, setTab] = useState<InspectorTab>("response");
  const [requestPart, setRequestPart] = useState<Part>("headers");
  const [responsePart, setResponsePart] = useState<Part>("body");
  const summary: Summary | null = exchange ?? request ?? null;
  if (!summary) return null;
  const tone = statusTone({ status: summary.status, ended_at_ms: summary.ended_at_ms, note: summary.note });
  const answered = exchange?.status != null;
  const failed = t("失败");
  const statusText = summary.status != null ? summary.status : tone === "pending" ? "…" : tone === "neutral" ? "–" : failed;

  const pending = <div className="inspector-wait"><EmptyState icon={FileSearch} title={t("抓包内容")} message={waiting ?? t("正在读取…")} /></div>;
  let panel: ReactNode;
  if (tab === "overview") {
    panel = <Overview summary={summary} request={request} exchange={exchange} exportCard={exportCard} />;
  } else if (!exchange) {
    panel = pending;
  } else if (tab === "request") {
    panel = <MessagePanel side="request" url={exchange.url} message={exchange.request} part={requestPart} onPart={setRequestPart} />;
  } else if (tab === "response") {
    panel = answered
      ? <MessagePanel side="response" message={exchange.response} part={responsePart} onPart={setResponsePart} />
      : <div className="inspector-wait"><EmptyState icon={FileSearch} title={t("还没有响应")} message={noteText(exchange.note) ?? t("响应到达后显示在这里。")} /></div>;
  } else {
    panel = <Timeline exchange={exchange} siblings={siblings ?? [exchange]} />;
  }

  return (
    <div className="inspector">
      <header className="inspector-head">
        {onBack && <button className="icon-button" title={backLabel ?? t("返回")} aria-label={backLabel ?? t("返回")} onClick={onBack}><ChevronLeft size={16} /></button>}
        <span className="method-badge">{summary.method}</span>
        <span className={["status-badge", tone].join(" ")}>{statusText}</span>
        <span className="inspector-url selectable" title={summary.url}>{hostAndPath(summary.url)}</span>
        {actions && <span className="inspector-actions">{actions}</span>}
      </header>
      <Tabs
        label={t("请求详情")}
        options={[
          { value: "overview", label: t("概览") },
          { value: "request", label: t("请求") },
          { value: "response", label: t("响应") },
          { value: "timeline", label: t("时间线") },
        ]}
        value={tab}
        onChange={setTab}
      />
      <div className="inspector-panel" role="tabpanel">{panel}</div>
    </div>
  );
}

/** "https://api.example.com/v2/feed?x=1" → "api.example.com/v2/feed?x=1": the scheme is in the overview. */
function hostAndPath(url: string): string {
  return url.replace(/^[a-z][a-z0-9+.-]*:\/\//i, "");
}

function Overview({ summary, request, exchange, exportCard }: { summary: Summary; request?: CaptureRequest; exchange: CaptureExchange | null; exportCard?: ReactNode }) {
  const took = durationMs(summary.started_at_ms, summary.ended_at_ms);
  const note = noteText(summary.note);
  const type = exchange ? header(exchange.response, "Content-Type") : request?.response_content_type;
  const requestBytes = exchange ? (exchange.request.body.size > 0 ? bodySize(exchange.request.body) : null) : null;
  const responseBytes = exchange
    ? (exchange.response.body.size > 0 ? bodySize(exchange.response.body) : null)
    : request && request.response_size > 0 ? formatBytes(request.response_size) : null;
  const facts: Array<[string, ReactNode]> = [
    [t("网址"), <span className="mono wrap">{summary.url}</span>],
    [t("方法"), summary.method],
    [t("状态"), summary.status != null ? String(summary.status) : t("没有响应")],
  ];
  if (request?.http_version) facts.push([t("协议"), request.http_version]);
  facts.push([t("开始"), timeOfDay(summary.started_at_ms)]);
  if (took != null) facts.push([t("耗时"), `${took} ms`]);
  if (type) facts.push([t("类型"), type]);
  if (requestBytes) facts.push([t("请求体"), requestBytes]);
  if (responseBytes) facts.push([t("响应体"), responseBytes]);
  if (note) facts.push([t("说明"), note]);
  return (
    <div className="inspector-scroll">
      <dl className="facts-list selectable">
        {facts.map(([name, value]) => <div key={name}><dt>{name}</dt><dd>{value}</dd></div>)}
      </dl>
      {exportCard}
    </div>
  );
}

function MessagePanel({ side, url, message, part, onPart }: {
  side: "request" | "response";
  /** The request's URL, whose query parameters get their own view. */
  url?: string;
  message: CaptureMessage;
  part: Part;
  onPart: (part: Part) => void;
}) {
  const query = url ? queryPairs(url) : [];
  const hasBody = message.body.size > 0;
  const options: Array<{ value: Part; label: string }> = [{ value: "headers", label: t("头 {count}", { count: message.headers.length }) }];
  if (query.length > 0) options.push({ value: "query", label: t("查询 {count}", { count: query.length }) });
  options.push({ value: "body", label: t("正文") });
  // The choice is remembered across requests; one that does not exist here falls back to the headers.
  const shown: Part = part === "query" && query.length === 0 ? "headers" : part;
  const lead = <Segmented label={side === "request" ? t("请求的部分") : t("响应的部分")} className="compact" options={options} value={shown} onChange={onPart} />;

  if (shown === "body" && hasBody) return <BodyViewer message={message} lead={lead} />;
  const pairs = shown === "query" ? query : message.headers;
  return (
    <div className="body-viewer">
      <div className="inspector-tools">
        {lead}
        <span className="bytes-spacer" />
        {shown !== "body" && pairs.length > 0 && (
          <CopyButton label={shown === "query" ? t("拷贝全部参数") : t("拷贝全部头")} text={() => headerLines(pairs)} />
        )}
      </div>
      <div className="body-content">
        {shown === "body"
          ? <p className="muted inspector-note">{side === "request" ? t("这个请求没有正文。") : t("这个响应没有正文。")}</p>
          : <PairTable pairs={pairs} />}
      </div>
    </div>
  );
}

/**
 * The request among the others on its connection, on one time axis. The engine records when each request started
 * and ended, so the bars show order, overlap and length; it does not time DNS, connect or TLS apart.
 */
function Timeline({ exchange, siblings }: { exchange: CaptureExchange; siblings: CaptureExchange[] }) {
  const bars = waterfall(siblings, Date.now());
  const took = durationMs(exchange.started_at_ms, exchange.ended_at_ms);
  const running = t("进行中");
  const tookText = took != null ? `${took} ms` : running;
  return (
    <div className="inspector-scroll">
      <dl className="facts-list">
        <div><dt>{t("开始")}</dt><dd>{timeOfDay(exchange.started_at_ms)}</dd></div>
        <div><dt>{t("结束")}</dt><dd>{exchange.ended_at_ms != null ? timeOfDay(exchange.ended_at_ms) : running}</dd></div>
        <div><dt>{t("耗时")}</dt><dd>{tookText}</dd></div>
      </dl>
      <h3 className="inspector-caption">{t("同一连接上的 {count} 个请求", { count: siblings.length })}</h3>
      <ol className="waterfall">
        {siblings.map((item, index) => {
          const bar = bars[index];
          const current = item === exchange || (item.started_at_ms === exchange.started_at_ms && item.url === exchange.url && item.method === exchange.method);
          return (
            <li key={`${item.started_at_ms}-${index}`} className={current ? "current" : ""} aria-current={current ? "true" : undefined}>
              <span className="waterfall-name" title={item.url}><b>{item.method}</b>{requestPath(item)}</span>
              <span className="waterfall-track">
                <i className={bar.took == null ? "running" : ""} style={{ left: `${bar.left * 100}%`, width: `${bar.width * 100}%` }} />
              </span>
              <span className="waterfall-took">{bar.took != null ? `${bar.took} ms` : "…"}</span>
            </li>
          );
        })}
      </ol>
    </div>
  );
}
