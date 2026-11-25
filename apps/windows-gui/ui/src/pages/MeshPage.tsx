import { Network, Plus, Search, TriangleAlert, X } from "lucide-react";
import { Heading } from "react-aria-components";
import { useCallback, useEffect, useState } from "react";
import { MoreMenu, ModalDialog, Select } from "../controls";
import { t } from "../i18n";
import { keepIfUnchanged, usePageVisible } from "../live";
import {
  connectionLabel,
  draftConfig,
  draftProblem,
  draftProblemText,
  emptyDraft,
  failureText,
  joinedState,
  joiningText,
  latencyText,
  markLabel,
  meshApi,
  noticeDetail,
  noticeText,
  regionName,
  roleLabel,
  securityLabel,
  securityTone,
  stateLabel,
  stateTone,
  type MemberQuery,
  type MeshDraft,
  type MeshGroup,
  type MeshMemberRow,
  type MeshRefusal,
  type MeshSavedNetwork,
  type MeshSubnetRow,
  type MeshView,
} from "../mesh";
import { Card, CopyButton, EmptyState, Field, PageTools, Row, Segmented, StatusDot, Switch } from "../ui";

// 组网 on Windows and Linux (docs/mesh-ui.md): the network, its status bar and join switch at the top, the notices,
// then 成员 · 子网 · 路径 · 本机. The member list has a detail column on the right. The proxy's connection switch stays
// in the sidebar; this page has none of its own.

export const defaultMemberQuery: MemberQuery = { groupBy: "region", filter: "all", sort: "name", query: "" };

/**
 * The mesh page's data, read every two seconds while the window shows: null while the engine has no mesh module
 * (the sidebar entry is then hidden). The member query goes to the shared views, which group, filter and sort.
 */
export function useMesh() {
  const visible = usePageVisible();
  const [query, setQuery] = useState<MemberQuery>(defaultMemberQuery);
  const [view, setView] = useState<MeshView | null>(null);
  const [saved, setSaved] = useState<MeshSavedNetwork[]>([]);
  const reload = useCallback(async () => {
    const [next, networks] = await Promise.all([
      meshApi.view(query).catch(() => null),
      meshApi.networks().catch(() => null),
    ]);
    setView((previous) => keepIfUnchanged(previous, next));
    if (networks) setSaved((previous) => keepIfUnchanged(previous, networks));
  }, [query]);
  useEffect(() => {
    if (!visible) return;
    let polling = false;
    const poll = () => {
      if (polling) return;
      polling = true;
      void reload().finally(() => {
        polling = false;
      });
    };
    poll();
    const timer = window.setInterval(poll, 2000);
    return () => window.clearInterval(timer);
  }, [reload, visible]);
  return { view, saved, query, setQuery, reload };
}

type Tab = "members" | "subnets" | "paths" | "self";

