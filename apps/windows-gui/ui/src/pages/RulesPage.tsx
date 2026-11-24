import { Check, Layers, ListChecks, RefreshCw, TriangleAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { desktop, NativeGroupSummary, RuleSetStatus, RuntimeSnapshot } from "../bridge";
import { skippedLines } from "../rule-sets";
import { availableRoutingModes, groupMemberLabel } from "../labels";
import { liveEngine, usePolling } from "../live";
import { Card, Row, Run } from "../ui";
import { Select } from "../controls";
import { t } from "../i18n";
import { routingWarningText } from "../error-text";
import { DnsCards } from "./DnsCards";
import { MyRules } from "./MyRules";
import { DataLists, DeviceNames, RuleHits, RuleTester } from "./RuleTools";

export function RulesPage({ snapshot, busy, run }: { snapshot: RuntimeSnapshot | null; busy: string | null; run: Run }) {
  const [sets, reloadSets] = usePolling(desktop.ruleSets, liveEngine(snapshot));
  const missing = sets && Array.isArray(sets.missing) ? sets.missing : [];
  return (
    <div className="page-stack">
      <Card
        title={t("分流模式")}
        description={t("无预设规则：规则模式仅按订阅规则和自定义规则分流。")}
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
      <RuleTester snapshot={snapshot} busy={busy} />
      <RuleHits snapshot={snapshot} busy={busy} run={run} />
      <DataLists missing={missing} busy={busy} run={run} />
      <DeviceNames busy={busy} run={run} />
      <RuleSets answer={sets} reload={reloadSets} busy={busy} run={run} />
      <ImportWarnings snapshot={snapshot} />
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
function RuleSets({ answer, reload, busy, run }: {
  answer: { rule_sets: RuleSetStatus[] } | null;
  reload: () => void;
  busy: string | null;
  run: Run;
}) {
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

/** What importing the subscription's rules left out, so a rule that does not run is never silent. */
function ImportWarnings({ snapshot }: { snapshot: RuntimeSnapshot | null }) {
  const [warnings, setWarnings] = useState<string[]>([]);
  // Asked again when the subscription is switched or refreshed.
  const profile = `${snapshot?.subscription ?? ""}|${snapshot?.subscriptionLastUpdated ?? ""}`;

  useEffect(() => {
    void desktop.routingWarnings().then((lines) => setWarnings(Array.isArray(lines) ? lines : [])).catch(() => setWarnings([]));
  }, [profile]);

  if (warnings.length === 0) return null;
  return (
    <Card
      title={t("导入时跳过的规则")}
      description={t("订阅里的这些规则客户端不支持，没有生效。")}
      actions={<span className="count">{t("{count} 条", { count: warnings.length })}</span>}
    >
      {warnings.map((warning, index) => {
        // The kind of line in the window's language; the kernel's English line says which rule or group.
        const { text, detail } = routingWarningText(warning);
        return <Row key={index} icon={TriangleAlert} color="orange" label={text} hint={detail} />;
      })}
    </Card>
  );
}

function ruleSetDetail(set: RuleSetStatus): string {
  const skipped = skippedLines(set);
  return [
    set.source === "remote" ? t("远程") : t("内置"),
    t("{count} 条", { count: set.entries }),
    set.updated_at_ms ? t("更新于 {time}", { time: new Date(set.updated_at_ms).toLocaleString() }) : set.source === "remote" ? t("尚未下载") : null,
    set.last_error ? t("上次下载失败：{error}", { error: set.last_error }) : null,
    skipped.count > 0 ? t("列表里有 {count} 条规则不支持，已跳过：{types}", { count: skipped.count, types: skipped.summary }) : null,
  ].filter(Boolean).join(" · ");
}
