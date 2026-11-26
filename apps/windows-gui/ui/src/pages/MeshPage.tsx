import { ArrowDown, ArrowUp, Network, Plus, Search, TriangleAlert, X } from "lucide-react";
import { Heading } from "react-aria-components";
import { useCallback, useEffect, useState } from "react";
import { MoreMenu, ModalDialog, Select } from "../controls";
import { t } from "../i18n";
import { keepIfUnchanged, usePageVisible } from "../live";
import {
  advertisedCidr,
  connectionLabel,
  draftConfig,
  draftProblem,
  draftProblemText,
  emptyDraft,
  failureText,
  forwardingLabel,
  joinedState,
  joiningText,
  latencyText,
  listenText,
  markLabel,
  maxWaypoints,
  meshApi,
  noticeDetail,
  noticeText,
  pathFallbackText,
  pathStateText,
  regionName,
  roleLabel,
  routingPaths,
  securityLabel,
  securityTone,
  stateLabel,
  stateTone,
  type MemberQuery,
  type MeshDraft,
  type PathDraft,
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
      onCredential={(credential) => {
        setAdding(false);
        perform(() => meshApi.joinCredential(credential));
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
      {view.notices.map((notice) => {
        const showDetail = notice.key === "failure:join_failed" || notice.key.startsWith("failure:peer_not_native:");
        return <NoticeBar key={notice.key} text={noticeText(notice)} detail={showDetail ? noticeDetail(notice) ?? undefined : undefined} />;
      })}
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
      {tab === "paths" && <Paths view={view} network={networkId ?? ""} busy={busy} perform={perform} />}
      {tab === "self" && <Self view={view} network={networkId ?? ""} busy={busy} perform={perform} />}
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

function Paths({ view, network, busy, perform }: {
  view: MeshView;
  network: string;
  busy: boolean;
  perform: (task: () => Promise<MeshRefusal | null>) => void;
}) {
  const paths = view.paths;
  const [editing, setEditing] = useState(false);
  // What the paths were before the last removal, for 撤销 (T0).
  const [undo, setUndo] = useState<Record<string, unknown>[] | null>(null);
  const setPolicy = (policy: "latency" | "hops") => perform(() => meshApi.setPolicy(network, policy));
  const remove = (id: string) => {
    const before = routingPaths(view.status, {});
    const after = routingPaths(view.status, { remove: id });
    if (!after) return;
    setUndo(before);
    perform(() => meshApi.setPaths(network, after));
  };
  const cards = (
    <div className="mesh-cards">
      <Card title={t("默认选路")}>
        <Row label={t("路径策略")}>
          <Segmented<"latency" | "hops">
            label={t("路径策略")}
            value={paths.policy === "hops" ? "hops" : "latency"}
            onChange={(policy) => {
              if (policy !== paths.policy) setPolicy(policy);
            }}
            disabled={busy}
            options={[
              { value: "latency", label: t("延迟最低") },
              { value: "hops", label: t("跳数最低") },
            ]}
          />
        </Row>
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
        <Card
          title={t("自定义路径")}
          actions={<button className="link-button" disabled={busy || editing} onClick={() => setEditing(true)}><Plus size={14} />{t("添加路径")}</button>}
        >
          {undo && (
            <Row label={t("已移除路径。")}>
              <button className="secondary" disabled={busy} onClick={() => { const previous = undo; setUndo(null); perform(() => meshApi.setPaths(network, previous)); }}>{t("撤销")}</button>
            </Row>
          )}
          {paths.custom.length === 0 && <Row label={t("暂无自定义路径")} />}
          {paths.custom.map((path) => {
            const parts: string[] = [];
            if ((path.via ?? []).length > 0) parts.push(t("经 {name}", { name: (path.via ?? []).join(" → ") }));
            if (path.advertiser) parts.push(t("由 {name} 通告", { name: path.advertiser }));
            const fallback = pathFallbackText(path);
            if (fallback) parts.push(fallback);
            const state = pathStateText(path.state);
            return (
              <Row key={path.id} label={path.targetLabel} hint={parts.join(" · ")} value={state}>
                <button className="icon-button" aria-label={t("移除路径 {name}", { name: path.targetLabel })} disabled={busy} onClick={() => remove(path.id)}><X size={14} /></button>
              </Row>
            );
          })}
        </Card>
      )}
    </div>
  );
  if (!editing || !paths.custom) return cards;
  return (
    <div className="mesh-split">
      {cards}
      <aside className="mesh-detail" aria-label={t("添加路径")}>
        <PathEditor
          view={view}
          onCancel={() => setEditing(false)}
          onSave={(draft) => {
            const next = routingPaths(view.status, { add: draft });
            if (!next) return false;
            setEditing(false);
            setUndo(null);
            perform(() => meshApi.setPaths(network, next));
            return true;
          }}
        />
      </aside>
    </div>
  );
}

/** 添加路径 (desktop: a side column): target kind, target, waypoints in order, and the path it gives. */
function PathEditor({ view, onCancel, onSave }: {
  view: MeshView;
  onCancel: () => void;
  onSave: (draft: PathDraft) => boolean;
}) {
  const members = view.members.groups.flatMap((group) => group.rows).filter((row) => !row.self);
  const subnets = view.subnets.rows.filter((row) => row.state !== "refused");
  const [draft, setDraft] = useState<PathDraft>({ kind: "member", target: "", via: [] });
  const [problem, setProblem] = useState<string | null>(null);
  const name = (id: string) => members.find((row) => row.id === id)?.name ?? id;
  const targets = draft.kind === "member"
    ? members.map((row) => ({ value: row.id, label: row.name }))
    : subnets.map((row) => ({ value: row.cidr, label: row.cidr }));
  const advertiser = draft.kind === "subnet" ? subnets.find((row) => row.cidr === draft.target)?.advertiser : null;
  const waypoints = members.filter((row) => row.id !== draft.target).map((row) => ({ value: row.id, label: row.name }));
  const setVia = (via: string[]) => setDraft({ ...draft, via });
  const move = (index: number, by: number) => {
    const via = [...draft.via];
    const [item] = via.splice(index, 1);
    via.splice(index + by, 0, item);
    setVia(via);
  };
  const preview = [t("本机"), ...draft.via.map(name), draft.kind === "member" ? name(draft.target) : draft.target].filter(Boolean).join(" → ");
  return (
    <Card title={t("添加路径")}>
      <div className="mesh-form">
        <Field label={t("目标类型")}>
          <Segmented<"member" | "subnet">
            label={t("目标类型")}
            value={draft.kind}
            onChange={(kind) => setDraft({ kind, target: "", via: draft.via })}
            options={[
              { value: "member", label: t("成员") },
              { value: "subnet", label: t("子网") },
            ]}
          />
        </Field>
        <Field label={t("目标")}>
          <Select label={t("目标")} value={draft.target} placeholder={t("选择目标")} choices={targets} onChange={(target) => setDraft({ ...draft, target, via: draft.via.filter((id) => id !== target) })} />
        </Field>
        {advertiser && <p className="field-hint">{t("由 {name} 通告", { name: advertiser })}</p>}
        <Field label={t("途经点")}>
          <div className="mesh-waypoints">
            {draft.via.map((id, index) => (
              <div key={`${id}-${index}`} className="mesh-waypoint">
                <Select
                  label={t("途经点 {count}", { count: index + 1 })}
                  value={id}
                  choices={waypoints}
                  onChange={(next) => setVia(draft.via.map((item, at) => (at === index ? next : item)))}
                />
                <button className="icon-button" aria-label={t("上移")} disabled={index === 0} onClick={() => move(index, -1)}><ArrowUp size={14} /></button>
                <button className="icon-button" aria-label={t("下移")} disabled={index === draft.via.length - 1} onClick={() => move(index, 1)}><ArrowDown size={14} /></button>
                <button className="icon-button" aria-label={t("移除途经点")} onClick={() => setVia(draft.via.filter((_, at) => at !== index))}><X size={14} /></button>
              </div>
            ))}
            <button
              className="link-button"
              disabled={draft.via.length >= maxWaypoints || waypoints.length === 0}
              onClick={() => setVia([...draft.via, waypoints[0]?.value ?? ""])}
            >
              <Plus size={14} />{t("添加途经点")}
            </button>
          </div>
        </Field>
        <Row label={t("预览")} value={<span className="truncate">{preview}</span>} />
        {problem && <p className="danger-text" role="alert">{problem}</p>}
      </div>
      <footer className="terms-actions">
        <button className="secondary" onClick={onCancel}>{t("取消")}</button>
        <button
          className="primary"
          onClick={() => {
            if (draft.target === "") setProblem(t("选择目标。"));
            else if (draft.via.length === 0) setProblem(t("至少选择一个途经点。"));
            else if (!onSave(draft)) setProblem(t("路径中的成员缺少虚拟地址。"));
          }}
        >
          {t("保存")}
        </button>
      </footer>
    </Card>
  );
}

// This device (desktop only, docs/mesh-ui.md, 本机): identity with the region change request, the node's roles, the
// subnets it advertises with the local networks it found as suggestions, and how it runs. The roles and subnets come
// with kernel 0.4: without them in the snapshot those cards are left out.

function Self({ view, network, busy, perform }: {
  view: MeshView;
  network: string;
  busy: boolean;
  perform: (task: () => Promise<MeshRefusal | null>) => void;
}) {
  const own = view.head.self;
  const [region, setRegion] = useState("");
  if (!own) return <p className="mesh-empty">{t("加入网络后显示本机身份。")}</p>;
  const current = regionName(view.head, own.region);
  const basis = own.regionBasis?.border;
  const regionText = !own.region
    ? ""
    : own.regionState === "pending"
      ? t("{name} · 待批准", { name: current })
      : own.regionState === "pinned"
        ? t("{name} · 已固定", { name: current })
        : basis
          ? t("{name} · 到 {border} 延迟最低", { name: current, border: basis })
          : current;
  const others = (view.head.regions ?? []).filter((item) => item.code !== own.region);
  const chosen = others.some((item) => item.code === region) ? region : others[0]?.code ?? "";
  const advertised = (own.advertised ?? []).map(advertisedCidr);
  const suggested = (own.suggestedSubnets ?? []).filter((item) => !advertised.includes(item.cidr));
  const roles = typeof own.relay === "boolean" || typeof own.hostMode === "boolean";
  const subnets = own.advertised !== undefined || own.suggestedSubnets !== undefined;
  const forwarding = forwardingLabel(own.ipForwarding);
  const listen = listenText(own.listen);
  return (
    <>
      <Card title={t("身份")}>
        <Row label={t("名称")} value={own.name} />
        {own.address && <Row label={t("虚拟地址")} value={<span className="mono">{own.address}</span>}><CopyButton label={t("拷贝")} text={own.address} /></Row>}
        {own.domain && <Row label={t("内网域名")} value={<span className="mono">{own.domain}</span>}><CopyButton label={t("拷贝")} text={own.domain} /></Row>}
        {regionText && <Row label={t("地区")} value={regionText} />}
        {own.region && others.length > 0 && own.regionState !== "pending" && (
          <Row label={t("变更地区")} hint={t("变更需网络管理者批准。")}>
            <Select label={t("地区")} value={chosen} onChange={setRegion} choices={others.map((item) => ({ value: item.code, label: item.name }))} disabled={busy} />
            <button className="secondary" disabled={busy || !chosen} onClick={() => perform(() => meshApi.requestRegion(network, chosen))}>{t("申请变更")}</button>
          </Row>
        )}
      </Card>
      {roles && (
        <Card title={t("角色")}>
          {typeof own.relay === "boolean" && (
            <Row label={t("为其他成员中转")}>
              <Switch label={t("为其他成员中转")} checked={own.relay} disabled={busy} onChange={() => perform(() => meshApi.setRelay(network, !own.relay))} />
            </Row>
          )}
          {typeof own.hostMode === "boolean" && (
            <Row label={t("宿主模式")} hint={t("本机上的虚拟机经本机加入网络。")}>
              <Switch label={t("宿主模式")} checked={own.hostMode} disabled={busy} onChange={() => perform(() => meshApi.setHostMode(network, !own.hostMode))} />
            </Row>
          )}
        </Card>
      )}
      {subnets && (
        <Card title={t("通告子网")}>
          {advertised.length === 0 && suggested.length === 0 && <Row label={t("暂无通告子网")} />}
          {advertised.map((cidr) => (
            <Row key={cidr} label={<span className="mono">{cidr}</span>} hint={t("已通告")}>
              <button
                className="secondary"
                disabled={busy}
                onClick={() => perform(() => meshApi.withdraw(network, cidr))}
              >
                {t("停止通告")}
              </button>
            </Row>
          ))}
          {suggested.map((item) => (
            <Row
              key={item.cidr}
              label={<span className="mono">{item.cidr}</span>}
              hint={item.public ? t("公网网段，不可通告") : item.interface ? t("建议 · {name}", { name: item.interface }) : t("建议")}
            >
              {!item.public && (
                <button className="secondary" disabled={busy} onClick={() => perform(() => meshApi.advertise(network, item.cidr))}>{t("通告")}</button>
              )}
            </Row>
          ))}
        </Card>
      )}
      {(listen || forwarding) && (
        <Card title={t("运行")}>
          {listen && <Row label={t("监听端口")} value={<span className="mono">{listen}</span>} />}
          {forwarding && <Row label={t("系统 IP 转发")} value={forwarding} />}
        </Card>
      )}
    </>
  );
}

function Latency({ milliseconds, tone }: { milliseconds?: number | null; tone?: string | null }) {
  const dot = tone === "fast" ? "success" : tone === "medium" ? "warning" : tone === "slow" ? "danger" : "neutral";
  return <span className="mesh-latency"><StatusDot tone={dot} />{latencyText(milliseconds)}</span>;
}

// 添加网络.

function AddNetworkDialog({ taken, onClose, onJoin, onCredential }: {
  taken: string[];
  onClose: () => void;
  onJoin: (config: Record<string, unknown>) => void;
  /** 入网凭据 (kernel 0.5): a one-time credential from the panel joins this computer without typing the network. */
  onCredential: (credential: string) => void;
}) {
  const [method, setMethod] = useState<"manual" | "credential">("manual");
  const [credential, setCredential] = useState("");
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
        <Segmented<"manual" | "credential">
          label={t("加入方式")}
          value={method}
          onChange={(next) => {
            setMethod(next);
            setShown(false);
          }}
          options={[
            { value: "manual", label: t("手动填写") },
            { value: "credential", label: t("入网凭据") },
          ]}
        />
      </div>
      {method === "credential" ? (
        <div className="mesh-form">
          <Field label={t("入网凭据")}>
            <textarea rows={4} value={credential} onChange={(event) => setCredential(event.target.value)} />
          </Field>
          <p className="field-hint">{t("入网凭据由面板生成，仅可使用一次。")}</p>
          {shown && credential.trim() === "" && <p className="danger-text" role="alert">{t("填写入网凭据。")}</p>}
        </div>
      ) : (
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
      )}
      <footer className="terms-actions">
        <button className="secondary" onClick={onClose}>{t("取消")}</button>
        <button
          className="primary"
          onClick={() => {
            if (method === "credential") {
              if (credential.trim() !== "") onCredential(credential);
              else setShown(true);
              return;
            }
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