export function MeshPage({ mesh }: { mesh: ReturnType<typeof useMesh> }) {
  const { view, saved, query, setQuery, reload } = mesh;
  const [tab, setTab] = useState<Tab>("members");
  const [adding, setAdding] = useState(false);
  const [busy, setBusy] = useState(false);
  const [refusal, setRefusal] = useState<MeshRefusal | null>(null);
  const [selected, setSelected] = useState<string | null>(null);

  const perform = (task: () => Promise<MeshRefusal | null>) => {
    if (busy) return;
    setBusy(true);
    void task()
      .then(setRefusal)
      .finally(() => {
        setBusy(false);
        void reload();
      });
  };

  if (!view) return <EmptyState icon={Network} title={t("组网")} message={t("连接后显示组网状态。")} />;
  const network = view.head.network;
  const networkId = network?.id ?? null;
  const networkName = network?.name || networkId || "";
  const dialog = adding && (
    <AddNetworkDialog
      taken={saved.map((item) => item.name)}
      onClose={() => setAdding(false)}
      onJoin={(config) => {
        setAdding(false);
        perform(() => meshApi.save(config));
      }}
    />
  );

  if (!network) {
    return (
      <>
        <EmptyState icon={Network} title={t("暂无网络")} message={t("加入网络后显示成员与子网。")}>
          <button className="primary" onClick={() => setAdding(true)}><Plus size={14} />{t("添加网络")}</button>
        </EmptyState>
        {dialog}
      </>
    );
  }

  const security = securityLabel(network.security);
  const status = joiningText(network.state === "joining" ? network.joining : null)
    ?? (network.state === "joined" || network.state === "partial"
      ? t("{state} · {online}/{total} 台在线", { state: stateLabel(network.state), online: view.entry.online, total: view.entry.total })
      : stateLabel(network.state));
  const others = saved.filter((item) => item.name !== networkId);

  return (
    <div className="mesh-page">
      <PageTools>
        <MoreMenu
          label={t("更多")}
          items={[
            { label: t("添加网络"), icon: Plus, onSelect: () => setAdding(true) },
            {
              label: t("移除网络"),
              danger: true,
              confirm: t("移除网络“{name}”？", { name: networkName }),
              onSelect: () => perform(() => meshApi.remove(networkId ?? "")),
            },
          ]}
        />
      </PageTools>
      <section className="mesh-status" aria-label={t("组网状态")}>
        <div className="mesh-network">
          {others.length > 0 ? (
            <Select
              label={t("网络")}
              value={networkId ?? ""}
              choices={saved.map((item) => ({ value: item.name, label: item.name }))}
              onChange={(name) => perform(() => meshApi.setJoined(name, true))}
              disabled={busy}
            />
          ) : (
            <strong className="mesh-network-name">{networkName}</strong>
          )}
          <span className="mesh-state">
            <StatusDot tone={stateTone(network.state)} />
            <span>{status}</span>
          </span>
          {security && <span className={`mesh-tag ${securityTone(network.security)}`}>{security}</span>}
        </div>
        <Switch
          label={t("加入")}
          checked={joinedState(network.state)}
          disabled={busy}
          busy={busy}
          onChange={() => perform(() => meshApi.setJoined(networkId ?? "", !joinedState(network.state)))}
        />
      </section>
      {refusal && <NoticeBar text={failureText(refusal.code)} detail={refusal.detail} onDismiss={() => setRefusal(null)} />}
      {view.notices.map((notice) => <NoticeBar key={notice.key} text={noticeText(notice)} detail={noticeDetail(notice) ?? undefined} />)}
      <div className="mesh-tools">
        <Segmented<Tab>
          label={t("组网")}
          value={tab}
          onChange={setTab}
          options={[
            { value: "members", label: t("成员") },
            { value: "subnets", label: t("子网") },
            { value: "paths", label: t("路径") },
            { value: "self", label: t("本机") },
          ]}
        />
        {(tab === "members" || tab === "subnets") && (
          <label className="search">
            <Search size={14} aria-hidden="true" />
            <input
              aria-label={t("搜索名称、域名、地址或子网")}
              placeholder={t("搜索名称、域名、地址或子网")}
              value={query.query}
              onChange={(event) => setQuery({ ...query, query: event.target.value })}
            />
          </label>
        )}
      </div>
      {tab === "members" && <Members view={view} query={query} setQuery={setQuery} selected={selected} setSelected={setSelected} />}
      {tab === "subnets" && (
        <Subnets view={view} query={query.query} busy={busy} setRoute={(cidr, route) => perform(() => meshApi.setSubnetRoute(networkId ?? "", cidr, route))} />
      )}
      {tab === "paths" && <Paths view={view} />}
      {tab === "self" && <Self view={view} />}
      {dialog}
    </div>
  );
}

function NoticeBar({ text, detail, onDismiss }: { text: string; detail?: string; onDismiss?: () => void }) {
  return (
    <div className="mesh-notice" role="status">
      <TriangleAlert size={15} aria-hidden="true" />
      <div>
        <span>{text}</span>
        {detail && <code>{detail}</code>}
      </div>
      {onDismiss && <button className="icon-button" aria-label={t("关闭提示")} onClick={onDismiss}><X size={14} /></button>}
    </div>
  );
}

// Members.

