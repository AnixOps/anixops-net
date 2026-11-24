import { FileInput, Plus, Shuffle, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { CaptureExchange, DebugMapping, desktop, ReplayResponse } from "../bridge";
import { asText, decodedBody, fromBase64, header, headerLines, parseHeaderLines, toBase64, withoutQuery } from "../capture-format";
import { formatBytes } from "../subscription-format";
import { Card, EmptyState, Field, FoldedRows, MoreMenu, Row, Run, Segmented, Switch } from "../ui";
import { t } from "../i18n";
import { BackBar, BytesView } from "./CaptureViews";

/** A body in a text box: editable when it is text, kept as captured bytes otherwise. */
type BodyDraft = { text: string } | { bytes: Uint8Array };

function bodyDraft(bytes: Uint8Array): BodyDraft {
  const text = asText(bytes);
  return text === null ? { bytes } : { text };
}

function draftBase64(draft: BodyDraft): string {
  return "text" in draft ? toBase64(new TextEncoder().encode(draft.text)) : toBase64(draft.bytes);
}

function BodyField({ label, draft, onChange }: { label: string; draft: BodyDraft; onChange: (draft: BodyDraft) => void }) {
  if ("bytes" in draft) {
    return (
      <Row label={label} value={t("二进制内容 {size}，按原样发送", { size: formatBytes(draft.bytes.length) })}>
        <button className="secondary compact" onClick={() => onChange({ text: "" })}>{t("清空")}</button>
      </Row>
    );
  }
  return (
    <Field label={label}>
      <textarea value={draft.text} spellCheck={false} onChange={(event) => onChange({ text: event.target.value })} />
    </Field>
  );
}

/**
 * Sends an edited copy of a captured request again through the local proxy, so it leaves through the current exit
 * and shows up in capture. Redirects are not followed and no cookies are kept.
 */
export function ReplayEditor({ exchange, busy, run, onBack }: { exchange: CaptureExchange; busy: string | null; run: Run; onBack: () => void }) {
  const [method, setMethod] = useState(exchange.method);
  const [url, setUrl] = useState(exchange.url);
  const [headers, setHeaders] = useState(() => headerLines(exchange.request.headers.filter(([name]) => !name.startsWith(":"))));
  const [body, setBody] = useState<BodyDraft>(() => bodyDraft(fromBase64(exchange.request.body.data_base64)));
  const [answer, setAnswer] = useState<ReplayResponse | null>(null);
  const cut = exchange.request.body.truncated;

  const send = () => void run("replay", async () => {
    setAnswer(null);
    const response = await desktop.replayRequest({
      method: method.trim() || "GET",
      url: url.trim(),
      headers: parseHeaderLines(headers),
      bodyBase64: draftBase64(body),
    });
    setAnswer(response);
    return { message: t("已收到响应 {status}，用时 {took} ms", { status: response.status, took: response.elapsedMs }) };
  });

  return (
    <div className="page-stack">
      <BackBar label={t("请求")} onBack={onBack} />
      <Card
        title={t("重放请求")}
        description={t("经本机代理发出，走当前出口，也会出现在抓包里。不跟随重定向，不保存 Cookie。")}
      >
        <div className="replay-line">
          <input aria-label={t("方法")} className="method-input" value={method} onChange={(event) => setMethod(event.target.value.toUpperCase())} />
          <input aria-label={t("网址")} value={url} spellCheck={false} onChange={(event) => setUrl(event.target.value)} />
        </div>
        <Field label={t("请求头（每行一个“名称: 值”）")}>
          <textarea value={headers} spellCheck={false} onChange={(event) => setHeaders(event.target.value)} />
        </Field>
        <BodyField label={cut ? t("请求体（抓包只保留了前一部分）") : t("请求体")} draft={body} onChange={setBody} />
        <div className="button-row">
          <button className="primary" disabled={Boolean(busy) || !url.trim()} onClick={send}>{t("发送")}</button>
        </div>
      </Card>
      {answer && <ReplayAnswer answer={answer} />}
    </div>
  );
}

function ReplayAnswer({ answer }: { answer: ReplayResponse }) {
  const raw = useMemo(() => fromBase64(answer.bodyBase64), [answer.bodyBase64]);
  const message = { headers: answer.headers, body: { size: raw.length, truncated: answer.truncated, data_base64: answer.bodyBase64 } };
  const encoding = header(message, "Content-Encoding");
  const [body, setBody] = useState<{ bytes: Uint8Array; decoded: boolean }>({ bytes: raw, decoded: false });
  useEffect(() => {
    let cancelled = false;
    setBody({ bytes: raw, decoded: false });
    void decodedBody(raw, encoding, answer.truncated).then((next) => !cancelled && setBody(next));
    return () => {
      cancelled = true;
    };
  }, [raw, encoding, answer.truncated]);
  const caption = [
    header(message, "Content-Type"),
    encoding ? (body.decoded ? t("{encoding} 已解压", { encoding }) : encoding) : null,
    answer.truncated ? t("只显示前 {kept}", { kept: formatBytes(raw.length) }) : formatBytes(raw.length),
  ].filter(Boolean).join(" · ");
  return (
    <>
      <Card title={t("响应 {status}", { status: answer.status })}>
        <Row label={t("耗时")} value={`${answer.elapsedMs} ms`} />
        {answer.headers.length > 0 && (
          <dl className="headers selectable">{answer.headers.map(([name, value], index) => (
            <div key={`${name}-${index}`}><dt>{name}</dt><dd>{value}</dd></div>
          ))}</dl>
        )}
      </Card>
      {raw.length > 0 && <Card title={t("响应体")}><BytesView key={body.decoded ? "decoded" : "raw"} bytes={body.bytes} caption={caption} /></Card>}
    </>
  );
}

const emptyMapping: DebugMapping = {
  id: "",
  name: "",
  enabled: true,
  kind: "local",
  match: "url",
  pattern: "",
  status: 200,
  headers: [{ name: "Content-Type", value: "application/json; charset=utf-8" }],
  body: "",
  target: "",
};

/** A new mapping for a captured request: its URL without the query, and for Map Local the response it got. */
export async function mappingFromExchange(exchange: CaptureExchange, kind: DebugMapping["kind"]): Promise<DebugMapping> {
  const pattern = withoutQuery(exchange.url);
  if (kind === "remote") return { ...emptyMapping, kind, pattern, headers: [], target: pattern };
  const raw = fromBase64(exchange.response.body.data_base64);
  const encoding = header(exchange.response, "Content-Encoding");
  const body = await decodedBody(raw, encoding, exchange.response.body.truncated);
  const type = header(exchange.response, "Content-Type");
  return {
    ...emptyMapping,
    kind,
    pattern,
    status: exchange.status ?? 200,
    headers: type ? [{ name: "Content-Type", value: type }] : [],
    body: toBase64(body.bytes),
  };
}

/** Adds or changes one Map Local / Map Remote mapping. */
export function MappingEditor({ initial, busy, run, onDone }: { initial: DebugMapping; busy: string | null; run: Run; onDone: () => void }) {
  const [entry, setEntry] = useState<DebugMapping>(initial);
  const [headers, setHeaders] = useState(() => headerLines(initial.headers.map(({ name, value }) => [name, value])));
  const [body, setBody] = useState<BodyDraft>(() => bodyDraft(fromBase64(initial.body)));
  const change = (patch: Partial<DebugMapping>) => setEntry((current) => ({ ...current, ...patch }));
  const local = entry.kind === "local";

  const save = () => void run("mapping", async () => {
    const result = await desktop.saveDebugMapping({
      ...entry,
      headers: local ? parseHeaderLines(headers).map(([name, value]) => ({ name, value })) : [],
      body: local ? draftBase64(body) : "",
      target: local ? "" : entry.target.trim(),
      pattern: entry.pattern.trim(),
    });
    onDone();
    return result;
  });

  return (
    <div className="page-stack">
      <BackBar label={t("返回")} onBack={onDone} />
      <Card
        title={initial.id ? t("编辑映射") : t("新建映射")}
        description={t("保存后该域名加入 HTTPS 解密，需先开启 HTTPS 解密。")}
      >
        <Row label={t("方式")}>
          <Segmented
            label={t("方式")}
            options={[{ value: "local", label: t("本地应答") }, { value: "remote", label: t("转到其他网址") }]}
            value={entry.kind}
            onChange={(kind) => change({ kind })}
          />
        </Row>
        <Row label={t("匹配")}>
          <Segmented
            label={t("匹配")}
            options={[
              { value: "url", label: t("网址"), hint: t("这个网址，带任意查询参数") },
              { value: "prefix", label: t("前缀"), hint: t("以它开头的所有网址") },
              { value: "regex", label: t("正则"), hint: t("对整个网址的正则表达式") },
            ]}
            value={entry.match}
            onChange={(match) => change({ match })}
          />
        </Row>
        <Field label={entry.match === "regex" ? t("正则表达式") : t("要匹配的网址")}>
          <input value={entry.pattern} spellCheck={false} placeholder="https://" onChange={(event) => change({ pattern: event.target.value })} />
        </Field>
        <Field label={t("名称（可选）")}>
          <input value={entry.name} onChange={(event) => change({ name: event.target.value })} />
        </Field>
        {local ? (
          <>
            <Field label={t("状态码")}>
              <input type="number" min={100} max={599} value={entry.status} onChange={(event) => change({ status: Number(event.target.value) })} />
            </Field>
            <Field label={t("响应头（每行一个“名称: 值”）")}>
              <textarea value={headers} spellCheck={false} onChange={(event) => setHeaders(event.target.value)} />
            </Field>
            <BodyField label={t("响应体（最多 256 KiB）")} draft={body} onChange={setBody} />
          </>
        ) : (
          <Field label={entry.match === "regex" ? t("新网址（可用 $1 引用分组）") : t("新网址（查询参数或其余路径会接在后面）")}>
            <input value={entry.target} spellCheck={false} placeholder="https://" onChange={(event) => change({ target: event.target.value })} />
          </Field>
        )}
        <div className="button-row">
          <button className="primary" disabled={Boolean(busy) || !entry.pattern.trim()} onClick={save}>{t("保存")}</button>
          <button className="secondary" onClick={onDone}>{t("取消")}</button>
        </div>
      </Card>
    </div>
  );
}

function mappingLabel(entry: DebugMapping): string {
  return entry.name.trim() || entry.pattern;
}

function mappingHint(entry: DebugMapping): string {
  return entry.kind === "local"
    ? t("本地应答 {status} · {size}", { status: entry.status, size: formatBytes(fromBase64(entry.body).length) })
    : t("转到 {target}", { target: entry.target });
}

/** The saved mappings on the Extensions page; an open editor replaces the page content. */
export function MappingsCard({ busy, run, onEdit }: { busy: string | null; run: Run; onEdit: (entry: DebugMapping) => void }) {
  const [entries, setEntries] = useState<DebugMapping[]>([]);
  const load = useCallback(() => {
    desktop.debugMappings().then(setEntries).catch(() => setEntries([]));
  }, []);
  useEffect(load, [load]);
  const change = (task: () => Promise<{ message: string }>) => void run("mapping", task).then(load);

  return (
    <Card
      title={t("映射")}
      description={t("Map Local 以本地内容应答请求，Map Remote 将请求转到其他网址。需开启 HTTPS 解密。")}
      actions={<button className="secondary compact" disabled={Boolean(busy)} onClick={() => onEdit(emptyMapping)}><Plus size={14} />{t("新建映射")}</button>}
    >
      {entries.length === 0 ? (
        <EmptyState icon={Shuffle} title={t("暂无映射")} message={t("可从请求详情创建，或在此新建。")} />
      ) : (
        <FoldedRows
          items={entries}
          label={(count) => t("显示全部 {count} 条映射", { count })}
          render={(entry) => (
            <Row
              key={entry.id}
              icon={entry.kind === "local" ? FileInput : Shuffle}
              color={entry.enabled ? "teal" : "gray"}
              label={<span className="mapping-pattern" title={entry.pattern}>{mappingLabel(entry)}</span>}
              hint={mappingHint(entry)}
              className="plugin-row"
            >
              <span className="row-actions">
                <MoreMenu
                  label={t("更多")}
                  disabled={Boolean(busy)}
                  items={[
                    { label: t("编辑"), onSelect: () => onEdit(entry) },
                    {
                      label: t("删除映射"),
                      icon: Trash2,
                      danger: true,
                      confirm: t("确认删除 {name}", { name: mappingLabel(entry) }),
                      onSelect: () => change(() => desktop.removeDebugMapping(entry.id)),
                    },
                  ]}
                />
              </span>
              <Switch
                label={mappingLabel(entry)}
                checked={entry.enabled}
                disabled={Boolean(busy)}
                onChange={() => change(() => desktop.saveDebugMapping({ ...entry, enabled: !entry.enabled }))}
              />
            </Row>
          )}
        />
      )}
    </Card>
  );
}
