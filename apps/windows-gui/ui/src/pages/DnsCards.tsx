import { Check, Sparkles } from "lucide-react";
import { useEffect, useState } from "react";
import { desktop, DnsPreset, DnsText, inProcessEngine, RuntimeSnapshot } from "../bridge";
import { Card, Run, ToggleRow } from "../ui";
import { t } from "../i18n";

const dnsChoices: Array<{ value: DnsPreset; label: string; hint: string }> = [
  { value: "system", label: "系统 DNS", hint: "内置引擎用系统 DNS 解析规则里需要 IP 的域名；经节点的连接由节点服务器解析域名。" },
  { value: "encrypted", label: "加密 DNS", hint: "所有域名经节点用 Cloudflare DNS over HTTPS 解析，本地网络看不到查询内容。" },
  { value: "custom", label: "自定义 DNS", hint: "用下面填写的服务器、分域规则和 Hosts。" },
];

/** Examples shown in the empty editors; the formats follow the common [DNS] and [Host] line syntax. */
const SERVERS_PLACEHOLDER = "cloudflare = https://1.1.1.1/dns-query, server-name=cloudflare-dns.com\nalidns = quic://223.5.5.5, server-name=dns.alidns.com, direct, default\nlocal = system, fallback";
const RULES_PLACEHOLDER = "DOMAIN-SUFFIX,example.cn,alidns\nDOMAIN-KEYWORD,intranet,alidns";
const HOSTS_PLACEHOLDER = "router.lan = 192.168.1.1\n*.home.lan = 192.168.1.20, fd00::20";

/**
 * The DNS choice, the Fake-IP switch where the client captures IP traffic (Windows TUN mode, Android), and the
 * "自定义" editor while that choice is selected. Windows applies a change on the next connection, Linux and Android
 * restart a running engine.
 */
export function DnsCards({ snapshot, busy, run }: { snapshot: RuntimeSnapshot | null; busy: string | null; run: Run }) {
  const current = snapshot?.dnsPreset ?? "system";
  const locked = !inProcessEngine(snapshot) && snapshot?.connection === "Connected";
  const [saved, setSaved] = useState<DnsText | null>(null);

  useEffect(() => {
    void desktop.customDns().then(setSaved).catch(() => setSaved(null));
  }, []);

  const save = (text: DnsText) => run("dns", async () => {
    const result = await desktop.saveCustomDns(text);
    setSaved(await desktop.customDns());
    return result;
  });
  const fakeIpHint = snapshot?.platform === "windows"
    ? t("TUN 模式下生效。应用查询域名时立刻拿到 198.18.0.0/15（IPv6 为 fc00::/18）里的地址，连接时还原成域名按规则分流，由节点解析。局域网、对时、STUN 和推送等域名仍拿真实地址。")
    : t("应用查询域名时立刻拿到 198.18.0.0/15（IPv6 为 fc00::/18）里的地址，连接时还原成域名按规则分流，由节点解析。局域网、对时、STUN 和推送等域名仍拿真实地址。");

  return (
    <>
      <Card title="DNS" description={locked ? t("断开后才能更改 DNS。") : undefined}>
        <div role="radiogroup" aria-label="DNS">
        {dnsChoices.map((item) => {
          const selected = item.value === current;
          return (
            <button
              key={item.value}
              role="radio"
              aria-checked={selected}
              className={`row interactive choice ${selected ? "selected" : ""}`}
              disabled={Boolean(busy) || locked || snapshot?.dnsPreset === undefined}
              onClick={() => void run("dns", () => desktop.setDnsPreset(item.value))}
            >
              <span className="row-label"><strong>{t(item.label)}</strong><small>{t(item.hint)}</small></span>
              <span className="check">{selected && <Check size={15} strokeWidth={2.25} />}</span>
            </button>
          );
        })}
        </div>
        {snapshot?.platform !== "linux" && saved && (
          <ToggleRow
            icon={Sparkles}
            color="indigo"
            label="Fake-IP"
            hint={fakeIpHint}
            checked={saved.fake_ip}
            disabled={Boolean(busy) || locked}
            onChange={() => void save({ ...saved, fake_ip: !saved.fake_ip })}
          />
        )}
      </Card>
      {current === "custom" && saved && <CustomDnsCard saved={saved} locked={locked} busy={busy} save={save} />}
    </>
  );
}

/** The "自定义" choice's servers, per-domain servers and hosts, saved together. */
function CustomDnsCard({ saved, locked, busy, save }: {
  saved: DnsText;
  locked: boolean;
  busy: string | null;
  save: (text: DnsText) => Promise<void>;
}) {
  const [draft, setDraft] = useState(saved);
  const changed = draft.servers !== saved.servers || draft.rules !== saved.rules || draft.hosts !== saved.hosts;
  const edit = (field: "servers" | "rules" | "hosts") => (event: { target: { value: string } }) =>
    setDraft({ ...draft, [field]: event.target.value });

  return (
    <Card
      title={t("自定义 DNS")}
      description={t("每行一条，# 开头的行是注释。没有填服务器时，引擎经节点用 TCP 查询默认服务器。")}
    >
      <div className="rule-editor">
        <span className="editor-label">{t("服务器")}<small>{t("名称 = 地址。地址可以是 IP、tcp://、udp://、tls://、https://、quic://（DoQ），或 system（本机网络的 DNS，总是直连）；加 direct 表示直连，不加经节点。加 default 的服务器（最多 4 个）同时查询、取最先的应答，都不加时第一个是默认服务器；加 fallback 的服务器在默认服务器失败时再查。")}</small></span>
        <textarea aria-label={t("服务器")} spellCheck={false} placeholder={SERVERS_PLACEHOLDER} value={draft.servers} onChange={edit("servers")} />
        <span className="editor-label">{t("分域服务器")}<small>{t("类型,域名,服务器名称，从上到下匹配。类型是 DOMAIN、DOMAIN-SUFFIX、DOMAIN-KEYWORD 或 RULE-SET（分流模式里有的规则集）。")}</small></span>
        <textarea aria-label={t("分域服务器")} spellCheck={false} placeholder={RULES_PLACEHOLDER} value={draft.rules} onChange={edit("rules")} />
        <span className="editor-label">Hosts<small>{t("域名 = 地址，最多 8 个地址；*.域名 匹配它的子域名。")}</small></span>
        <textarea aria-label="Hosts" spellCheck={false} placeholder={HOSTS_PLACEHOLDER} value={draft.hosts} onChange={edit("hosts")} />
        <div className="button-row">
          <button className="link-button" disabled={!changed || Boolean(busy)} onClick={() => setDraft(saved)}>{t("还原")}</button>
          <button className="secondary compact" disabled={!changed || Boolean(busy) || locked} onClick={() => void save({ ...draft, fake_ip: saved.fake_ip })}>{t("保存")}</button>
        </div>
      </div>
    </Card>
  );
}