function Members({ view, query, setQuery, selected, setSelected }: {
  view: MeshView;
  query: MemberQuery;
  setQuery: (query: MemberQuery) => void;
  selected: string | null;
  setSelected: (id: string | null) => void;
}) {
  const members = view.members;
  const [folded, setFolded] = useState<Record<string, boolean>>({});
  const rows = members.groups.flatMap((group) => group.rows);
  const detail = rows.find((row) => row.id === selected) ?? null;
  const grouped = members.groupBy !== "none";
  return (
    <>
      <div className="mesh-filters">
        <Select label={t("分组")} value={query.groupBy} onChange={(groupBy) => setQuery({ ...query, groupBy })} choices={[
          { value: "region", label: t("按地区") },
          { value: "role", label: t("按角色") },
          { value: "none", label: t("不分组") },
        ]} />
        <Select label={t("筛选")} value={query.filter} onChange={(filter) => setQuery({ ...query, filter })} choices={[
          { value: "all", label: t("全部") },
          { value: "online", label: t("仅在线") },
          { value: "unreachable", label: t("不可达") },
          { value: "gateway", label: t("网关") },
          { value: "host", label: t("宿主") },
          { value: "border", label: t("边界") },
          { value: "custom_path", label: t("有自定义路径") },
        ]} />
        <Select label={t("排序")} value={query.sort} onChange={(sort) => setQuery({ ...query, sort })} choices={[
          { value: "name", label: t("名称") },
          { value: "latency", label: t("延迟") },
          { value: "address", label: t("虚拟地址") },
        ]} />
      </div>
      {members.groups.length === 0 ? (
        <p className="mesh-empty">{query.query === "" && query.filter === "all" ? t("暂无成员") : t("无匹配的成员")}</p>
      ) : (
        <div className="mesh-split">
          <div className="mesh-table" role="table" aria-label={t("成员")}>
            <div className="mesh-row mesh-head" role="row">
              <span role="columnheader">{t("成员")}</span>
              {!grouped || members.groupBy === "role" ? <span role="columnheader">{t("地区")}</span> : null}
              <span role="columnheader">{t("连接|route")}</span>
              <span role="columnheader">{t("传输")}</span>
              <span role="columnheader">{t("虚拟地址")}</span>
              <span role="columnheader" className="number">{t("延迟")}</span>
            </div>
            {members.groups.map((group) => {
              const key = group.key ?? "";
              const open = folded[key] === undefined ? group.open : !folded[key];
              return (
                <div key={key || "all"} role="rowgroup">
                  {grouped && (
                    <div className="mesh-group-row" role="row">
                      <span role="rowheader">
                        <button className="mesh-group" aria-expanded={open} onClick={() => setFolded({ ...folded, [key]: open })}>
                          <strong>{groupTitle(view, members.groupBy, group)}</strong>
                          {!open && <small>{groupSummary(group)}</small>}
                        </button>
                      </span>
                    </div>
                  )}
                  {open && group.rows.map((row) => (
                    <MemberRow key={row.id} view={view} row={row} showRegion={!grouped || members.groupBy === "role"} selected={row.id === selected} onSelect={() => setSelected(row.id === selected ? null : row.id)} />
                  ))}
                </div>
              );
            })}
          </div>
          <aside className="mesh-detail" aria-label={t("成员详情")}>
            {detail ? <MemberDetail view={view} row={detail} /> : <p className="mesh-empty">{t("选择成员以查看详情。")}</p>}
          </aside>
        </div>
      )}
    </>
  );
}

function groupTitle(view: MeshView, by: string, group: MeshGroup): string {
  const name = by === "role" ? roleLabel(group.key) ?? group.key ?? "" : regionName(view.head, group.key);
  return t("{name} · {count} 台", { name, count: group.count });
}

function groupSummary(group: MeshGroup): string {
  const parts: string[] = [];
  if (group.link) {
    if (group.link.via) parts.push(t("经 {name}", { name: group.link.via }));
    parts.push(latencyText(group.link.latencyMs));
  }
  if (group.worst) {
    parts.push(group.worst.state === "unreachable"
      ? t("{count} 台不可达", { count: group.worst.count })
      : t("{count} 条路径已回落", { count: group.worst.count }));
  }
  return parts.join(" · ");
}

