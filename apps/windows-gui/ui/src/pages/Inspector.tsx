import { ArrowDown, ArrowUp, ChevronLeft, FileSearch } from "lucide-react";
import { ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { CaptureExchange, CaptureMessage, CaptureRequest, desktop, H2Frame } from "../bridge";
import { queryPairs, waterfall } from "../body-view";
import {
  bodySize,
  durationMs,
  frameDetail,
  hasMessages,
  header,
  headerLines,
  messageText,
  messageTitle,
  mergeMessageList,
  MessageList,
  notableFrame,
  noteText,
  skippedText,
  timeOfDay,
} from "../capture-format";
import { usePageVisible } from "../live";
import { requestPath, statusTone } from "../request-table";
import { formatBytes } from "../subscription-format";
import { CopyButton, EmptyState, Segmented, Tabs } from "../ui";
import { t } from "../i18n";
import { BodyViewer, PairTable } from "./BodyViewer";

export type InspectorTab = "overview" | "request" | "response" | "messages" | "frames" | "timeline";
type Part = "headers" | "query" | "body";

/** What the inspector's top line shows; known from the table row before the request's content has loaded. */
interface Summary {
  method: string;
  url: string;
  status?: number | null;
  note?: string | null;
  started_at_ms: number;
  ended_at_ms?: number | null;
  state?: "in_progress" | "done";
  rules_skipped?: boolean;
}

/** Where the exchange sits in the capture, for reading its messages and its connection's HTTP/2 frames. */
export interface ExchangePlace {
  connectionId: number;
  index: number;
  /** HTTP/2 frame headers seen on the connection; 0 when it is not HTTP/2. */
  frames: number;
}

/**
 * One request and its response in tabs, after Proxyman's inspector: an overview, the request, the response and
 * where the request sits in time on its connection. The top line and the overview come from the table row, so they
 * are there at once; headers and bodies fill in when the content has been read. The tab and the headers/body choice
 * stay as the user moves through the table.
 */
export function ExchangeInspector({ request, exchange, siblings, place, waiting, onBack, backLabel, actions, exportCard }: {
  /** The table row; absent when the inspector is opened from a connection's detail. */
  request?: CaptureRequest;
  /** The content; null while it is being read or when capture no longer holds it. */
  exchange: CaptureExchange | null;
  /** The requests of the same connection, for the timeline. */
  siblings?: CaptureExchange[];
  /** Where it is in the capture; without it there are no messages or frames tabs. */
  place?: ExchangePlace;
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
  const messages = Boolean(place && exchange && hasMessages(exchange));
  const frames = Boolean(place && place.frames > 0);
  // A tab remembered from another request that this one does not have shows the response.
  const shown: InspectorTab = (tab === "messages" && !messages) || (tab === "frames" && !frames) ? "response" : tab;
  let panel: ReactNode;
  if (shown === "overview") {
    panel = <Overview summary={summary} request={request} exchange={exchange} exportCard={exportCard} />;
  } else if (!exchange) {
    panel = pending;
  } else if (shown === "messages" && place) {
    panel = <MessagesPanel key={`${place.connectionId}-${place.index}-${exchange.started_at_ms}`} exchange={exchange} place={place} />;
  } else if (shown === "frames" && place) {
    panel = <FramesPanel connectionId={place.connectionId} />;
  } else if (shown === "request") {
    panel = <MessagePanel side="request" url={exchange.url} message={exchange.request} part={requestPart} onPart={setRequestPart} />;
  } else if (shown === "response") {
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
          { value: "overview" as const, label: t("概览") },
          { value: "request" as const, label: t("请求") },
          { value: "response" as const, label: t("响应") },
          ...(messages ? [{ value: "messages" as const, label: exchange?.status === 101 ? "WebSocket" : t("事件"), badge: exchange?.messages?.seen || undefined }] : []),
          ...(frames ? [{ value: "frames" as const, label: t("HTTP/2 帧") }] : []),
          { value: "timeline" as const, label: t("时间线") },
        ]}
        value={shown}
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
  if (summary.state === "in_progress") facts.push([t("状态"), t("进行中")]);
  if (type) facts.push([t("类型"), type]);
  if (requestBytes) facts.push([t("请求体"), requestBytes]);
  if (responseBytes) facts.push([t("响应体"), responseBytes]);
  if (note) facts.push([t("说明"), note]);
  if (summary.rules_skipped) facts.push([t("改写"), <span className="warning-text">{skippedText(summary.note)}</span>]);
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

  if (shown === "body" && hasBody) return <BodyViewer message={message} lead={lead} side={side} />;
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
      <div className="body-content" tabIndex={0} role="region" aria-label={t("正文")}>
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

/**
 * The exchange's server-sent events or WebSocket messages, oldest first, read again every second while it is under
 * way. They are shown as they went by: decryption only looks at them.
 */
function MessagesPanel({ exchange, place }: { exchange: CaptureExchange; place: ExchangePlace }) {
  const [list, setList] = useState<MessageList>(() => mergeMessageList({ messages: [], missed: 0, seen: 0 }, exchange.messages));
  const latest = useRef(list);
  latest.current = list;
  // Each newer copy of the exchange from the inspector carries the kept messages too. The panel's own reads stop
  // when the stream ends, so the messages that came after the last one (an SSE `[DONE]`, a WebSocket Close) and the
  // final count arrive this way.
  useEffect(() => {
    const updated = mergeMessageList(latest.current, exchange.messages);
    if (updated === latest.current) return;
    latest.current = updated;
    setList(updated);
  }, [exchange.messages]);
  const [open, setOpen] = useState<number | null>(null);
  const visible = usePageVisible();
  const running = exchange.state === "in_progress";
  const { connectionId, index } = place;
  const startedAt = exchange.started_at_ms;

  useEffect(() => {
    if (!running || !visible) return;
    let cancelled = false;
    const timer = window.setInterval(() => {
      const current = latest.current;
      const since = current.messages.length > 0 ? current.messages[current.messages.length - 1].index + 1 : 0;
      void desktop.captureExchange(connectionId, index, since).then((answer) => {
        const item = answer.exchange;
        if (cancelled || !item || item.started_at_ms !== startedAt) return;
        const updated = mergeMessageList(latest.current, item.messages);
        if (updated === latest.current) return;
        latest.current = updated;
        setList(updated);
      }).catch(() => undefined);
    }, 1000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [running, visible, connectionId, index, startedAt]);

  const websocket = exchange.status === 101;
  const summary = [t("共 {count} 条", { count: list.seen })];
  if (list.missed > 0) summary.push(t("较早的 {count} 条未保留", { count: list.missed }));
  if (running) summary.push(t("进行中"));
  return (
    <div className="inspector-scroll">
      <p className="muted inspector-note">{summary.join(" · ")}</p>
      {list.messages.length === 0
        ? <p className="muted inspector-note">{running ? t("等待消息…") : t("没有消息")}</p>
        : (
          <ol className="stream-messages">
            {list.messages.map((message) => (
              <li key={message.index} className={message.dir === "up" ? "up" : "down"}>
                <button className="stream-message-head" aria-expanded={open === message.index} onClick={() => setOpen(open === message.index ? null : message.index)}>
                  {message.dir === "up"
                    ? <ArrowUp size={12} aria-label={t("App 发出")} />
                    : <ArrowDown size={12} aria-label={t("收到")} />}
                  <b>{messageTitle(message)}</b>
                  {message.kind !== "sse" && message.label && <span className="warning-text">{t("压缩消息未能解压")}</span>}
                  {message.id && <span className="muted">id: {message.id}</span>}
                  <span className="bytes-spacer" />
                  <span className="muted">{formatBytes(message.size)}{message.truncated && <> · {t("已截断")}</>}</span>
                  <span className="muted">{timeOfDay(message.at_ms)}</span>
                </button>
                <pre className={`body-text selectable ${open === message.index ? "" : "clamped"}`}>{messageText(message) || " "}</pre>
                {open === message.index && <CopyButton label={t("拷贝")} text={() => messageText(message)} />}
              </li>
            ))}
          </ol>
        )}
      <p className="muted inspector-note">
        {websocket
          ? t("只读查看：消息原样转发，不能改写。每个请求保留最近 512 条、每条前 4 KiB。")
          : t("事件原样逐条转发给 App。每个请求保留最近 512 条、每条前 4 KiB。")}
      </p>
    </div>
  );
}

/**
 * The HTTP/2 frame headers of the exchange's connection on both links, App ↔ AnixOps and AnixOps ↔ server, read
 * again every two seconds. Only headers, error codes and window increments are kept, never payloads.
 */
function FramesPanel({ connectionId }: { connectionId: number }) {
  const [frames, setFrames] = useState<H2Frame[]>([]);
  const [seen, setSeen] = useState(0);
  const [link, setLink] = useState<"all" | "app" | "origin">("all");
  const [notableOnly, setNotableOnly] = useState(false);
  const [gone, setGone] = useState(false);
  const visible = usePageVisible();

  useEffect(() => {
    setFrames([]);
    setSeen(0);
    setGone(false);
  }, [connectionId]);

  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    let kept: H2Frame[] = [];
    const load = () => {
      const since = kept.length > 0 ? kept[kept.length - 1].index + 1 : 0;
      void desktop.captureH2Frames(connectionId, since).then((answer) => {
        if (cancelled) return;
        if (!answer.h2_frames) {
          setGone(kept.length === 0);
          return;
        }
        kept = [...kept, ...answer.h2_frames.frames.filter((frame) => frame.index >= since)].slice(-2000);
        setFrames(kept);
        setSeen(answer.h2_frames.seen);
      }).catch(() => undefined);
    };
    load();
    const timer = window.setInterval(load, 2000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [connectionId, visible]);

  const shown = useMemo(
    () => frames.filter((frame) => (link === "all" || frame.link === link) && (!notableOnly || notableFrame(frame))),
    [frames, link, notableOnly],
  );
  const dropped = Math.max(0, seen - frames.length);
  return (
    <div className="inspector-scroll">
      <div className="inspector-tools">
        <Segmented
          label={t("链路")}
          className="compact"
          options={[
            { value: "all", label: t("全部") },
            { value: "app", label: "App ↔ AnixOps" },
            { value: "origin", label: t("AnixOps ↔ 服务器") },
          ]}
          value={link}
          onChange={setLink}
        />
        <label className="check-label">
          <input type="checkbox" checked={notableOnly} onChange={(event) => setNotableOnly(event.target.checked)} />
          {t("只看 RST_STREAM、GOAWAY 和错误")}
        </label>
      </div>
      <p className="muted inspector-note">
        {[t("共 {count} 帧", { count: seen }), dropped > 0 ? t("较早的 {count} 帧未保留", { count: dropped }) : null].filter(Boolean).join(" · ")}
      </p>
      {shown.length === 0
        ? <p className="muted inspector-note">{gone ? t("这个连接的抓包内容已被更新的连接挤出（抓包内存有上限），或抓包已清空。") : t("没有符合条件的帧")}</p>
        : (
          <table className="h2-frames">
            <thead>
              <tr>
                <th>{t("时间")}</th>
                <th>{t("方向")}</th>
                <th>{t("类型")}</th>
                <th>{t("流")}</th>
                <th>{t("长度")}</th>
                <th>{t("标志")}</th>
                <th>{t("详情")}</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((frame) => (
                <tr key={frame.index} className={notableFrame(frame) ? "notable" : undefined}>
                  <td>{timeOfDay(frame.at_ms)}</td>
                  <td>{frameDirection(frame)}</td>
                  <td className="mono">{frame.type}</td>
                  <td className="mono">{frame.stream}</td>
                  <td className="mono">{frame.length}</td>
                  <td className="mono">{frame.flag_names.join(" ")}</td>
                  <td className="mono">{frameDetail(frame) ?? ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      <p className="muted inspector-note">{t("只记帧头（类型、标志、流、长度）和错误码、窗口增量等，不保存帧内容。每个连接保留最近 512 帧。")}</p>
    </div>
  );
}

/** Which link and which way a frame went. */
function frameDirection(frame: H2Frame): string {
  if (frame.link === "app") return frame.dir === "up" ? "App →" : "→ App";
  return frame.dir === "up" ? t("→ 服务器") : t("服务器 →");
}
