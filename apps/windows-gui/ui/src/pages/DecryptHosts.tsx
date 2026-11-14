import { Check, Globe, LockKeyhole, Plug, Radio, ShieldCheck, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { CaptureStatus, desktop, MitmRootStatus, RuntimeSnapshot } from "../bridge";
import { pendingSteps, SetupStep } from "../decrypt-hosts";
import { liveEngine } from "../live";
import { Card, IconButton, Row, Run, TileColor } from "../ui";
import { t } from "../i18n";

/** The decrypted domains, read again after every change and when the window comes back to the front. */
export function useDecryptHosts(): [string[], () => void] {
  const [hosts, setHosts] = useState<string[]>([]);
  const load = useCallback(() => {
    desktop.decryptHosts().then(setHosts).catch(() => setHosts([]));
  }, []);
  useEffect(() => {
    load();
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, [load]);
  return [hosts, load];
}

/**
 * "Rewrite" › Decrypted domains: HTTPS decryption is per domain (the user's choice, 2026-11-14). Domains come from
 * this list, the plugins and the mappings; a connection's details add its domain in one tap.
 */
export function DecryptHostsCard({ busy, run }: { busy: string | null; run: Run }) {
  const [hosts, reload] = useDecryptHosts();
  const [typed, setTyped] = useState("");
  const add = () => {
    const host = typed.trim();
    if (!host) return;
    void run("decrypt-host", () => desktop.addDecryptHost(host)).then(() => {
      setTyped("");
      reload();
    });
  };
  return (
    <Card
      title={t("解密域名")}
      description={t("只解密这里、插件和映射里列出的域名，其余 HTTPS 原样转发。也可以在加密连接的详情里点“解密此域名”。写 *.example.com 包含它的所有子域名。")}
      actions={hosts.length > 0 ? <span className="count">{t("{count} 个域名", { count: hosts.length })}</span> : undefined}
    >
      <div className="row import-row">
        <input
          aria-label={t("要解密的域名")}
          placeholder={t("api.example.com 或 *.example.com")}
          spellCheck={false}
          value={typed}
          onChange={(event) => setTyped(event.target.value)}
          onKeyDown={(event) => { if (event.key === "Enter" && !busy) add(); }}
        />
        <button className="secondary compact" disabled={Boolean(busy) || !typed.trim()} onClick={add}>{t("添加")}</button>
      </div>
      {hosts.map((host) => (
        <Row key={host} icon={Globe} color="pink" label={host}>
          <IconButton
            label={t("不再解密 {host}", { host })}
            disabled={Boolean(busy)}
            onClick={() => void run("decrypt-host", () => desktop.removeDecryptHost(host)).then(reload)}
          >
            <X size={15} />
          </IconButton>
        </Row>
      ))}
    </Card>
  );
}

/** How many domains are decrypted in all: the list's, the enabled plugins' and the enabled mappings'. */
function useDecryptedDomainCount(hosts: string[]): number {
  const [others, setOthers] = useState(0);
  useEffect(() => {
    let cancelled = false;
    const load = () => Promise.all([
      desktop.mitmPlugins().catch(() => []),
      desktop.debugMappings().catch(() => []),
    ]).then(([plugins, mappings]) => {
      if (cancelled) return;
      const fromPlugins = plugins.filter((plugin) => plugin.enabled).reduce((sum, plugin) => sum + plugin.hostnames.length, 0);
      setOthers(fromPlugins + mappings.filter((mapping) => mapping.enabled).length);
    });
    void load();
    window.addEventListener("focus", load);
    return () => {
      cancelled = true;
      window.removeEventListener("focus", load);
    };
  }, []);
  return hosts.length + others;
}

const steps: Record<SetupStep, { icon: LucideIcon; color: TileColor; label: string; hint: string }> = {
  connect: { icon: Plug, color: "green", label: "连接", hint: "用连接开关连接。调试模式默认直连，不需要节点。" },
  certificate: { icon: ShieldCheck, color: "orange", label: "信任根证书", hint: "根证书在本机生成，要你按系统提示手动信任一次。" },
  decryption: { icon: LockKeyhole, color: "pink", label: "开启 HTTPS 解密", hint: "开启后只解密你列出的域名。" },
  domains: { icon: Globe, color: "sky", label: "添加要解密的域名", hint: "在“改写”里添加，或在加密连接的详情里点“解密此域名”。" },
  capture: { icon: Radio, color: "indigo", label: "开始抓包", hint: "开始后，新的请求会逐条出现在这里。" },
};

/**
 * Debug mode's first-use checklist (the user's choice, 2026-11-14): what is still missing before HTTPS requests show
 * up, in order, each with the one action that does it. Shown on the requests page until nothing is missing.
 */
export function DebugSetupCard({ snapshot, capture, onStartCapture, onOpenRewrite, compact = false }: {
  snapshot: RuntimeSnapshot | null;
  capture: CaptureStatus | null;
  /** Absent where the page around the checklist already has "Start capture" as its main button. */
  onStartCapture?: () => void;
  onOpenRewrite: () => void;
  /** Lists only the steps still to do: above the request table, where room is short. */
  compact?: boolean;
}) {
  const [hosts] = useDecryptHosts();
  const domains = useDecryptedDomainCount(hosts);
  const [root, setRoot] = useState<MitmRootStatus | null>(null);
  useEffect(() => {
    const load = () => desktop.mitmRootStatus().then(setRoot).catch(() => setRoot(null));
    void load();
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, []);

  const connected = liveEngine(snapshot);
  const pending = pendingSteps({
    connected,
    trusted: root ? root.trusted : null,
    decrypting: Boolean(snapshot?.httpsDecryptionEnabled),
    domains,
    capturing: Boolean(capture?.enabled),
  });
  if (pending.length === 0) return null;

  const action = (step: SetupStep) => {
    switch (step) {
      case "connect":
        return null;
      case "capture":
        return connected && onStartCapture ? <button className="secondary compact" onClick={onStartCapture}>{t("开始抓包")}</button> : null;
      default:
        return <button className="secondary compact" onClick={onOpenRewrite}>{t("去改写页")}</button>;
    }
  };
  const all: SetupStep[] = ["connect", "certificate", "decryption", "domains", "capture"];

  return (
    <Card
      className="debug-setup"
      title={t("抓 HTTPS 请求前还差 {count} 步", { count: pending.length })}
      description={t("明文 HTTP 请求不需要这些步骤，抓包开始后就会列出。")}
    >
      {all.filter((step) => (step !== "certificate" || root) && (!compact || pending.includes(step))).map((step) => {
        const item = steps[step];
        const done = !pending.includes(step);
        return (
          <Row key={step} icon={done ? Check : item.icon} color={done ? "green" : item.color} label={t(item.label)} hint={done ? undefined : t(item.hint)} className={done ? "done" : ""}>
            {done ? <span className="muted">{t("已完成")}</span> : action(step)}
          </Row>
        );
      })}
    </Card>
  );
}