function MemberRow({ view, row, showRegion, selected, onSelect }: {
  view: MeshView;
  row: MeshMemberRow;
  showRegion: boolean;
  selected: boolean;
  onSelect: () => void;
}) {
  const [all, setAll] = useState(false);
  const children = row.children ?? [];
  const shown = all ? children : children.slice(0, row.visibleChildren ?? children.length);
  const role = roleLabel(row.role);
  return (
    <>
      {/* The row is a table row; its name is the button that selects it, so the keyboard reaches every member. */}
      <div className={`mesh-row selectable ${selected ? "selected" : ""}`} role="row" onClick={onSelect}>
        <span role="cell" className="mesh-member">
          <StatusDot tone={stateTone(row.state)} />
          <button
            className="mesh-member-name truncate"
            aria-pressed={selected}
            onClick={(event) => {
              event.stopPropagation();
              onSelect();
            }}
          >
            {row.name}
          </button>
          {role && <span className="mesh-tag accent">{role}</span>}
          {row.self && <span className="mesh-tag">{t("本机")}</span>}
          {(row.marks ?? []).map((mark) => markLabel(mark) && <span key={mark} className="mesh-tag warning">{markLabel(mark)}</span>)}
          {row.contains && (
            <small>{row.contains.noun === "vms" ? t("{count} 台虚拟机", { count: row.contains.count }) : t("{count} 个子网", { count: row.contains.count })}</small>
          )}
        </span>
        {showRegion && <span role="cell">{regionName(view.head, row.region)}</span>}
        <span role="cell">{connectionLabel(row.connection, row.hops)}</span>
        <span role="cell">{row.transport?.toUpperCase() ?? ""}</span>
        <span role="cell" className="mono">{row.address ?? ""}</span>
        <span role="cell" className="number">{row.self ? "" : <Latency milliseconds={row.latencyMs} tone={row.tone} />}</span>
      </div>
      {(row.open || selected) && shown.map((child) => (
        <div key={`${child.kind}:${child.name}`} className={`mesh-child ${child.match ? "match" : ""}`} role="row">
          <span role="cell" className={child.kind === "subnet" ? "mono" : ""}>{child.name}</span>
          <span role="cell" className="mono">{child.domain ?? child.address ?? ""}</span>
        </div>
      ))}
      {(row.open || selected) && shown.length < children.length && (
        <div className="mesh-child" role="row">
          <span role="cell">
            <button className="link-button more" onClick={() => setAll(true)}>{t("显示全部 {count} 项", { count: children.length })}</button>
          </span>
        </div>
      )}
    </>
  );
}

function MemberDetail({ view, row }: { view: MeshView; row: MeshMemberRow }) {
  return (
    <Card title={row.name}>
      <Row label={t("状态")} value={row.state === "unreachable" ? t("不可达") : connectionLabel(row.connection, row.hops)} />
      {row.domain && <Row label={t("内网域名")} value={<span className="mono">{row.domain}</span>}><CopyButton label={t("拷贝")} text={row.domain} /></Row>}
      {row.address && <Row label={t("虚拟地址")} value={<span className="mono">{row.address}</span>}><CopyButton label={t("拷贝")} text={row.address} /></Row>}
      {row.region && <Row label={t("地区")} value={regionName(view.head, row.region)} />}
      {roleLabel(row.role) && <Row label={t("角色")} value={roleLabel(row.role)} />}
      {(row.marks ?? []).includes("duplicate") && <Row label={t("重名")} hint={t("本地区有同名成员。")} />}
      {(row.marks ?? []).includes("unmanaged") && <Row label={t("地址未受管")} hint={t("地址未受管：虚拟地址可能变化，内网域名可能随之失效。")} />}
    </Card>
  );
}

// Subnets.

function Subnets({ view, query, busy, setRoute }: {
  view: MeshView;
  query: string;
  busy: boolean;
  setRoute: (cidr: string, route: "local" | "mesh") => void;
}) {
  const subnets = view.subnets;
  if (subnets.rows.length === 0) return <p className="mesh-empty">{query === "" ? t("暂无子网") : t("无匹配的子网")}</p>;
  return (
    <>
      <div className="mesh-table subnets" role="table" aria-label={t("子网")}>
        <div className="mesh-row mesh-head" role="row">
          <span role="columnheader">{t("子网")}</span>
          <span role="columnheader">{t("通告者")}</span>
          <span role="columnheader">{t("地区")}</span>
          <span role="columnheader">{t("当前路径")}</span>
          <span role="columnheader" className="number">{t("延迟")}</span>
          <span role="columnheader">{t("状态")}</span>
        </div>
        {subnets.rows.map((row) => <SubnetRow key={row.cidr} row={row} busy={busy} setRoute={setRoute} />)}
      </div>
      {subnets.refused > 0 && <p className="mesh-empty">{t("已拒绝 {count} 项公网网段通告。", { count: subnets.refused })}</p>}
    </>
  );
}

