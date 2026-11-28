import { useState } from "react";
import { Heading } from "react-aria-components";
import { desktop } from "../bridge";
import type { NodeCheckupSection, NodeSummary } from "../bridge";
import { ModalDialog } from "../controls";
import { errorLine } from "../error-text";
import { t } from "../i18n";
import { formatBytes } from "../subscription-format";

const sectionLabels: Record<string, string> = {
  download: "下载",
  latency: "延迟",
};

const rowLabels: Record<string, string> = {
  bloat: "延迟增加",
  bytes: "下载量",
  cold_handshake: "首次握手",
  failure: "失败原因",
  handshake: "握手延迟",
  idle: "空闲延迟",
  jitter: "抖动",
  loaded: "负载下延迟",
  loaded_loss: "负载下丢包",
  loss: "丢包",
  mbps: "平均速度",
  peak_mbps: "峰值速度",
  request: "请求延迟",
  warm_handshake: "后续握手",
};

const failures: Record<string, string> = {
  connect_failed: "连接失败",
  http_status: "HTTP 状态异常",
  not_http: "不是 HTTP 应答",
  timeout: "超时",
  tls_failed: "TLS 握手失败",
};

function rowValue({ kind, value }: NodeCheckupSection["rows"][number]): string {
  if (kind === "failure" && typeof value === "string") {
    return t(Object.hasOwn(failures, value) ? failures[value] : "操作没有完成");
  }
  if (typeof value !== "number") return "—";
  switch (kind) {
    case "ms": return `${value.toFixed(value < 10 ? 1 : 0)} ms`;
    case "pct": return `${value.toFixed(0)}%`;
    case "mbps": return `${value.toFixed(value < 10 ? 2 : 1)} Mbps`;
    case "bytes": return formatBytes(value);
    default: return String(value);
  }
}

export function NodeCheckup({ node, onClose }: { node: NodeSummary; onClose: () => void }) {
  const [download, setDownload] = useState(false);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<NodeCheckupSection[] | null>(null);
  const [error, setError] = useState<{ cause: unknown } | null>(null);

  const start = async () => {
    setRunning(true);
    setResult(null);
    setError(null);
    try {
      const answer = await desktop.nodeCheckup(node.id, download);
      setResult(answer.sections);
    } catch (cause) {
      setError({ cause });
    } finally {
      setRunning(false);
    }
  };

  return (
    <ModalDialog onClose={running ? undefined : onClose}>
      <div className="terms-head">
        <div>
          <Heading slot="title">{t("节点体检")}</Heading>
          <p>{node.label}</p>
        </div>
      </div>
      <div className="checkup-body">
        <p>{t("经该节点访问 1.1.1.1 和 www.gstatic.com 各 5 次，测握手、请求延迟、抖动和丢包。仅在开始检测时运行。")}</p>
        <label>
          <input type="checkbox" checked={download} disabled={running} onChange={(event) => setDownload(event.target.checked)} />
          {t("测下载速度")}
        </label>
        {download && <p>{t("另外从 speed.cloudflare.com 下载约 8 秒测速度，最多 200 MB，会消耗节点流量。")}</p>}
        <div role="status">
          {running && t("正在体检…")}
          {result?.filter((section) => Object.hasOwn(sectionLabels, section.id)).map((section) => (
            <section key={section.id}>
              <h3 className="subhead">{t(sectionLabels[section.id])}</h3>
              <dl className="facts">
                {section.rows.filter((row) => Object.hasOwn(rowLabels, row.id)).map((row) => (
                  <div key={row.id}>
                    <dt>{t(rowLabels[row.id])}</dt>
                    <dd className={row.warn ? "warning-text" : undefined}>{rowValue(row)}</dd>
                  </div>
                ))}
              </dl>
              {section.capped && <p>{t("达到 200 MB 上限，提前结束。")}</p>}
            </section>
          ))}
        </div>
        {error && <p role="alert" className="danger-text">{errorLine(error.cause)}</p>}
      </div>
      <div className="terms-actions">
        <button className="secondary" disabled={running} onClick={onClose}>{t("关闭|dialog")}</button>
        <button className="primary" disabled={running} onClick={() => void start()}>
          {t(result || error ? "再测一次" : "开始体检")}
        </button>
      </div>
    </ModalDialog>
  );
}
