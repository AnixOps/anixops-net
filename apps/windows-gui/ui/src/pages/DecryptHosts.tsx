import { Ban, Check, Globe, LockKeyhole, Pencil, Plug, Radio, ShieldCheck, TriangleAlert, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { CaptureStatus, DecryptDomain, desktop, MitmHostOutcome, MitmRootStatus, RuntimeSnapshot } from "../bridge";
import { excludedBy, failureReason, isFailing, matchesPattern, pendingSteps, SetupStep } from "../decrypt-hosts";
import { liveEngine } from "../live";
import { Card, IconButton, Row, Run, Segmented, Switch, TileColor } from "../ui";
import { t } from "../i18n";

/**
 * The decryption list's patterns that are on, its exclusions and the whole list, read again after every change and
 * when the window comes back to the front.
 */
export function useDecryptHosts(): [string[], () => void, string[], DecryptDomain[]] {
  const [domains, setDomains] = useState<DecryptDomain[]>([]);
  const load = useCallback(() => {
    desktop.decryptDomains().then(setDomains).catch(() => setDomains([]));
  }, []);
  useEffect(() => {
    load();
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, [load]);
  const hosts = domains.filter((domain) => domain.enabled && !domain.exclude).map((domain) => domain.pattern);
  const excluded = domains.filter((domain) => domain.enabled && domain.exclude).map((domain) => domain.pattern);
  return [hosts, load, excluded, domains];
}

/** The engine's per-domain decryption results, read every five seconds while the engine runs. */
function useMitmHosts(): [MitmHostOutcome[], () => void] {
  const [hosts, setHosts] = useState<MitmHostOutcome[]>([]);
  const load = useCallback(() => {
    desktop.mitmHosts().then((answer) => setHosts(answer.hosts ?? [])).catch(() => setHosts([]));
  }, []);
  useEffect(() => {
    load();
    const timer = window.setInterval(load, 5000);
    return () => window.clearInterval(timer);
  }, [load]);
  const clear = useCallback(() => {
    desktop.mitmHosts(true).then((answer) => setHosts(answer.hosts ?? [])).catch(() => setHosts([]));
  }, []);
  return [hosts, clear];
}

/**
 * "Rewrite" › Decrypted domains, for developers and power users: domains and wildcard patterns to decrypt,
 * exclusions that win over every include (plugins' too), each line on or paused and edited in place. While the
 * engine runs, its per-domain results (`mitm_hosts`) mark the domains whose decryption failed and why, certificate
 * pinning first, with a one-click exclusion. The list starts empty: nothing is bundled (public builds carry no
 * presets).
 */
export function DecryptHostsCard({ busy, run }: { busy: string | null; run: Run }) {
  const [, reload, excluded, domains] = useDecryptHosts();
  const [outcomes, clearOutcomes] = useMitmHosts();
  const [typed, setTyped] = useState("");
  const [kind, setKind] = useState<"decrypt" | "exclude">("decrypt");
  const [editing, setEditing] = useState<{ pattern: string; text: string; kind: "decrypt" | "exclude" } | null>(null);
  const change = (action: () => Promise<{ message: string }>) => run("decrypt-host", action).then(reload);
  const add = () => {
    const host = typed.trim();
    if (!host) return;
    void change(() => desktop.addDecryptHost(host, kind === "exclude")).then(() => setTyped(""));
  };
  const save = () => {
    if (!editing) return;
    const { pattern, text, kind: next } = editing;
    void change(() => desktop.editDecryptHost(pattern, text.trim(), next === "exclude")).then(() => setEditing(null));
  };
  const failing = outcomes.filter(isFailing);
  const decrypting = domains.filter((domain) => !domain.exclude);
  const excluding = domains.filter((domain) => domain.exclude);
  const kinds = [
    { value: "decrypt" as const, label: t("解密") },
    { value: "exclude" as const, label: t("排除") },
  ];
  const status = (domain: DecryptDomain): string | undefined => {
    if (!domain.enabled) return t("已停用");
    if (domain.exclude) return undefined;
    const seen = outcomes.filter((outcome) => matchesPattern(domain.pattern, outcome.host));
    if (seen.length === 0) return undefined;
    const decrypted = seen.reduce((sum, outcome) => sum + outcome.decrypted, 0);
    const failed = seen.filter(isFailing).length;
    return failed > 0
      ? t("已解密 {count} 个连接，{failed} 个域名失败", { count: decrypted, failed })
      : t("已解密 {count} 个连接", { count: decrypted });
  };
  const row = (domain: DecryptDomain) => editing?.pattern === domain.pattern ? (
    <div key={domain.pattern} className="row import-row">
      <input
        aria-label={t("编辑域名")}
        spellCheck={false}
        value={editing.text}
        onChange={(event) => setEditing({ ...editing, text: event.target.value })}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !busy) save();
          if (event.key === "Escape") setEditing(null);
        }}
      />
      <Segmented label={t("类型")} options={kinds} value={editing.kind} onChange={(next) => setEditing({ ...editing, kind: next })} />
      <button className="secondary compact" disabled={Boolean(busy) || !editing.text.trim()} onClick={save}>{t("保存")}</button>
      <button className="secondary compact" onClick={() => setEditing(null)}>{t("取消")}</button>
    </div>
  ) : (
    <Row
      key={domain.pattern}
      icon={domain.exclude ? Ban : Globe}
      color={domain.exclude ? "gray" : "pink"}
      label={domain.exclude ? `-${domain.pattern}` : domain.pattern}
      hint={status(domain)}
      className={domain.enabled ? "" : "done"}
    >
      <IconButton
        label={t("编辑 {host}", { host: domain.pattern })}
        disabled={Boolean(busy)}
        onClick={() => setEditing({ pattern: domain.pattern, text: domain.pattern, kind: domain.exclude ? "exclude" : "decrypt" })}
      >
        <Pencil size={15} />
      </IconButton>
      <Switch
        label={t("启用 {host}", { host: domain.pattern })}
        checked={domain.enabled}
        disabled={Boolean(busy)}
        onChange={() => void change(() => desktop.setDecryptHostEnabled(domain.pattern, !domain.enabled))}
      />
      <IconButton
        label={t("删除 {host}", { host: domain.pattern })}
        disabled={Boolean(busy)}
        onClick={() => void change(() => desktop.removeDecryptHost(domain.pattern))}
      >
        <X size={15} />
      </IconButton>
    </Row>
  );
  const count = decrypting.filter((domain) => domain.enabled).length;
  return (
    <Card
      title={t("解密域名")}
      description={t("只解密这里、插件和映射里列出的域名，其余 HTTPS 原样转发。* 匹配任意字符（包括点），? 匹配一个字符；*.example.com 匹配所有子域名，不含 example.com 本身。排除的域名永不解密，优先于所有解密规则；也可以直接写 -example.com。")}
      actions={count > 0 ? <span className="count">{t("{count} 个域名", { count })}</span> : undefined}
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
        <Segmented label={t("类型")} options={kinds} value={kind} onChange={setKind} />
        <button className="secondary compact" disabled={Boolean(busy) || !typed.trim()} onClick={add}>{t("添加")}</button>
      </div>
      {failing.length > 0 && (
        <>
          <p className="muted">{t("解密失败：根据这次连接以来的解密结果自动标出，只记录域名、次数和原因。App 拒绝证书多半是证书固定，无法解密；排除后它的连接原样转发，App 会照常联网。")}</p>
          {failing.map((outcome) => (
            <Row
              key={`failing-${outcome.host}`}
              icon={TriangleAlert}
              color="orange"
              label={outcome.host}
              hint={[t(failureReason(outcome.last_failure)), t("失败 {failed} 次 · 解密 {count} 次", { failed: outcome.failed, count: outcome.decrypted })].join(" · ")}
            >
              {excludedBy(outcome.host, excluded)
                ? <span className="muted">{t("已排除")}</span>
                : (
                  <button
                    className="secondary compact"
                    disabled={Boolean(busy)}
                    onClick={() => void change(() => desktop.addDecryptHost(outcome.host, true))}
                  >
                    {t("排除")}
                  </button>
                )}
            </Row>
          ))}
          <div className="row">
            <button className="secondary compact" onClick={clearOutcomes}>{t("清除这些记录")}</button>
          </div>
        </>
      )}
      {decrypting.map(row)}
      {excluding.map(row)}
      <p className="muted">{t("只解密 443 端口的 HTTPS；列表里域名的 HTTP/3（QUIC）连接会被拒绝，让 App 改用可以解密的 HTTPS。固定了证书的 App（常见于银行、支付和系统服务）和只有 IP 地址的连接不能解密。")}</p>
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