function SubnetRow({ row, busy, setRoute }: { row: MeshSubnetRow; busy: boolean; setRoute: (cidr: string, route: "local" | "mesh") => void }) {
  const marks = row.marks ?? [];
  const advertisers = row.advertisers ?? [];
  return (
    <>
      <div className="mesh-row" role="row">
        <span role="cell" className="mesh-member">
          <StatusDot tone={row.state === "ok" ? "success" : "danger"} />
          <strong className="mono">{row.cidr}</strong>
          {marks.map((mark) => markLabel(mark) && <span key={mark} className="mesh-tag warning">{markLabel(mark)}</span>)}
        </span>
        <span role="cell">{row.advertiser ?? ""}</span>
        <span role="cell">{row.regionName ?? ""}</span>
        <span role="cell" className="truncate">{row.state === "refused" ? t("公网网段，未安装路由") : (row.path ?? []).join(" → ")}</span>
        <span role="cell" className="number">{row.state === "refused" ? "" : <Latency milliseconds={row.latencyMs} tone={row.tone} />}</span>
        <span role="cell">
          {marks.includes("local_overlap") ? (
            <Switch label={t("走组网")} checked={row.route === "mesh"} disabled={busy} onChange={() => setRoute(row.cidr, row.route === "mesh" ? "local" : "mesh")} />
          ) : row.state === "ok" ? t("正常") : row.state === "refused" ? t("已拒绝") : t("不可达")}
        </span>
      </div>
      {advertisers.length > 1 && advertisers.map((advertiser) => (
        <div key={advertiser.id} className="mesh-child" role="row">
          <span role="cell">{advertiser.name}</span>
          <span role="cell">{advertiser.inUse ? t("使用中") : t("备用")}</span>
        </div>
      ))}
      {(row.overlaps ?? []).length > 0 && (
        <div className="mesh-child warning" role="row">
          <span role="cell">{t("与 {networks} 重叠", { networks: (row.overlaps ?? []).join(t("、")) })}</span>
        </div>
      )}
    </>
  );
}

// Paths.

function Paths({ view }: { view: MeshView }) {
  const paths = view.paths;
  return (
    <>
      <Card title={t("默认选路")}>
        <Row label={t("路径策略")} value={paths.policy === "hops" ? t("跳数最低") : t("延迟最低")} />
      </Card>
      {paths.links && (
        <Card title={t("跨区链路")}>
          {paths.links.length === 0 && <Row label={t("暂无跨区链路")} />}
          {paths.links.map((link) => {
            const parts = [link.state === "detour" ? t("已自动切换，经{name}", { name: link.viaName ?? "" }) : link.state === "down" ? t("中断") : t("直连")];
            if ((link.borders ?? []).length > 0) parts.push((link.borders ?? []).join(t("、")));
            if (typeof link.loss === "number") parts.push(t("丢包 {value}%", { value: Math.round(link.loss * 100) }));
            return (
              <Row
                key={`${link.from}-${link.to}`}
                label={<><StatusDot tone={link.state === "down" ? "danger" : link.state === "detour" ? "warning" : "success"} /> {link.fromName} – {link.toName}</>}
                hint={parts.join(" · ")}
                value={<Latency milliseconds={link.latencyMs} tone={link.tone} />}
              />
            );
          })}
        </Card>
      )}
      {paths.custom && (
        <Card title={t("自定义路径")}>
          {paths.custom.length === 0 && <Row label={t("暂无自定义路径")} />}
          {paths.custom.map((path) => {
            const parts: string[] = [];
            if ((path.via ?? []).length > 0) parts.push(t("经 {name}", { name: (path.via ?? []).join(" → ") }));
            if (path.advertiser) parts.push(t("由 {name} 通告", { name: path.advertiser }));
            if (path.state === "fallback" && path.unreachable) parts.push(t("{name} 不可达", { name: path.unreachable }));
            const state = path.state === "fallback" ? t("已回落") : path.state === "first_hop_only" ? t("仅首跳生效") : t("生效");
            return <Row key={path.id} label={path.targetLabel} hint={parts.join(" · ")} value={state} />;
          })}
        </Card>
      )}
    </>
  );
}

