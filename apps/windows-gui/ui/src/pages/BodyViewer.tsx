import { ChevronDown, ChevronRight } from "lucide-react";
import { createContext, KeyboardEvent, ReactNode, useContext, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { CaptureMessage } from "../bridge";
import { BodyKind, BodyMode, bodyModes, bodyShape, chosenMode, formPairs, imageType } from "../body-view";
import { asText, bodySize, decodedBody, fromBase64, header, hexDump, toBase64 } from "../capture-format";
import { childCount, containerIds, copyText, initiallyCollapsed, JsonNode, JsonRow, jsonRows, prettyJson, prettyTokens } from "../json-view";
import { CopyButton, MoreMenu, Segmented } from "../ui";
import { t } from "../i18n";

/** The view the user picked last for each kind of body, kept while the window is open. */
const lastMode = new Map<BodyKind, BodyMode>();

function modeLabel(mode: BodyMode): string {
  switch (mode) {
    case "tree": return t("树");
    case "pretty": return t("格式化");
    case "preview": return t("预览");
    case "table": return t("表格");
    case "raw": return t("原文");
    case "hex": return t("十六进制");
  }
}

/** Longer text than this is cut on screen; copying still takes all of it. */
const SHOWN_CHARS = 400_000;

/**
 * A captured body, shown by what it is: JSON as a folding tree or indented and coloured, an image as a picture, a
 * form as name/value pairs, other text as text, anything else as a hex dump. The user can switch views, and the
 * choice is kept per kind. gzip and deflate are undone first. `lead` goes at the left of the tool line (the
 * inspector puts its headers/body switch there); the viewer fills the height it is given.
 */
export function BodyViewer({ message, lead }: { message: CaptureMessage; lead?: ReactNode }) {
  const raw = useMemo(() => fromBase64(message.body.data_base64), [message.body.data_base64]);
  const encoding = header(message, "Content-Encoding");
  const contentType = header(message, "Content-Type");
  const [body, setBody] = useState<{ bytes: Uint8Array; decoded: boolean }>({ bytes: raw, decoded: false });
  useEffect(() => {
    let cancelled = false;
    setBody({ bytes: raw, decoded: false });
    void decodedBody(raw, encoding, message.body.truncated).then((next) => !cancelled && setBody(next));
    return () => {
      cancelled = true;
    };
  }, [raw, encoding, message.body.truncated]);

  const text = useMemo(() => asText(body.bytes), [body.bytes]);
  const shape = useMemo(() => bodyShape(contentType, text), [contentType, text]);
  const [picked, setPicked] = useState<BodyMode | undefined>(() => lastMode.get(shape.kind));
  // A picture that does not draw (a cut or damaged file) falls back to the hex dump.
  const [broken, setBroken] = useState(false);
  useEffect(() => {
    setPicked(lastMode.get(shape.kind));
    setBroken(false);
  }, [shape.kind, body.bytes]);
  const mode = broken && shape.kind === "image" ? "hex" : chosenMode(shape.kind, picked);
  const modes = bodyModes(shape.kind);
  const [fold, setFold] = useState<{ all: boolean; stamp: number } | null>(null);
  // A new document starts a new tree: fold state and selection belong to the document they were made on.
  const tree = useRef<{ json: JsonNode | undefined; serial: number }>({ json: shape.json, serial: 0 });
  if (tree.current.json !== shape.json) tree.current = { json: shape.json, serial: tree.current.serial + 1 };

  const caption = [
    contentType,
    encoding ? (body.decoded ? t("{encoding} 已解压", { encoding }) : encoding) : null,
    bodySize(message.body),
  ].filter(Boolean).join(" · ");

  const copied = () => {
    if (mode === "hex") return hexDump(body.bytes);
    if ((mode === "tree" || mode === "pretty") && shape.json) return prettyJson(shape.json);
    return text ?? hexDump(body.bytes);
  };

  let content: ReactNode;
  if (mode === "tree" && shape.json) {
    content = <JsonTree key={[tree.current.serial, fold?.stamp ?? 0].join("-")} root={shape.json} foldAll={fold?.all} />;
  } else if (mode === "pretty" && shape.json) {
    content = <PrettyJson root={shape.json} />;
  } else if (mode === "preview") {
    const type = imageType(contentType) ?? "image/png";
    const source = `data:${type};base64,${toBase64(body.bytes)}`;
    content = (
      <div className="body-image">
        <img src={source} alt={t("图片预览")} onError={() => setBroken(true)} />
        {message.body.truncated && <p className="muted">{t("只保留了图片的开头，画面可能不完整。")}</p>}
      </div>
    );
  } else if (mode === "table" && text !== null) {
    content = <PairTable pairs={formPairs(text)} />;
  } else if (mode === "raw" && text !== null) {
    content = <pre className="body-text selectable">{text.length > SHOWN_CHARS ? `${text.slice(0, SHOWN_CHARS)}\n…` : text || " "}</pre>;
  } else {
    content = <pre className="body-text selectable">{hexDump(body.bytes)}</pre>;
  }

  return (
    <div className="body-viewer">
      <div className="inspector-tools">
        {lead}
        <span className="bytes-spacer" />
        {modes.length > 1 && (
          <Segmented
            label={t("显示方式")}
            className="compact"
            options={modes.map((value) => ({ value, label: modeLabel(value) }))}
            value={mode}
            onChange={(next) => {
              lastMode.set(shape.kind, next);
              setPicked(next);
              setBroken(false);
            }}
          />
        )}
        <CopyButton label={t("拷贝正文")} text={copied} />
        {mode === "tree" && (
          <MoreMenu
            label={t("更多")}
            items={[
              { label: t("全部展开"), onSelect: () => setFold({ all: false, stamp: Date.now() }) },
              { label: t("全部折叠"), onSelect: () => setFold({ all: true, stamp: Date.now() }) },
            ]}
          />
        )}
      </div>
      <BodyFrame caption={caption}>{content}</BodyFrame>
    </div>
  );
}

/** Where the tree shows the selected value's path: the left of the caption line under the content. */
const StatusSlot = createContext<HTMLElement | null>(null);

/** The content with the caption line under it. */
function BodyFrame({ caption, children }: { caption: string; children: ReactNode }) {
  const [slot, setSlot] = useState<HTMLSpanElement | null>(null);
  return (
    <StatusSlot.Provider value={slot}>
      <div className="body-content">{children}</div>
      <div className="body-status">
        <span className="body-status-lead" ref={setSlot} />
        <span className="body-caption" title={caption}>{caption}</span>
      </div>
    </StatusSlot.Provider>
  );
}

/** Name/value pairs (headers, query parameters, form fields) in the inspector's dense two-column list. */
export function PairTable({ pairs }: { pairs: Array<[string, string]> }) {
  if (pairs.length === 0) return <p className="muted inspector-note">{t("无")}</p>;
  return (
    <dl className="headers dense selectable">
      {pairs.map(([name, value], index) => <div key={`${name}-${index}`}><dt>{name}</dt><dd>{value}</dd></div>)}
    </dl>
  );
}

function PrettyJson({ root }: { root: JsonNode }) {
  const tokens = useMemo(() => prettyTokens(root), [root]);
  return (
    <pre className="body-text code selectable">
      {tokens.map((token, index) => (token.kind === "space" || token.kind === "punctuation"
        ? token.text
        : <span key={index} className={`tok-${token.kind}`}>{token.text}</span>))}
    </pre>
  );
}

const TREE_ROW = 22;
const TREE_OVERSCAN = 12;

function valueText(node: JsonNode): string {
  switch (node.kind) {
    case "string": return JSON.stringify(node.value);
    case "number": return node.raw;
    case "boolean": return String(node.value);
    case "null": return "null";
    case "object": return `{${childCount(node)}}`;
    case "array": return `[${childCount(node)}]`;
  }
}

function valueClass(node: JsonNode): string {
  switch (node.kind) {
    case "string": return "tok-string";
    case "number": return "tok-number";
    case "boolean":
    case "null": return "tok-literal";
    default: return "tok-count";
  }
}

/**
 * The folding tree. Only the rows in view are in the document, so a long array stays quick. Keys: up and down move,
 * right opens or steps in, left closes or steps out, Enter or Space folds, Ctrl+C copies the selected value. The
 * selected value's JSONPath shows under the tree; clicking it copies the path.
 */
function JsonTree({ root, foldAll }: { root: JsonNode; foldAll?: boolean }) {
  const [collapsed, setCollapsed] = useState<Set<string>>(() => (
    foldAll === undefined ? initiallyCollapsed(root) : new Set(foldAll ? containerIds(root, 1) : [])
  ));
  const [selected, setSelected] = useState<string | null>(null);
  const [view, setView] = useState({ top: 0, height: 480 });
  const scroller = useRef<HTMLDivElement>(null);
  const slot = useContext(StatusSlot);
  const rows = useMemo(() => jsonRows(root, collapsed), [root, collapsed]);
  const current = rows.find((row) => row.id === selected) ?? null;

  useEffect(() => {
    const element = scroller.current;
    if (!element) return;
    const measure = () => setView((previous) => ({ ...previous, height: element.clientHeight }));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const toggle = (row: JsonRow) => {
    if (row.open === undefined) return;
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (row.open) next.add(row.id);
      else next.delete(row.id);
      return next;
    });
  };

  const reveal = (index: number) => {
    const element = scroller.current;
    if (!element) return;
    const top = index * TREE_ROW;
    if (top < element.scrollTop) element.scrollTop = top;
    else if (top + TREE_ROW > element.scrollTop + element.clientHeight) element.scrollTop = top + TREE_ROW - element.clientHeight;
  };

  const select = (index: number) => {
    const row = rows[Math.max(0, Math.min(rows.length - 1, index))];
    if (!row) return;
    setSelected(row.id);
    reveal(rows.indexOf(row));
  };

  const onKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const at = current ? rows.indexOf(current) : -1;
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "c" && current && !window.getSelection()?.toString()) {
      event.preventDefault();
      void navigator.clipboard?.writeText(copyText(current.node));
      return;
    }
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    const page = Math.max(1, Math.floor(view.height / TREE_ROW) - 1);
    switch (event.key) {
      case "ArrowDown": select(at + 1); break;
      case "ArrowUp": select(at < 0 ? 0 : at - 1); break;
      case "PageDown": select(at + page); break;
      case "PageUp": select(Math.max(0, at - page)); break;
      case "Home": select(0); break;
      case "End": select(rows.length - 1); break;
      case "ArrowRight":
        if (!current) select(0);
        else if (current.open === false) toggle(current);
        else if (current.open) select(at + 1);
        break;
      case "ArrowLeft":
        if (!current) select(0);
        else if (current.open) toggle(current);
        else if (current.parent) select(rows.findIndex((row) => row.id === current.parent));
        break;
      case "Enter":
      case " ":
        if (current) toggle(current);
        break;
      default:
        return;
    }
    event.preventDefault();
  };

  const first = Math.max(0, Math.floor(view.top / TREE_ROW) - TREE_OVERSCAN);
  const last = Math.min(rows.length, Math.ceil((view.top + view.height) / TREE_ROW) + TREE_OVERSCAN);

  return (
    <>
      <div
        className="json-tree"
        role="tree"
        aria-label={t("JSON 树")}
        aria-activedescendant={current ? `json-${current.id}` : undefined}
        tabIndex={0}
        ref={scroller}
        onKeyDown={onKey}
        onScroll={(event) => setView({ top: event.currentTarget.scrollTop, height: event.currentTarget.clientHeight })}
      >
        <div style={{ height: rows.length * TREE_ROW, position: "relative" }}>
          {rows.slice(first, last).map((row, offset) => (
            <div
              key={row.id}
              id={`json-${row.id}`}
              role="treeitem"
              aria-level={row.depth + 1}
              aria-expanded={row.open}
              aria-selected={row.id === selected}
              className={row.id === selected ? "json-row selected" : "json-row"}
              style={{ top: (first + offset) * TREE_ROW, height: TREE_ROW, paddingLeft: 8 + row.depth * 14 }}
              onMouseDown={() => setSelected(row.id)}
              onDoubleClick={() => toggle(row)}
            >
              {row.open === undefined
                ? <span className="json-fold" aria-hidden="true" />
                : (
                  <span className="json-fold" aria-hidden="true" onMouseDown={(event) => { event.stopPropagation(); setSelected(row.id); toggle(row); }}>
                    {row.open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                  </span>
                )}
              {row.name !== undefined && <span className="tok-key">{row.name}</span>}
              {row.index !== undefined && <span className="tok-index">{row.index}</span>}
              {(row.name !== undefined || row.index !== undefined) && <span className="json-colon" aria-hidden="true">:</span>}
              <span className={`json-value ${valueClass(row.node)}`}>{valueText(row.node)}</span>
            </div>
          ))}
        </div>
      </div>
      {slot && current && createPortal(
        <>
          <button className="json-path" title={t("拷贝路径")} onClick={() => void navigator.clipboard?.writeText(current.path)}>{current.path}</button>
          <CopyButton label={t("拷贝这个值")} text={() => copyText(current.node)} />
        </>,
        slot,
      )}
    </>
  );
}
