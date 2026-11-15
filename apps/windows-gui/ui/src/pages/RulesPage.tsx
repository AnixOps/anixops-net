import { Check, Layers, ListChecks, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { desktop, NativeGroupSummary, RuleSetStatus, RuntimeSnapshot } from "../bridge";
import { availableRoutingModes, groupMemberLabel } from "../labels";
import { liveEngine, usePolling } from "../live";
import { Card, Row, Run } from "../ui";
import { Select } from "../controls";
import { t } from "../i18n";
import { DnsCards } from "./DnsCards";
import { MyRules } from "./MyRules";

export function RulesPage({ snapshot, busy, run }: { snapshot: RuntimeSnapshot | null; busy: string | null; run: Run }) {
  return (
    <div className="page-stack">
      <Card
        title={t("分流模式")}
        description={t("客户端不带预设规则：规则模式只按订阅自带的规则和你的规则分流。")}
      >
        <div role="radiogroup" aria-label={t("分流模式")}>
        {availableRoutingModes().map((mode) => {
          const selected = snapshot?.routingMode === mode.value;
          return (
            <button
              key={mode.value}
              role="radio"
              aria-checked={selected}
              className={`row interactive choice ${selected ? "selected" : ""}`}
              disabled={Boolean(busy) || !snapshot}
              onClick={() => void run("routing", () => desktop.setRoutingMode(mode.value))}
            >
              <span className="row-label"><strong>{t(mode.label)}</strong><small>{t(mode.hint)}</small></span>
              <span className="check">{selected && <Check size={15} strokeWidth={2.25} />}</span>
            </button>
          );
        })}
        </div>
      </Card>
      <MyRules busy={busy} run={run} />
      <PolicyGroups busy={busy} run={run} />
      <RuleSets snapshot={snapshot} busy={busy} run={run} />
      <DnsCards snapshot={snapshot} busy={busy} run={run} />
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
      actions={groups.length > 0 && <span className="count">{t("{count} 个策略组", { count: groups.length })}</span>}
    >
      {groups.length === 0
        ? <p className="muted">{t("当前配置里没有策略组。")}</p>
        : groups.map((group) => (
          <Row
            key={group.tag}
            icon={Layers}
            color="orange"
            label={group.tag}
            hint={group.groupType === "selector" ? t("手动选择") : group.groupType === "urltest" ? t("自动测速") : group.groupType}
            value={group.groupType === "selector" ? undefined : groupMemberLabel(group, group.selected)}
          >
            {group.groupType === "selector" && (
              <Select
                label={group.tag}
                value={group.selected ?? ""}
                placeholder={t("未选择")}
                disabled={Boolean(busy)}
                onChange={(outbound) => {
                  void run("native selector", async () => {
                    const result = await desktop.selectNativeGroupOutbound(group.tag, outbound);
                    setGroups((current) => current.map((item) => item.tag === group.tag ? { ...item, selected: outbound } : item));
                    return result;
                  });
                }}
                choices={group.outbounds.map((outbound) => ({ value: outbound, label: groupMemberLabel(group, outbound) ?? outbound }))}
              />
            )}
          </Row>
        ))}
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
      actions={remote && <button className="link-button" disabled={Boolean(busy)} onClick={() => void run("rule sets", async () => {
        const result = await desktop.updateRuleSets();
        reload();
        return { message: t("已开始更新 {count} 个远程规则集", { count: result.scheduled }) };
      })}><RefreshCw size={13} />{t("更新远程规则集")}</button>}
    >
      {sets.map((set) => <Row key={set.name} icon={ListChecks} color={set.source === "remote" ? "sky" : "gray"} label={set.name} hint={ruleSetDetail(set)} />)}
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