// This device (desktop only): identity. Relay, host mode and advertising come with kernel 0.4.

function Self({ view }: { view: MeshView }) {
  const own = view.head.self;
  if (!own) return <p className="mesh-empty">{t("加入网络后显示本机身份。")}</p>;
  const region = regionName(view.head, own.region);
  const basis = own.regionBasis?.border;
  const regionText = !own.region
    ? ""
    : own.regionState === "pending"
      ? t("{name} · 待批准", { name: region })
      : own.regionState === "pinned"
        ? t("{name} · 已固定", { name: region })
        : basis
          ? t("{name} · 到 {border} 延迟最低", { name: region, border: basis })
          : region;
  return (
    <Card title={t("身份")}>
      <Row label={t("名称")} value={own.name} />
      {own.address && <Row label={t("虚拟地址")} value={<span className="mono">{own.address}</span>}><CopyButton label={t("拷贝")} text={own.address} /></Row>}
      {own.domain && <Row label={t("内网域名")} value={<span className="mono">{own.domain}</span>}><CopyButton label={t("拷贝")} text={own.domain} /></Row>}
      {regionText && <Row label={t("地区")} value={regionText} />}
    </Card>
  );
}

function Latency({ milliseconds, tone }: { milliseconds?: number | null; tone?: string | null }) {
  const dot = tone === "fast" ? "success" : tone === "medium" ? "warning" : tone === "slow" ? "danger" : "neutral";
  return <span className="mesh-latency"><StatusDot tone={dot} />{latencyText(milliseconds)}</span>;
}

// 添加网络.

function AddNetworkDialog({ taken, onClose, onJoin }: {
  taken: string[];
  onClose: () => void;
  onJoin: (config: Record<string, unknown>) => void;
}) {
  const [draft, setDraft] = useState<MeshDraft>(emptyDraft);
  const [shown, setShown] = useState(false);
  const problem = draftProblem(draft, taken);
  const set = (change: Partial<MeshDraft>) => setDraft({ ...draft, ...change });
  return (
    <ModalDialog onClose={onClose} className="mesh-dialog">
      <header className="terms-head">
        <div>
          <Heading slot="title" level={2}>{t("添加网络")}</Heading>
        </div>
      </header>
      <div className="mesh-form">
        <Field label={t("名称")}><input value={draft.name} onChange={(event) => set({ name: event.target.value })} /></Field>
        <Field label={t("密钥")}><input type="password" value={draft.secret} onChange={(event) => set({ secret: event.target.value })} /></Field>
        <Field label={t("对端地址")}>
          <textarea rows={3} placeholder="203.0.113.10:11010" value={draft.peers} onChange={(event) => set({ peers: event.target.value })} />
        </Field>
        <p className="field-hint">{t("每行一个地址，可带 tcp:// 或 udp://，默认端口 11010。")}</p>
        <Field label={t("虚拟地址")}><input placeholder={t("自动分配")} value={draft.address} onChange={(event) => set({ address: event.target.value })} /></Field>
        <Field label={t("监听端口")}><input inputMode="numeric" placeholder="11010" value={draft.port} onChange={(event) => set({ port: event.target.value })} /></Field>
        <Field label={t("加密模式")}>
          <Segmented<"forward" | "standard">
            label={t("加密模式")}
            value={draft.forwardSecrecy ? "forward" : "standard"}
            onChange={(value) => set({ forwardSecrecy: value === "forward" })}
            options={[
              { value: "forward", label: t("前向保密") },
              { value: "standard", label: t("标准") },
            ]}
          />
        </Field>
        {!draft.forwardSecrecy && <p className="field-hint">{t("标准模式未启用前向保密。")}</p>}
        {shown && problem && <p className="danger-text" role="alert">{draftProblemText(problem)}</p>}
      </div>
      <footer className="terms-actions">
        <button className="secondary" onClick={onClose}>{t("取消")}</button>
        <button
          className="primary"
          onClick={() => {
            const config = draftConfig(draft, taken);
            if (config) onJoin(config);
            else setShown(true);
          }}
        >
          {t("加入")}
        </button>
      </footer>
    </ModalDialog>
  );
}
