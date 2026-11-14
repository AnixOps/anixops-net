import { ArrowDownLeft, ArrowUpRight, ChevronLeft, Copy, Download, FileCode, FileSearch, Lock, Network, RotateCw } from "lucide-react";
import { ReactNode, useEffect, useMemo, useState } from "react";
import { CaptureChunk, CaptureConnection, CaptureExchange, CaptureFormat, CaptureMessage, ConnectionSnapshot, DebugMapping, desktop } from "../bridge";
import { connectionName } from "../activity-format";
import { asText, bodySize, decodedBody, directionTotals, durationMs, fromBase64, header, hexDump, looksLikeTls, noteText, timeOfDay } from "../capture-format";
import { usePageVisible } from "../live";
import { formatBytes } from "../subscription-format";
import { Card, EmptyState, IconButton, MoreMenu, Row, Run, Segmented } from "../ui";
import { t } from "../i18n";
import { mappingFromExchange, MappingEditor, ReplayEditor } from "./DebugTools";

/** Saves an export to Downloads and shows where it went. */
export function exportCapture(run: Run, format: CaptureFormat, id?: number) {
  void run(`export-${format}`, async () => {
    const saved = await desktop.exportCapture(format, id);
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
export function CaptureDetail({ connection, busy, run, onBack, replayable }: {
  connection: ConnectionSnapshot;
  busy: string | null;
  run: Run;
  onBack: () => void;
  /** Whether replay can go through a local proxy (not on Android). */
  replayable: boolean;
}) {
  const [capture, setCapture] = useState<CaptureConnection | null>(null);
  const [state, setState] = useState<"loading" | "loaded" | "unreachable">("loading");
  const [exchange, setExchange] = useState<number | null>(null);
  const [tool, setTool] = useState<{ replay: CaptureExchange } | { mapping: DebugMapping } | null>(null);
  const visible = usePageVisible();
  const open = connection.ended_at_ms == null;

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
      <ExchangeDetail
        exchange={item}
        onBack={() => setExchange(null)}
        actions={<>
          {replayable && <button className="secondary compact" disabled={Boolean(busy)} onClick={() => setTool({ replay: item })}><RotateCw size={14} />{t("重放")}</button>}
          <MoreMenu
            label={t("更多")}
            disabled={Boolean(busy)}
            items={[
              { label: t("映射到本地"), onSelect: () => map("local") },
              { label: t("映射到远程"), onSelect: () => map("remote") },
            ]}
          />
        </>}
      />
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
          description={looksLikeTls(capture) ? t("这是加密的 TLS 数据。要看到 HTTPS 明文，请在扩展里为这个域名启用 HTTPS 解密（MITM），并信任证书。") : undefined}
        >
          {capture.chunks.map((chunk, index) => <ChunkRow key={`${chunk.at_ms}-${index}`} chunk={chunk} />)}
        </Card>
      )}

      {capture.http.length === 0 && capture.chunks.length === 0 && (
        <Card><p className="muted">{t("还没有数据")}</p></Card>
      )}

      <Card title={t("导出")} description={t("pcapng 可用 Wireshark 打开，HAR 可导入浏览器开发者工具、Charles 或 Proxyman。导出内容可能包含账号、Cookie 等敏感信息，请只发给信任的人。")}>
        <Row icon={Download} color="indigo" label={t("导出 pcapng")} hint={t("保存到“下载”文件夹")}>
          <button className="secondary compact" disabled={Boolean(busy)} onClick={() => exportCapture(run, "pcapng", connection.id)}>{t("导出")}</button>
        </Row>
        {capture.http.length > 0 && (
          <Row icon={FileCode} color="purple" label={t("导出 HAR")} hint={t("保存到“下载”文件夹")}>
            <button className="secondary compact" disabled={Boolean(busy)} onClick={() => exportCapture(run, "har", connection.id)}>{t("导出")}</button>
          </Row>
        )}
      </Card>
    </div>
  );
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
        <IconButton label={t("拷贝")} onClick={() => void navigator.clipboard?.writeText(shown)}><Copy size={14} /></IconButton>
      </div>
      <pre className="bytes">{shown || " "}</pre>
    </div>
  );
}

/** One decrypted request and its response: headers, and bodies decompressed when they say gzip or deflate. */
function ExchangeDetail({ exchange, onBack, actions }: { exchange: CaptureExchange; onBack: () => void; actions?: ReactNode }) {
  const took = durationMs(exchange.started_at_ms, exchange.ended_at_ms);
  const note = noteText(exchange.note);
  return (
    <div className="page-stack">
      <BackBar label={t("抓包内容")} onBack={onBack} />
      <Card title={`${exchange.method} ${exchange.status ?? ""}`.trim()} actions={actions}>
        <div className="selectable url-line">{exchange.url}</div>
        <Row label={t("状态")} value={exchange.status != null ? String(exchange.status) : t("没有响应")} />
        <Row label={t("开始")} value={timeOfDay(exchange.started_at_ms)} />
        {took != null && <Row label={t("耗时")} value={`${took} ms`} />}
        {note && <Row label={t("说明")} value={note} />}
      </Card>
      <MessageView title={t("请求")} message={exchange.request} />
      {exchange.status != null && <MessageView title={t("响应")} message={exchange.response} />}
    </div>
  );
}

function MessageView({ title, message }: { title: string; message: CaptureMessage }) {
  return (
    <>
      <Card title={t("{title}头", { title })}>
        {message.headers.length === 0
          ? <p className="muted">{t("无")}</p>
          : <dl className="headers selectable">{message.headers.map(([name, value], index) => (
            <div key={`${name}-${index}`}><dt>{name}</dt><dd>{value}</dd></div>
          ))}</dl>}
      </Card>
      {message.body.size > 0 && <BodyCard title={t("{title}体", { title })} message={message} />}
    </>
  );
}

function BodyCard({ title, message }: { title: string; message: CaptureMessage }) {
  const raw = useMemo(() => fromBase64(message.body.data_base64), [message.body.data_base64]);
  const encoding = header(message, "Content-Encoding");
  const [body, setBody] = useState<{ bytes: Uint8Array; decoded: boolean }>({ bytes: raw, decoded: false });
  useEffect(() => {
    let cancelled = false;
    setBody({ bytes: raw, decoded: false });
    void decodedBody(raw, encoding, message.body.truncated).then((next) => !cancelled && setBody(next));
    return () => {
      cancelled = true;
    };
  }, [raw, encoding, message.body.truncated]);
  const caption = [
    header(message, "Content-Type"),
    encoding ? (body.decoded ? t("{encoding} 已解压", { encoding }) : encoding) : null,
    bodySize(message.body),
  ].filter(Boolean).join(" · ");
  return (
    <Card title={title}>
      <BytesView key={body.decoded ? "decoded" : "raw"} bytes={body.bytes} caption={caption} />
    </Card>
  );
}
