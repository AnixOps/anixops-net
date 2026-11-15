import { ArrowDownLeft, ArrowUpRight, ChevronLeft, Copy, Download, FileCode, FileSearch, Lock, MousePointerClick, Network, RotateCw } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { CaptureChunk, CaptureConnection, CaptureExchange, CaptureFormat, CaptureRequest, CodeLanguage, ConnectionSnapshot, DebugMapping, desktop, ExportOptions } from "../bridge";
import { connectionName } from "../activity-format";
import { asText, directionTotals, durationMs, fromBase64, header, hexDump, looksLikeTls, noteText, timeOfDay } from "../capture-format";
import { usePageVisible } from "../live";
import { formatBytes } from "../subscription-format";
import { Card, CopyButton, EmptyState, MenuItem, MoreMenu, Row, Run, Segmented, ToggleRow } from "../ui";
import { Select } from "../controls";
import { ExchangeInspector } from "./Inspector";
import { requestKey } from "../capture-filter";
import { t } from "../i18n";
import { mappingFromExchange, MappingEditor, ReplayEditor } from "./DebugTools";
import { coveredBy, decryptHostOf, isDecryptableHost } from "../decrypt-hosts";
import { useDecryptHosts } from "./DecryptHosts";

/** Saves an export to Downloads and shows where it went. */
export function exportCapture(run: Run, format: CaptureFormat, id?: number, options?: ExportOptions) {
  void run(`export-${format}`, async () => {
    const saved = await desktop.exportCapture(format, id, options);
    return { message: t("已保存到 {location}", { location: saved.location }) };
  });
}

/** The way back to the list above a detail view. */
export function BackBar({ label, onBack }: { label: string; onBack: () => void }) {
  return (
    <div className="back-bar">
      <button className="link-button" onClick={onBack}><ChevronLeft size={14} />{label}</button>
    </div>
  );
}

/**
 * What capture kept of one connection: the decrypted HTTP requests and the raw bytes each way, with exports of this
 * connection alone. Read again every two seconds while the connection is open.
 */
