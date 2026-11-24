import { ArrowRightLeft, Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { desktop, RedirectDraft, RedirectNetwork, TrafficRedirect } from "../bridge";
import { Card, EmptyState, Field, FoldedRows, MoreMenu, Row, Run, Segmented, Switch } from "../ui";
import { t } from "../i18n";

const emptyDraft: RedirectDraft = { source: "", ports: "", network: "any", target: "" };

const networks: Array<{ value: RedirectNetwork; label: string }> = [
  { value: "any", label: "TCP 和 UDP" },
  { value: "tcp", label: "TCP" },
  { value: "udp", label: "UDP" },
];

/** What a rewrite takes, as the list shows it. */
function redirectLabel(entry: TrafficRedirect): string {
  const draft = entry.draft;
  if (!draft) return entry.match;
  const ports = draft.ports ? ":" + draft.ports : "";
  const network = draft.network === "any" ? "" : " " + draft.network.toUpperCase();
  return (draft.source || t("任意地址")) + ports + network;
}

/**
 * Destination rewrites, as the iOS 重定向 page: matching TCP connections and UDP flows go to another address, for
 * example a game's server to a development server. The engine applies them before routing.
 */
export function TrafficRedirectsCard({ busy, run, platform }: { busy: string | null; run: Run; platform?: string }) {
  const [entries, setEntries] = useState<TrafficRedirect[]>([]);
  const [editing, setEditing] = useState<{ id: string | null; draft: RedirectDraft } | null>(null);
  const load = useCallback(() => {
    desktop.trafficRedirects().then(setEntries).catch(() => setEntries([]));
  }, []);
  useEffect(load, [load]);
  const disabled = Boolean(busy);

  const saveList = (next: TrafficRedirect[]) => void run("redirects", () =>
    desktop.saveTrafficRedirects(next.map(({ id, match, target, enabled }) => ({ id, match, target, enabled }))),
  ).then(load);
  const saveDraft = () => {
    if (!editing) return;
    void run("redirects", async () => {
      const result = await desktop.addTrafficRedirect(editing.draft, editing.id);
      setEditing(null);
      return result;
    }).then(load);
  };
  const change = (patch: Partial<RedirectDraft>) => setEditing((current) => current && { ...current, draft: { ...current.draft, ...patch } });

  return (
    <Card
      title={t("流量重定向")}
      description={platform === "android"
        ? t("把匹配的 TCP 连接和 UDP 流量改发到另一个地址，例如把游戏服务器换成开发服务器。在路由之前生效，VPN 接管的所有应用都适用。")
        : t("把匹配的 TCP 连接和 UDP 流量改发到另一个地址，例如把游戏服务器换成开发服务器。在路由之前生效；不走系统代理的游戏需要打开 TUN 模式。")}
      actions={!editing && (
        <button className="secondary compact" disabled={disabled} onClick={() => setEditing({ id: null, draft: emptyDraft })}>
          <Plus size={14} />{t("新建重定向")}
        </button>
      )}
    >
      {editing && (
        <div className="rows">
          <Field label={t("匹配（地址、网段、域名，或带类型的规则）")}>
            <input value={editing.draft.source} spellCheck={false} placeholder="203.0.113.10" onChange={(event) => change({ source: event.target.value })} />
          </Field>
          <Field label={t("端口（可选，单个或范围）")}>
            <input value={editing.draft.ports} spellCheck={false} placeholder="7000-7100" onChange={(event) => change({ ports: event.target.value })} />
          </Field>
          <Row label={t("协议")}>
            <Segmented label={t("协议")} options={networks.map((option) => ({ ...option, label: t(option.label) }))} value={editing.draft.network} onChange={(network) => change({ network })} />
          </Row>
          <Field label={t("改发到（地址、地址:端口，或只写 :端口）")}>
            <input value={editing.draft.target} spellCheck={false} placeholder="192.168.1.20:7001" onChange={(event) => change({ target: event.target.value })} />
          </Field>
          <div className="button-row">
            <button className="secondary" disabled={disabled} onClick={() => setEditing(null)}>{t("取消")}</button>
            <button className="primary" disabled={disabled || !editing.draft.target.trim()} onClick={saveDraft}>{t("保存")}</button>
          </div>
        </div>
      )}
      {!editing && entries.length === 0 && (
        <EmptyState icon={ArrowRightLeft} title={t("还没有重定向")} message={t("新建一条，把某个地址或域名的流量改发到开发服务器。")} />
      )}
      {!editing && entries.length > 0 && (
        <FoldedRows
          items={entries}
          label={(count) => t("显示全部 {count} 条重定向", { count })}
          render={(entry) => (
            <Row
              key={entry.id}
              icon={ArrowRightLeft}
              color={entry.enabled ? "teal" : "gray"}
              label={<span className="mapping-pattern" title={entry.match}>{redirectLabel(entry)}</span>}
              hint={"→ " + entry.target}
              className="plugin-row"
            >
              <span className="row-actions">
                <MoreMenu
                  label={t("更多")}
                  disabled={disabled}
                  items={[
                    ...(entry.draft ? [{ label: t("编辑"), onSelect: () => setEditing({ id: entry.id, draft: entry.draft as RedirectDraft }) }] : []),
                    {
                      label: t("删除重定向"),
                      icon: Trash2,
                      danger: true,
                      confirm: t("确认删除 {name}", { name: redirectLabel(entry) }),
                      onSelect: () => saveList(entries.filter((other) => other.id !== entry.id)),
                    },
                  ]}
                />
              </span>
              <Switch
                label={redirectLabel(entry)}
                checked={entry.enabled}
                disabled={disabled}
                onChange={() => saveList(entries.map((other) => (other.id === entry.id ? { ...other, enabled: !other.enabled } : other)))}
              />
            </Row>
          )}
        />
      )}
    </Card>
  );
}
