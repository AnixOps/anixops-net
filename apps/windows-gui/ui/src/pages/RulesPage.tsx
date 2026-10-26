import { Check, Layers, ListChecks, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { desktop, DnsPreset, NativeGroupSummary, RuleSetStatus, RuntimeSnapshot } from "../bridge";
import { availableRoutingModes, groupMemberLabel } from "../labels";
import { liveEngine, usePolling } from "../live";
import { Card, Run, Segmented, StatusPill } from "../ui";
import { t } from "../i18n";

export function RulesPage({ snapshot, busy, run }: { snapshot: RuntimeSnapshot | null; busy: string | null; run: Run }) {
  return (
    <div className="page-stack">
      <Card
        title={t("分流模式")}
        description={t("规则资源从公开来源缓存到本机，下次连接时生效。")}
        actions={<button className="secondary" disabled={Boolean(busy)} onClick={() => void run("routing rules", desktop.refreshRoutingRules)}><RefreshCw size={15} />{t("刷新规则资源")}</button>}
      >
        <div className="mode-grid" role="radiogroup" aria-label={t("分流模式")}>
          {availableRoutingModes(snapshot).map((mode) => {
            const selected = snapshot?.routingMode === mode.value;
            return (
              <button
                key={mode.value}
                role="radio"
                aria-checked={selected}
                className={`mode-card ${selected ? "selected" : ""}`}
                disabled={Boolean(busy) || !snapshot}
                onClick={() => void run("routing", () => desktop.setRoutingMode(mode.value))}
              >
                <span className="mode-title">{t(mode.label)}{selected && <Check size={16} />}</span>
                <small>{t(mode.hint)}</small>
              </button>
            );
          })}
        </div>
      </Card>
      <PolicyGroups busy={busy} run={run} />
      <RuleSets snapshot={snapshot} busy={busy} run={run} />
      <DnsCard snapshot={snapshot} busy={busy} run={run} />
    </div>
  );
}

/** The profile's policy groups: pick a selector's default. */
function PolicyGroups({ busy, run }: { busy: string | null; run: Run }) {
  const [groups, setGroups] = useState<NativeGroupSummary[]>([]);

  useEffect(() => {
    void desktop.nativeGroups().then(setGroups).catch(() => setGroups([]));
  }, []);

  return (
    <Card
      title={t("策略组")}
      description={t("为手动选择的策略组设定默认出口。")}
      actions={<StatusPill label={groups.length ? t("{count} 个策略组", { count: groups.length }) : t("没有策略组")} tone={groups.length ? "success" : "neutral"} />}
    >
      {groups.length === 0
        ? <p className="muted">{t("当前配置里没有策略组。")}</p>
        : <ul className="list groups">
          {groups.map((group) => (
            <li key={group.tag}>
              <Layers size={16} className="list-icon" />
              <span className="list-main"><strong>{group.tag}</strong><small>{group.groupType === "selector" ? t("手动选择") : group.groupType === "urltest" ? t("自动测速") : group.groupType}{group.selected ? ` · ${groupMemberLabel(group, group.selected)}` : ""}</small></span>
              <div className="group-options">
                {group.groupType === "selector" && group.outbounds.map((outbound) => (
                  <button
                    key={outbound}
                    className={`chip-button ${group.selected === outbound ? "selected" : ""}`}
                    aria-pressed={group.selected === outbound}
                    disabled={Boolean(busy) || group.selected === outbound}
                    onClick={() => void run("native selector", async () => {
                      const result = await desktop.selectNativeGroupOutbound(group.tag, outbound);
                      setGroups((current) => current.map((item) => item.tag === group.tag ? { ...item, selected: outbound } : item));
                      return result;
                    })}
                  >
                    {groupMemberLabel(group, outbound)}
                  </button>
                ))}
              </div>
            </li>
          ))}
        </ul>}
    </Card>
  );
}

/** The running engine's rule sets: size, source and the last download of remote ones. */
function RuleSets({ snapshot, busy, run }: { snapshot: RuntimeSnapshot | null; busy: string | null; run: Run }) {
  const [answer, reload] = usePolling(desktop.ruleSets, liveEngine(snapshot));
  const sets = answer?.rule_sets ?? [];
  if (sets.length === 0) return null;
  const remote = sets.some((set) => set.source === "remote");

  return (
    <Card
      title={t("规则集")}
      description={t("当前连接正在使用的规则集。")}
      actions={remote && <button className="secondary" disabled={Boolean(busy)} onClick={() => void run("rule sets", async () => {
        const result = await desktop.updateRuleSets();
        reload();
        return { message: t("已开始更新 {count} 个远程规则集", { count: result.scheduled }) };
      })}><RefreshCw size={15} />{t("更新远程规则集")}</button>}
    >
      <ul className="list">
        {sets.map((set) => (
          <li key={set.name}>
            <ListChecks size={16} className="list-icon" />
            <span className="list-main"><strong>{set.name}</strong><small>{ruleSetDetail(set)}</small></span>
          </li>
        ))}
      </ul>
    </Card>
  );
}

function ruleSetDetail(set: RuleSetStatus): string {
  return [
    set.source === "remote" ? t("远程") : t("内置"),
    t("{count} 条", { count: set.entries }),
    set.updated_at_ms ? t("更新于 {time}", { time: new Date(set.updated_at_ms).toLocaleString() }) : set.source === "remote" ? t("尚未下载") : null,
    set.last_error ? t("上次下载失败：{error}", { error: set.last_error }) : null,
  ].filter(Boolean).join(" · ");
}

const dnsChoices: Array<{ value: DnsPreset; label: string; hint: string }> = [
  { value: "system", label: "系统 DNS", hint: "内置引擎用系统 DNS 解析规则里需要 IP 的域名；经节点的连接由节点服务器解析域名。" },
  { value: "encrypted", label: "加密 DNS", hint: "所有域名经节点用 Cloudflare DNS over HTTPS 解析，本地网络看不到查询内容。" },
  { value: "split", label: "国内外分流", hint: "国内域名直连阿里 DNS over HTTPS 解析，其余经节点用 Cloudflare 解析，国内网站拿到就近的地址。" },
];

/** The DNS choice; Windows applies it on the next connection, Linux restarts a running engine. */
function DnsCard({ snapshot, busy, run }: { snapshot: RuntimeSnapshot | null; busy: string | null; run: Run }) {
  const current = snapshot?.dnsPreset ?? "system";
  const choice = dnsChoices.find((item) => item.value === current) ?? dnsChoices[0];
  const locked = snapshot?.platform !== "linux" && snapshot?.connection === "Connected";

  return (
    <Card title="DNS" description={locked ? t("断开后才能更改 DNS。") : undefined} actions={<StatusPill label={t(choice.label)} tone="success" />}>
      {snapshot?.dnsPreset !== undefined && (
        <Segmented
          label="DNS"
          options={dnsChoices.map((item) => ({ value: item.value, label: t(item.label) }))}
          value={current}
          disabled={Boolean(busy) || locked}
          onChange={(preset) => void run("dns", () => desktop.setDnsPreset(preset))}
        />
      )}
      <p className="muted">{t(choice.hint)}</p>
    </Card>
  );
}