export function CaptureDetail({ connection, busy, run, onBack, initialExchange }: {
  connection: ConnectionSnapshot;
  /** Opens straight on this request of the connection (from the request list). */
  initialExchange?: number;
  busy: string | null;
  run: Run;
  onBack: () => void;
}) {
  const [capture, setCapture] = useState<CaptureConnection | null>(null);
  const [state, setState] = useState<"loading" | "loaded" | "unreachable">("loading");
  const [exchange, setExchange] = useState<number | null>(initialExchange ?? null);
  const [redact, setRedact] = useState(true);
  const [tool, setTool] = useState<{ replay: CaptureExchange } | { mapping: DebugMapping } | null>(null);
  const [decryptHosts, reloadDecryptHosts] = useDecryptHosts();
  const visible = usePageVisible();
  const open = connection.ended_at_ms == null;
  /** The connection's domain while the decrypted-domain list does not cover it; null for an address or a covered one. */
  const host = connection.host;
  const undecrypted = isDecryptableHost(host) && !coveredBy(host, decryptHosts) ? decryptHostOf(host) : null;

  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    const load = () => desktop.captureConnection(connection.id)
      .then((answer) => {
        if (cancelled) return;
        setCapture(answer.capture);
        setState("loaded");
      })
      .catch(() => !cancelled && setState((current) => (current === "loading" ? "unreachable" : current)));
    void load();
    const timer = open ? window.setInterval(() => void load(), 2000) : undefined;
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [connection.id, open, visible]);

  if (tool && "replay" in tool) {
    return <ReplayEditor exchange={tool.replay} busy={busy} run={run} onBack={() => setTool(null)} />;
  }
  if (tool) {
    return <MappingEditor initial={tool.mapping} busy={busy} run={run} onDone={() => setTool(null)} />;
  }
  if (exchange !== null && capture?.http[exchange]) {
    const item = capture.http[exchange];
    const map = (kind: DebugMapping["kind"]) => void mappingFromExchange(item, kind).then((mapping) => setTool({ mapping }));
    return (
      <div className="inspector-standalone">
        <ExchangeInspector
          exchange={item}
          siblings={capture.http}
          place={{ connectionId: connection.id, index: exchange, frames: capture.h2_frames?.seen ?? 0 }}
          onBack={() => setExchange(null)}
          backLabel={t("抓包内容")}
          exportCard={<ExchangeExport busy={busy} run={run} id={connection.id} index={exchange} redact={redact} onRedact={setRedact} />}
          actions={<>
            <button className="secondary compact" disabled={Boolean(busy)} onClick={() => setTool({ replay: item })}><RotateCw size={14} />{t("重放")}</button>
            <MoreMenu
              label={t("更多")}
              disabled={Boolean(busy)}
              items={[
                { label: t("拷贝网址"), icon: Copy, onSelect: () => void navigator.clipboard?.writeText(item.url) },
                { label: t("映射到本地"), onSelect: () => map("local") },
                { label: t("映射到远程"), onSelect: () => map("remote") },
              ]}
            />
          </>}
        />
      </div>
    );
  }

  const name = connectionName(connection);
  if (!capture) {
    const message = state === "unreachable"
      ? t("引擎已停止，抓包内容随之清空。")
      : state === "loading" ? t("正在读取…") : t("这个连接的抓包内容已被更新的连接挤出（抓包内存有上限），或抓包已清空。");
    return (
      <div className="page-stack">
        <BackBar label={t("连接")} onBack={onBack} />
        <Card title={name}><EmptyState icon={FileSearch} title={t("抓包内容")} message={message} /></Card>
      </div>
    );
  }

  return (
    <div className="page-stack">
      <BackBar label={t("连接")} onBack={onBack} />
      <Card title={name} description={t("每个方向保留前 64 KiB。")}>
        <Row icon={ArrowUpRight} color="orange" label={t("上传")} value={directionTotals(capture.upload)} />
        <Row icon={ArrowDownLeft} color="sky" label={t("下载")} value={directionTotals(capture.download)} />
        <Row icon={Network} color="gray" label={t("目标")} value={`${capture.network.toUpperCase()} ${capture.destination}`} />
        {capture.decrypted && <Row icon={Lock} color="green" label={t("内容")} value={t("HTTPS 已解密（MITM）")} />}
      </Card>

      {capture.http.length > 0 && (
        <Card title={t("HTTP 请求")}>
          {capture.http.map((item, index) => (
            <Row
              key={`${item.started_at_ms}-${index}`}
              label={<><span className="method">{item.method}</span>{item.url}</>}
              hint={exchangeHint(item)}
              onClick={() => setExchange(index)}
            />
          ))}
        </Card>
      )}

      {capture.chunks.length > 0 && (
        <Card
          title={capture.network === "udp" ? t("数据报") : t("数据")}
          description={looksLikeTls(capture) ? encryptedNote(connection.host, decryptHosts) : undefined}
          actions={looksLikeTls(capture) && undecrypted ? (
            <button
              className="secondary compact"
              disabled={Boolean(busy)}
              onClick={() => void run("decrypt-host", () => desktop.addDecryptHost(undecrypted)).then(reloadDecryptHosts)}
            >
              <Lock size={14} />{t("解密此域名")}
            </button>
          ) : undefined}
        >
          {capture.chunks.map((chunk, index) => <ChunkRow key={`${chunk.at_ms}-${index}`} chunk={chunk} />)}
        </Card>
      )}

      {capture.http.length === 0 && capture.chunks.length === 0 && (
        <Card><p className="muted">{t("还没有数据")}</p></Card>
      )}

      <Card title={t("导出")} description={t("pcapng 可用 Wireshark 打开，HAR 可导入浏览器开发者工具、Charles 或 Proxyman。导出内容可能包含账号、Cookie 等敏感信息，请只发给信任的人。")}>
        <Row icon={Download} color="indigo" label={t("导出 pcapng")} hint={t("保存到“下载”文件夹")}>
          <button className="secondary compact" disabled={Boolean(busy)} onClick={() => exportCapture(run, "pcapng", connection.id, { redact })}>{t("导出")}</button>
        </Row>
        {capture.http.length > 0 && (
          <Row icon={FileCode} color="purple" label={t("导出 HAR")} hint={t("保存到“下载”文件夹")}>
            <button className="secondary compact" disabled={Boolean(busy)} onClick={() => exportCapture(run, "har", connection.id, { redact })}>{t("导出")}</button>
          </Row>
        )}
        <RedactRow redact={redact} onRedact={setRedact} />
      </Card>
    </div>
  );
}

/** What to do about an encrypted connection: add its domain, or wait for a new connection once it is added. */
function encryptedNote(host: string | null | undefined, decrypted: string[]): string {
  if (!isDecryptableHost(host)) return t("这是加密的 TLS 数据。目标只有 IP 地址，没有域名，无法按域名解密。");
  if (coveredBy(host, decrypted)) {
    return t("这个域名已在解密列表里。解密只对新连接生效；如果仍看不到明文，请确认 HTTPS 解密已开启、根证书已信任，或者这个 App 固定了证书。");
  }
  return t("这是加密的 TLS 数据。点“解密此域名”后，这个域名的新连接会被解密（需要开启 HTTPS 解密并信任根证书）。");
}

function exchangeHint(exchange: CaptureExchange): string {
  const took = durationMs(exchange.started_at_ms, exchange.ended_at_ms);
  return [
    exchange.status != null ? String(exchange.status) : t("没有响应"),
    took != null ? `${took} ms` : null,
    exchange.response.body.size > 0 ? formatBytes(exchange.response.body.size) : null,
    header(exchange.response, "Content-Type"),
    noteText(exchange.note),
  ].filter(Boolean).join(" · ");
}

/** One read or write of the connection; opens in place to show its bytes. */
function ChunkRow({ chunk }: { chunk: CaptureChunk }) {
  const [open, setOpen] = useState(false);
  const bytes = useMemo(() => fromBase64(chunk.data_base64), [chunk.data_base64]);
  const upload = chunk.dir === "upload";
  return (
    <>
      <Row
        icon={upload ? ArrowUpRight : ArrowDownLeft}
        color={upload ? "orange" : "sky"}
        label={upload ? t("发出") : t("收到")}
        value={`${formatBytes(bytes.length)} · ${timeOfDay(chunk.at_ms)}`}
        onClick={() => setOpen((current) => !current)}
        className={open ? "expanded" : ""}
      />
      {open && <BytesView bytes={bytes} />}
    </>
  );
}

/** Text when the bytes read as text, else a hex dump; the user can switch, and copy what is shown. */
export function BytesView({ bytes, caption }: { bytes: Uint8Array; caption?: string }) {
  const text = useMemo(() => asText(bytes), [bytes]);
  const [style, setStyle] = useState<"text" | "hex">(text === null ? "hex" : "text");
  const shown = useMemo(() => {
    const full = style === "text" && text !== null ? text : hexDump(bytes);
    return full.length > 200_000 ? `${full.slice(0, 200_000)}\n…` : full;
  }, [bytes, style, text]);
  return (
    <div className="bytes-view">
      <div className="bytes-tools">
        {caption && <small className="muted">{caption}</small>}
        <span className="bytes-spacer" />
        <Segmented
          label={t("显示方式")}
          options={[{ value: "text", label: t("文本") }, { value: "hex", label: t("十六进制") }]}
          value={style}
          onChange={setStyle}
          disabled={text === null}
        />
        <CopyButton label={t("拷贝")} text={shown} />
      </div>
      <pre className="bytes">{shown || " "}</pre>
    </div>
  );
}

/**
 * The request table's inspector: the selected request and its response, with replay, mappings and export. It reads
 * the request's connection from the engine, again every two seconds until the response is complete. Replay and the
 * mapping editor open in its place, with the way back on top.
 */
export function RequestInspector({ request, busy, run, onBack, replayAsked, onReplayHandled, layoutItems = [] }: {
  /** Entries for the "more" menu that belong to the pane rather than the request, such as where it docks. */
  layoutItems?: MenuItem[];
  request: CaptureRequest | null;
  busy: string | null;
  run: Run;
  /** A narrow window shows the inspector in place of the table; this goes back to it. */
  onBack?: () => void;
  /** Ctrl+R in the table: opens replay for this request once it has loaded. */
  replayAsked?: boolean;
  onReplayHandled?: () => void;
}) {
  const [exchange, setExchange] = useState<CaptureExchange | null>(null);
  const [siblings, setSiblings] = useState<CaptureExchange[]>([]);
  const [state, setState] = useState<"loading" | "loaded" | "gone">("loading");
  const [redact, setRedact] = useState(true);
  const [tool, setTool] = useState<{ replay: CaptureExchange } | { mapping: DebugMapping } | null>(null);
  const visible = usePageVisible();
  const [frames, setFrames] = useState(0);
  const connectionId = request?.connection_id;
  const index = request?.index;
  // The row's place in the capture: the same while the request is under way and after it ends.
  const key = request ? requestKey(request) : null;
  const startedAt = request?.started_at_ms;

  useEffect(() => {
    setTool(null);
    setExchange(null);
    setSiblings([]);
    setFrames(0);
    setState("loading");
  }, [key]);

  useEffect(() => {
    if (connectionId == null || index == null || !visible) return;
    let cancelled = false;
    let timer: number | undefined;
    const load = () => desktop.captureConnection(connectionId)
      .then((answer) => {
        if (cancelled) return;
        const found = answer.capture?.http[index] ?? null;
        // A loaded session can reuse a connection id: only the same request counts.
        const item = found && found.started_at_ms === startedAt ? found : null;
        setExchange(item);
        setSiblings(answer.capture?.http ?? []);
        setFrames(answer.capture?.h2_frames?.seen ?? 0);
        setState(item ? "loaded" : "gone");
        // The response (or its event stream or WebSocket) is still going: read again until it ends.
        if (item && (item.state === "in_progress" || (item.state == null && item.ended_at_ms == null && item.status == null))) {
          timer = window.setTimeout(() => void load(), 2000);
        }
      })
      .catch(() => !cancelled && setState("gone"));
    void load();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [connectionId, index, startedAt, visible]);

  useEffect(() => {
    if (replayAsked && exchange) {
      setTool({ replay: exchange });
      onReplayHandled?.();
    }
  }, [replayAsked, exchange, onReplayHandled]);

  if (!request) {
    return (
      <div className="inspector-empty">
        <EmptyState icon={MousePointerClick} title={t("选择一条请求")} message={t("在列表里选一条，这里显示它的请求和响应。方向键或 j / k 切换，Ctrl+R 重放。")} />
      </div>
    );
  }
  if (tool && "replay" in tool) {
    return <div className="inspector-scroll padded"><ReplayEditor exchange={tool.replay} busy={busy} run={run} onBack={() => setTool(null)} /></div>;
  }
  if (tool) {
    return <div className="inspector-scroll padded"><MappingEditor initial={tool.mapping} busy={busy} run={run} onDone={() => setTool(null)} /></div>;
  }
  const map = (kind: DebugMapping["kind"]) => exchange && void mappingFromExchange(exchange, kind).then((mapping) => setTool({ mapping }));
  const more: MenuItem[] = [
    { label: t("拷贝网址"), icon: Copy, onSelect: () => void navigator.clipboard?.writeText(request.url) },
    { label: t("映射到本地"), disabled: !exchange, onSelect: () => map("local") },
    { label: t("映射到远程"), disabled: !exchange, onSelect: () => map("remote") },
    ...layoutItems,
  ];
  return (
    <ExchangeInspector
      request={request}
      exchange={exchange}
      siblings={siblings}
      place={{ connectionId: request.connection_id, index: request.index, frames }}
      waiting={state === "gone" ? t("这个请求的内容已被更新的连接挤出（抓包内存有上限），或抓包已清空。") : undefined}
      onBack={onBack}
      backLabel={t("请求")}
      exportCard={exchange && <ExchangeExport busy={busy} run={run} id={request.connection_id} index={request.index} redact={redact} onRedact={setRedact} />}
      actions={<>
        <button className="secondary compact" disabled={Boolean(busy) || !exchange} onClick={() => exchange && setTool({ replay: exchange })} title="Ctrl+R"><RotateCw size={14} />{t("重放")}</button>
        <MoreMenu label={t("更多")} disabled={Boolean(busy)} items={more} />
      </>}
    />
  );
}

/** Masking applies to every export; on unless the user turns it off for this view. */
function RedactRow({ redact, onRedact }: { redact: boolean; onRedact: (redact: boolean) => void }) {
  return (
    <ToggleRow
      icon={Lock}
      color="green"
      label={t("遮盖 Cookie、令牌和密码")}
      hint={t("导出的文件里把这些值换成 <redacted>")}
      checked={redact}
      onChange={() => onRedact(!redact)}
    />
  );
}

type ExchangeFormat = "curl" | CodeLanguage | "postman" | "insomnia" | "har";

const exchangeFormats: Array<{ value: ExchangeFormat; label: string }> = [
  { value: "curl", label: "cURL" },
  { value: "python", label: "Python (requests)" },
  { value: "javascript", label: "JavaScript (fetch)" },
  { value: "swift", label: "Swift (URLSession)" },
  { value: "kotlin", label: "Kotlin (OkHttp)" },
  { value: "go", label: "Go (net/http)" },
  { value: "postman", label: "Postman" },
  { value: "insomnia", label: "Insomnia" },
  { value: "har", label: "HAR" },
];

/** Exports this one request as a command, code, or a collection for an API tool. */
function ExchangeExport({ busy, run, id, index, redact, onRedact }: {
  busy: string | null;
  run: Run;
  id: number;
  index: number;
  redact: boolean;
  onRedact: (redact: boolean) => void;
}) {
  const [format, setFormat] = useState<ExchangeFormat>("curl");
  const save = () => {
    if (format === "curl" || format === "postman" || format === "insomnia" || format === "har") {
      exportCapture(run, format, id, { exchange: index, redact });
    } else {
      exportCapture(run, "code", id, { exchange: index, redact, language: format });
    }
  };
  return (
    <Card title={t("导出这条请求")} description={t("保存到“下载”文件夹。导出内容可能包含账号等敏感信息，请只发给信任的人。")}>
      <Row icon={FileCode} color="purple" label={t("格式")}>
        <Select label={t("导出格式")} value={format} onChange={setFormat} choices={exchangeFormats.map((item) => ({ value: item.value, label: item.label }))} />
        <button className="secondary compact" disabled={Boolean(busy)} onClick={save}>{t("导出")}</button>
      </Row>
      <RedactRow redact={redact} onRedact={onRedact} />
    </Card>
  );
}
