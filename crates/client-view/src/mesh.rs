//! The mesh page (组网): what its entry, member list, subnet list, path view and notices make of the
//! kernel's status snapshot.
//!
//! The snapshot's fields are described in `docs/architecture/mesh-client.md`; the page itself in
//! `docs/mesh-ui.md`. Fields a kernel version does not give yet are simply absent, and every function
//! here degrades with them: without `regions` the member list is not grouped by region, without `links`
//! the path view has no cross-region section, without `paths` no custom paths.
//!
//! Nothing here is wording. Answers carry ids (`state`, `connection`, `marks`, notice ids) and values,
//! and each window turns them into its own sentences.

use std::cmp::Ordering;
use std::net::IpAddr;

use serde_json::{json, Map, Value};

use crate::capture::compare_text;

/// Children a parent row shows before "show all" (N31).
pub const FOLDED_CHILDREN: usize = 5;

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn opt_text<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

fn list<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn latency(value: &Value) -> Option<u64> {
    value.get("latencyMs").and_then(Value::as_u64)
}

/// `fast` under 120 ms, `medium` under 250 ms, `slow` from there, `timeout` when there is no answer. The
/// thresholds are the node list's (`mesh-ui.md`, 成员).
pub fn latency_tone(latency_ms: Option<u64>) -> &'static str {
    match latency_ms {
        None => "timeout",
        Some(ms) if ms < 120 => "fast",
        Some(ms) if ms < 250 => "medium",
        Some(_) => "slow",
    }
}

/// A network in CIDR form, or a bare address as a one-address network.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cidr {
    address: IpAddr,
    prefix: u8,
}

impl Cidr {
    pub fn parse(text: &str) -> Option<Cidr> {
        let text = text.trim();
        let (address, prefix) = match text.split_once('/') {
            Some((address, prefix)) => (address, Some(prefix)),
            None => (text, None),
        };
        let address: IpAddr = address.parse().ok()?;
        let width = if address.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(prefix) => prefix
                .parse::<u8>()
                .ok()
                .filter(|prefix| *prefix <= width)?,
            None => width,
        };
        Some(Cidr { address, prefix })
    }

    /// Whether `address` lies inside this network.
    pub fn contains(&self, address: IpAddr) -> bool {
        match (self.address, address) {
            (IpAddr::V4(net), IpAddr::V4(other)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(net) & mask == u32::from(other) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(other)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(net) & mask == u128::from(other) & mask
            }
            _ => false,
        }
    }
}

/// The address a query names, when it is one.
fn query_address(query: &str) -> Option<IpAddr> {
    query.trim().parse().ok()
}

/// The address part of `10.144.144.5/24`.
fn bare_address(address: &str) -> &str {
    address.split('/').next().unwrap_or_default()
}

/// What follows the host name in an internal domain name: `eu.mesh.internal` of
/// `par-host.eu.mesh.internal`. The phone row's second line starts with it.
fn region_domain(domain: &str) -> Option<&str> {
    domain
        .split_once('.')
        .map(|(_, rest)| rest)
        .filter(|rest| !rest.is_empty())
}

/// The order roles are listed in when the list is grouped by role.
const ROLES: [&str; 5] = ["border", "gateway", "host", "server", "client"];

fn role_rank(role: &str) -> usize {
    ROLES
        .iter()
        .position(|known| *known == role)
        .unwrap_or(ROLES.len())
}

/// The snapshot's members with this device first, marked `self`.
fn all_members(status: &Value) -> Vec<Value> {
    let mut members = Vec::new();
    if let Some(own) = status.get("self").filter(|own| own.is_object()) {
        let mut own = own.clone();
        if let Some(object) = own.as_object_mut() {
            object.insert("self".into(), json!(true));
            object.insert("state".into(), json!("online"));
            object.insert("connection".into(), json!("self"));
        }
        members.push(own);
    }
    members.extend(list(status, "members").iter().cloned());
    members
}

fn member_name<'a>(members: &'a [Value], id: &'a str) -> &'a str {
    members
        .iter()
        .find(|member| text(member, "id") == id)
        .map_or(id, |member| {
            opt_text(member, "name").unwrap_or_else(|| text(member, "id"))
        })
}

fn region_name<'a>(status: &'a Value, code: &'a str) -> &'a str {
    list(status, "regions")
        .iter()
        .find(|region| text(region, "code") == code)
        .and_then(|region| opt_text(region, "name"))
        .unwrap_or(code)
}

fn local_region(status: &Value) -> Option<&str> {
    list(status, "regions")
        .iter()
        .find(|region| region.get("local").and_then(Value::as_bool) == Some(true))
        .map(|region| text(region, "code"))
        .or_else(|| status.get("self").and_then(|own| opt_text(own, "region")))
}

/// What a member row unfolds into: a host's virtual machines first, then a gateway's or host's subnets and
/// the devices named inside them (`mesh-ui.md`, 宿主组). `devices` are the ones named on this device:
/// `[{ "name", "address" }]`.
fn children(member: &Value, devices: &[Value]) -> Vec<Value> {
    let mut children = Vec::new();
    for vm in list(member, "vms") {
        children.push(json!({
            "kind": "vm",
            "name": text(vm, "name"),
            "address": vm.get("address").cloned().unwrap_or(Value::Null),
            "domain": vm.get("domain").cloned().unwrap_or(Value::Null),
        }));
    }
    let subnets: Vec<&str> = list(member, "subnets")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for subnet in &subnets {
        children.push(json!({ "kind": "subnet", "name": subnet }));
    }
    if !subnets.is_empty() {
        let networks: Vec<Cidr> = subnets
            .iter()
            .filter_map(|subnet| Cidr::parse(subnet))
            .collect();
        for device in devices {
            let inside = bare_address(text(device, "address"))
                .parse::<IpAddr>()
                .is_ok_and(|address| networks.iter().any(|network| network.contains(address)));
            if inside {
                children.push(json!({
                    "kind": "device",
                    "name": text(device, "name"),
                    "address": text(device, "address"),
                }));
            }
        }
    }
    children
}

fn child_matches(child: &Value, query: &str, address: Option<IpAddr>) -> bool {
    let inside = match (address, text(child, "kind")) {
        (Some(address), "subnet") => {
            Cidr::parse(text(child, "name")).is_some_and(|network| network.contains(address))
        }
        _ => false,
    };
    if inside {
        return true;
    }
    ["name", "address", "domain"]
        .iter()
        .any(|field| text(child, field).to_lowercase().contains(query))
}

fn member_matches(member: &Value, query: &str) -> bool {
    ["name", "domain", "address", "addressV6"]
        .iter()
        .any(|field| text(member, field).to_lowercase().contains(query))
}

/// Whether a member passes the list's filter.
fn passes(member: &Value, filter: &str) -> bool {
    match filter {
        "online" => text(member, "state") == "online",
        "unreachable" => text(member, "state") == "unreachable",
        "gateway" | "host" | "border" => text(member, "role") == filter,
        "custom_path" => matches!(
            text(member, "pathState"),
            "custom" | "fallback" | "first_hop_only"
        ),
        _ => true,
    }
}

/// The pills a row carries besides its role: `duplicate` (N34 rows sharing a host name in one region),
/// `unmanaged` (an infrastructure member whose address is not fixed), `external_bridge` (it imports
/// addresses of an external network), `fallback` (its custom path fell back), `first_hop_only`.
fn marks(member: &Value) -> Vec<&'static str> {
    let mut marks = Vec::new();
    if !list(member, "duplicateOf").is_empty() {
        marks.push("duplicate");
    }
    let infrastructure = matches!(
        text(member, "role"),
        "border" | "gateway" | "host" | "server"
    );
    if infrastructure && member.get("addressManaged").and_then(Value::as_bool) == Some(false) {
        marks.push("unmanaged");
    }
    if member.get("bridgesExternal").and_then(Value::as_bool) == Some(true) {
        marks.push("external_bridge");
    }
    match text(member, "pathState") {
        "fallback" => marks.push("fallback"),
        "first_hop_only" => marks.push("first_hop_only"),
        _ => {}
    }
    marks
}

fn compare_members(a: &Value, b: &Value, sort: &str) -> Ordering {
    let own = |member: &Value| member.get("self").and_then(Value::as_bool) != Some(true);
    own(a).cmp(&own(b)).then_with(|| match sort {
        "latency" => {
            let key = |member: &Value| latency(member).unwrap_or(u64::MAX);
            key(a).cmp(&key(b))
        }
        "address" => {
            let key = |member: &Value| bare_address(text(member, "address")).parse::<IpAddr>().ok();
            match (key(a), key(b)) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
        }
        _ => compare_text(text(a, "name"), text(b, "name")),
    })
}

/// One member row (N30, N31, N34).
fn member_row(
    member: &Value,
    devices: &[Value],
    query: &str,
    address: Option<IpAddr>,
) -> Option<Value> {
    let kids = children(member, devices);
    let own_match = query.is_empty() || member_matches(member, query);
    let matched: Vec<bool> = kids
        .iter()
        .map(|child| !query.is_empty() && child_matches(child, query, address))
        .collect();
    let child_match = matched.iter().any(|matched| *matched);
    if !own_match && !child_match {
        return None;
    }
    // A match in a child opens the row with the match visible (N32).
    let visible = match matched.iter().rposition(|matched| *matched) {
        Some(last) if last >= FOLDED_CHILDREN => kids.len(),
        _ => kids.len().min(FOLDED_CHILDREN),
    };
    let contains = match (text(member, "role"), kids.is_empty()) {
        (_, true) => Value::Null,
        ("host", false) => json!({ "noun": "vms", "count": list(member, "vms").len() }),
        _ => json!({ "noun": "subnets", "count": list(member, "subnets").len() }),
    };
    let children: Vec<Value> = kids
        .into_iter()
        .zip(&matched)
        .map(|(mut child, matched)| {
            if let Some(object) = child.as_object_mut() {
                object.insert("match".into(), json!(matched));
            }
            child
        })
        .collect();
    let domain = text(member, "domain");
    let tone = if text(member, "connection") == "self" {
        Value::Null
    } else {
        json!(latency_tone(latency(member)))
    };
    Some(json!({
        "id": text(member, "id"),
        "name": text(member, "name"),
        "self": member.get("self").and_then(Value::as_bool).unwrap_or(false),
        "role": opt_text(member, "role"),
        "region": opt_text(member, "region"),
        "state": text(member, "state"),
        "address": opt_text(member, "address"),
        "domain": opt_text(member, "domain"),
        "regionDomain": region_domain(domain),
        "connection": opt_text(member, "connection").unwrap_or("unreachable"),
        "hops": member.get("hops").and_then(Value::as_u64).unwrap_or(0),
        "transport": opt_text(member, "transport"),
        "latencyMs": latency(member),
        "tone": tone,
        "contains": contains,
        "children": children,
        "visibleChildren": visible,
        "open": child_match,
        "marks": marks(member),
    }))
}

/// The worst status among rows (N33): unreachable members first, then fallen-back paths.
fn worst(rows: &[&Value]) -> Value {
    let unreachable = rows
        .iter()
        .filter(|member| text(member, "state") == "unreachable")
        .count();
    if unreachable > 0 {
        return json!({ "state": "unreachable", "count": unreachable });
    }
    let fallback = rows
        .iter()
        .filter(|member| text(member, "pathState") == "fallback")
        .count();
    if fallback > 0 {
        return json!({ "state": "fallback", "count": fallback });
    }
    Value::Null
}

/// The kind of a subnet: `lan` (the default, and what an unknown kind is read as), `host_group`,
/// `external` or `summary`.
fn subnet_kind(subnet: &Value) -> &'static str {
    match text(subnet, "kind") {
        "host_group" => "host_group",
        "external" => "external",
        "summary" => "summary",
        _ => "lan",
    }
}

/// `{ id, name }` of a member, or `null` for an empty id.
fn named(members: &[Value], id: &str) -> Value {
    if id.is_empty() {
        Value::Null
    } else {
        json!({ "id": id, "name": member_name(members, id) })
    }
}

/// The member a subnet is in use through: its `inUse` advertiser.
fn in_use_member(subnet: &Value) -> Option<&str> {
    list(subnet, "advertisers")
        .iter()
        .find(|advertiser| advertiser.get("inUse").and_then(Value::as_bool) == Some(true))
        .and_then(|advertiser| opt_text(advertiser, "member"))
}

/// A host group's gateway and backup, `null` for other kinds. The gateway shown is the member that
/// advertises the range now (the backup gateway advertises only while the main one is offline), the
/// configured one when none does.
fn host_group(members: &[Value], subnet: &Value) -> Value {
    if subnet_kind(subnet) != "host_group" {
        return Value::Null;
    }
    let config = subnet.get("hostGroup").unwrap_or(&Value::Null);
    let gateway = in_use_member(subnet).or_else(|| opt_text(config, "gateway"));
    json!({
        "gateway": gateway.map_or(Value::Null, |id| named(members, id)),
        "backup": opt_text(config, "backupGateway").map_or(Value::Null, |id| named(members, id)),
    })
}

/// The subnet as it is listed: an external one-address network without its prefix, others as given.
fn subnet_display(subnet: &Value) -> String {
    let cidr = text(subnet, "cidr");
    let single = Cidr::parse(cidr).is_some_and(|network| {
        let width = if network.address.is_ipv4() { 32 } else { 128 };
        network.prefix == width
    });
    if subnet_kind(subnet) == "external" && single && cidr.contains('/') {
        bare_address(cidr).to_owned()
    } else {
        cidr.to_owned()
    }
}

/// One subnet row; see `subnets`.
fn subnet_row(status: &Value, members: &[Value], subnet: &Value) -> Value {
    let cidr = text(subnet, "cidr");
    let kind = subnet_kind(subnet);
    let advertisers: Vec<Value> = list(subnet, "advertisers")
        .iter()
        .map(|advertiser| {
            let id = text(advertiser, "member");
            json!({
                "id": id,
                "name": member_name(members, id),
                "inUse": advertiser.get("inUse").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect();
    let in_use = advertisers
        .iter()
        .find(|advertiser| advertiser["inUse"] == json!(true))
        .or_else(|| advertisers.first())
        .map(|advertiser| advertiser["name"].clone())
        .unwrap_or(Value::Null);
    let state = opt_text(subnet, "state").unwrap_or("ok");
    let route = opt_text(subnet, "route").unwrap_or("mesh");
    let overlaps = list(subnet, "overlaps").to_vec();
    let mut marks = Vec::new();
    if advertisers.len() > 1 {
        marks.push("multi_source");
    }
    if !overlaps.is_empty() {
        marks.push("overlap");
    }
    if subnet.get("localOverlap").and_then(Value::as_bool) == Some(true) {
        marks.push("local_overlap");
    }
    if state == "refused" {
        marks.push("refused");
    }
    let path: Vec<&str> = list(subnet, "path")
        .iter()
        .filter_map(Value::as_str)
        .map(|id| member_name(members, id))
        .collect();
    let region = opt_text(subnet, "region");
    let tone = if state == "refused" {
        Value::Null
    } else {
        json!(latency_tone(latency(subnet)))
    };
    let bridge = if kind == "external" {
        opt_text(subnet, "bridge").map_or(Value::Null, |id| named(members, id))
    } else {
        Value::Null
    };
    // The virtual machines of a host group's range, from the members that list them.
    let vms: Vec<Value> = if kind == "host_group" {
        members
            .iter()
            .flat_map(|member| list(member, "vms"))
            .filter(|vm| text(vm, "cidr") == cidr)
            .map(|vm| {
                json!({
                    "name": text(vm, "name"),
                    "domain": vm.get("domain").cloned().unwrap_or(Value::Null),
                    "address": vm.get("address").cloned().unwrap_or(Value::Null),
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    json!({
        "cidr": cidr,
        "display": subnet_display(subnet),
        "kind": kind,
        "label": opt_text(subnet, "name"),
        "hostGroup": host_group(members, subnet),
        "bridge": bridge,
        "vms": vms,
        "region": region,
        "regionName": region.map(|code| region_name(status, code)),
        "advertiser": in_use,
        "advertisers": advertisers,
        "path": path,
        "latencyMs": latency(subnet),
        "tone": tone,
        "state": state,
        "route": route,
        "overlaps": overlaps,
        "marks": marks,
    })
}

/// Whether a subnet row matches a lower-cased query: the subnet, its panel name, an advertiser's name,
/// or an address inside it.
fn subnet_matches(row: &Value, query: &str, address: Option<IpAddr>) -> bool {
    let cidr = text(row, "cidr");
    query.is_empty()
        || cidr.to_lowercase().contains(query)
        || text(row, "label").to_lowercase().contains(query)
        || address.is_some_and(|address| {
            Cidr::parse(cidr).is_some_and(|network| network.contains(address))
        })
        || list(row, "advertisers")
            .iter()
            .any(|advertiser| text(advertiser, "name").to_lowercase().contains(query))
}

/// The cross-region link between the local region and `code`, in either direction.
fn link_between<'a>(status: &'a Value, local: &str, code: &str) -> Option<&'a Value> {
    list(status, "links").iter().find(|link| {
        (text(link, "from") == local && text(link, "to") == code)
            || (text(link, "from") == code && text(link, "to") == local)
    })
}

/// What the member list and the notices know of another region (`mesh-ui.md`, 其他地区): `{ state,
/// exitBorder, via, viaName, latencyMs, loss, subnets }`, or `null` when the snapshot has no link to it,
/// no verdict on its reachability and no summary range of it.
///
/// With a link (only border nodes get them) the link gives the state (`ok`, `detour`, `down`), the exit
/// border (`localBorder`, else the first of `borders` in the local region, else the member a summary range
/// is in use through), the detour and the loss. Without one: `regions[].reachable` decides `ok` or `down`,
/// else the summary ranges do (`down` when all are unreachable); latency is the lowest of theirs.
fn region_summary(status: &Value, members: &[Value], local: &str, code: &str) -> Value {
    let link = link_between(status, local, code);
    let reachable = list(status, "regions")
        .iter()
        .find(|region| text(region, "code") == code)
        .and_then(|region| region.get("reachable").and_then(Value::as_bool));
    let ranges: Vec<&Value> = list(status, "subnets")
        .iter()
        .filter(|subnet| subnet_kind(subnet) == "summary" && text(subnet, "region") == code)
        .collect();
    if link.is_none() && reachable.is_none() && ranges.is_empty() {
        return Value::Null;
    }
    let state = match (link, reachable) {
        (Some(link), _) => json!(opt_text(link, "state").unwrap_or("ok")),
        (None, Some(true)) => json!("ok"),
        (None, Some(false)) => json!("down"),
        (None, None) => {
            if ranges
                .iter()
                .all(|range| text(range, "state") == "unreachable")
            {
                json!("down")
            } else {
                json!("ok")
            }
        }
    };
    let in_local_region = |id: &str| {
        members
            .iter()
            .any(|member| text(member, "id") == id && text(member, "region") == local)
    };
    let exit = link
        .and_then(|link| opt_text(link, "localBorder"))
        .or_else(|| {
            link.and_then(|link| {
                list(link, "borders")
                    .iter()
                    .filter_map(Value::as_str)
                    .find(|id| in_local_region(id))
            })
        })
        .or_else(|| ranges.iter().find_map(|range| in_use_member(range)));
    let via = link.and_then(|link| opt_text(link, "via"));
    let latency_ms = match link {
        Some(link) => latency(link),
        None => ranges.iter().filter_map(|range| latency(range)).min(),
    };
    json!({
        "state": state,
        "exitBorder": exit.map_or(Value::Null, |id| named(members, id)),
        "via": via,
        "viaName": via.map(|code| region_name(status, code)),
        "latencyMs": latency_ms,
        "loss": link.and_then(|link| link.get("loss").and_then(Value::as_f64)),
        "subnets": ranges
            .iter()
            .map(|range| subnet_row(status, members, range))
            .collect::<Vec<_>>(),
    })
}

/// The summaries of every region but the local one that has one, in the order of `regions`.
fn region_summaries(status: &Value, members: &[Value]) -> Vec<(String, Value)> {
    let Some(local) = local_region(status) else {
        return Vec::new();
    };
    list(status, "regions")
        .iter()
        .map(|region| text(region, "code"))
        .filter(|code| !code.is_empty() && *code != local)
        .map(|code| {
            (
                code.to_owned(),
                region_summary(status, members, local, code),
            )
        })
        .filter(|(_, summary)| !summary.is_null())
        .collect()
}

/// The member list, grouped (N28, N29):
///
/// - `groupBy`: `region` (the default; `none` when the snapshot has no regions), `role` or `none`.
/// - `filter`: `all`, `online`, `unreachable`, `gateway`, `host`, `border`, `custom_path`.
/// - `sort`: `name`, `latency` or `address`; this device stays first.
/// - `query`: name, internal domain name, virtual address, subnet, device or virtual machine name. An
///   address inside an advertised subnet finds its gateway.
/// - `devices`: the subnet devices named on this device.
///
/// Answer: `{ groupBy, groups: [{ key, count, matched, open, worst, link, summary, rows }], total,
/// matched }`. `key` is a region code, a role, or `null` for the single group of `none`. `link` is the
/// cross-region link from the local region, for the folded summary. `summary` (region groups of other
/// regions only, else `null`) is `region_summary`: other regions are listed from their link and summary
/// ranges even when the snapshot names no member in them, and their rows are only the members the
/// snapshot does list. A group opens unfolded when it is the local one, when it needs attention (an
/// unreachable member, a region that is `down`), or when a query or filter is in force.
pub fn members(
    status: &Value,
    group_by: &str,
    filter: &str,
    sort: &str,
    query: &str,
    devices: &[Value],
) -> Value {
    let query = query.trim().to_lowercase();
    let address = query_address(&query);
    let narrowed = !query.is_empty() || !matches!(filter, "" | "all");
    let regions = list(status, "regions");
    let group_by = match group_by {
        "role" => "role",
        "region" | "" if !regions.is_empty() => "region",
        _ => "none",
    };
    let local = local_region(status);

    let mut everyone = all_members(status);
    everyone.sort_by(|a, b| compare_members(a, b, sort));
    let summaries = if group_by == "region" {
        region_summaries(status, &everyone)
    } else {
        Vec::new()
    };

    let key_of = |member: &Value| -> Option<String> {
        match group_by {
            "region" => Some(text(member, "region").to_owned()),
            "role" => Some(text(member, "role").to_owned()),
            _ => None,
        }
    };
    let mut keys: Vec<Option<String>> = Vec::new();
    for member in &everyone {
        let key = key_of(member);
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    for (code, _) in &summaries {
        let key = Some(code.clone());
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    match group_by {
        "region" => {
            let rank = |key: &Option<String>| {
                let code = key.as_deref().unwrap_or_default();
                let local_first = Some(code) != local;
                let at = regions
                    .iter()
                    .position(|region| text(region, "code") == code)
                    .unwrap_or(regions.len());
                (local_first, at)
            };
            keys.sort_by_key(rank);
        }
        "role" => keys.sort_by_key(|key| role_rank(key.as_deref().unwrap_or_default())),
        _ => {}
    }

    let mut matched_total = 0;
    let groups: Vec<Value> = keys
        .into_iter()
        .filter_map(|key| {
            let in_group: Vec<&Value> = everyone
                .iter()
                .filter(|member| key_of(member) == key)
                .collect();
            let rows: Vec<Value> = in_group
                .iter()
                .filter(|member| passes(member, filter))
                .filter_map(|member| member_row(member, devices, &query, address))
                .collect();
            let summary = summaries
                .iter()
                .find(|(code, _)| Some(code.as_str()) == key.as_deref())
                .map_or(Value::Null, |(_, summary)| summary.clone());
            // A region without listed members stays under a query that finds one of its ranges.
            let range_found = !query.is_empty()
                && matches!(filter, "" | "all")
                && list(&summary, "subnets")
                    .iter()
                    .any(|row| subnet_matches(row, &query, address));
            if narrowed && rows.is_empty() && !range_found {
                return None;
            }
            matched_total += rows.len();
            let attention = worst(&in_group);
            let is_local = group_by == "region" && key.as_deref() == local;
            let link = match (group_by, key.as_deref(), local) {
                ("region", Some(code), Some(local)) if code != local => {
                    link_between(status, local, code)
                        .map(|link| {
                            json!({
                                "latencyMs": latency(link),
                                "via": link.get("via").cloned().unwrap_or(Value::Null),
                                "state": text(link, "state"),
                            })
                        })
                        .unwrap_or(Value::Null)
                }
                _ => Value::Null,
            };
            let down = text(&summary, "state") == "down";
            let open = group_by == "none" || is_local || !attention.is_null() || down || narrowed;
            Some(json!({
                "key": key,
                "count": in_group.len(),
                "matched": rows.len(),
                "open": open,
                "worst": attention,
                "link": link,
                "summary": summary,
                "rows": rows,
            }))
        })
        .collect();
    json!({
        "groupBy": group_by,
        "groups": groups,
        "total": everyone.len(),
        "matched": matched_total,
    })
}

/// The subnet list. Each row: `{ cidr, display, kind, label, hostGroup, bridge, vms, region, regionName,
/// advertiser, advertisers: [{ id, name, inUse }], path: [names], latencyMs, tone, state, route,
/// overlaps, marks }`.
///
/// - `kind` is `lan`, `host_group`, `external` or `summary` (`mesh-ui.md`, 子网类型); `display` is the
///   address alone for an external one-address network, else `cidr`; `label` is the panel's name for it.
/// - `hostGroup` is `{ gateway, backup }` (each `{ id, name }` or `null`) for a host group, whose `vms`
///   list its virtual machines; `bridge` is the `{ id, name }` an external network is imported through.
/// - `marks` are `multi_source`, `overlap`, `local_overlap` and `refused`; refused subnets come last.
///
/// `query` matches the subnet, its name, an advertiser's name, or an address inside the subnet.
pub fn subnets(status: &Value, query: &str) -> Value {
    let query = query.trim().to_lowercase();
    let address = query_address(&query);
    let members = all_members(status);
    let mut rows: Vec<Value> = list(status, "subnets")
        .iter()
        .map(|subnet| subnet_row(status, &members, subnet))
        .filter(|row| subnet_matches(row, &query, address))
        .collect();
    rows.sort_by_key(|row| row["state"] == json!("refused"));
    json!({ "rows": rows, "refused": status.get("refusedAdvertisements").and_then(Value::as_u64).unwrap_or(0) })
}

/// The region panel (`mesh-ui.md`, 地区): `{ code, name, basis, state, request, suggestion, targets,
/// canRequest, canWithdraw, backbone }`.
///
/// - `basis` is `{ border, latencyMs, kind }` (what the region was assigned by) or `null`; `state` is
///   `auto`, `pinned`, `pending` or `other` (any other kernel value; `auto` when absent).
/// - `request` is `{ id, region, regionName, state, reason, rejectReason }` (`state` `pending` or
///   `rejected`) or `null`. `suggestion` is `{ region, regionName, currentMedianMs, suggestedMedianMs }`,
///   `null` while a request is pending.
/// - `targets` lists the other regions with the median latency of the link to them, when there is one.
/// - `canRequest` is whether a change can be asked for now (the network is joined, the region is known);
///   `canWithdraw` whether there is a request to withdraw. A window also checks the kernel's feature list.
/// - `backbone` is the kernel's `{ state, linksUp, linksTotal, failure }` of a border node, else `null`.
pub fn region(status: &Value) -> Value {
    let own = status.get("self").unwrap_or(&Value::Null);
    let network = status.get("network").unwrap_or(&Value::Null);
    let local = local_region(status);
    let basis = own
        .get("regionBasis")
        .filter(|basis| basis.is_object())
        .map(|basis| {
            json!({
                "border": basis.get("border").cloned().unwrap_or(Value::Null),
                "latencyMs": latency(basis),
                "kind": basis.get("kind").cloned().unwrap_or(Value::Null),
            })
        });
    let state = match opt_text(own, "regionState") {
        None | Some("auto") => "auto",
        Some("pinned") => "pinned",
        Some("pending") => "pending",
        Some(_) => "other",
    };
    let request = own
        .get("regionRequest")
        .filter(|request| request.is_object())
        .map(|request| {
            json!({
                "id": request.get("id").cloned().unwrap_or(Value::Null),
                "region": text(request, "region"),
                "regionName": region_name(status, text(request, "region")),
                "state": text(request, "state"),
                "reason": opt_text(request, "reason"),
                "rejectReason": opt_text(request, "rejectReason"),
            })
        });
    let pending = request
        .as_ref()
        .is_some_and(|request| request["state"] == json!("pending"));
    let suggestion = own
        .get("regionSuggestion")
        .filter(|suggestion| suggestion.is_object() && !pending)
        .map(|suggestion| {
            let code = text(suggestion, "region");
            json!({
                "region": code,
                "regionName": region_name(status, code),
                "currentMedianMs": suggestion.get("currentMedianMs").and_then(Value::as_u64),
                "suggestedMedianMs": suggestion.get("suggestedMedianMs").and_then(Value::as_u64),
            })
        });
    let targets: Vec<Value> = list(status, "regions")
        .iter()
        .map(|region| text(region, "code"))
        .filter(|code| !code.is_empty() && Some(*code) != local)
        .map(|code| {
            let toward = local.and_then(|local| {
                list(status, "links")
                    .iter()
                    .find(|link| text(link, "from") == local && text(link, "to") == code)
            });
            json!({
                "code": code,
                "name": region_name(status, code),
                "latencyMs": toward.and_then(latency),
            })
        })
        .collect();
    let can_request = matches!(text(network, "state"), "joined" | "partial")
        && opt_text(own, "region").is_some()
        && !list(status, "regions").is_empty();
    json!({
        "code": local,
        "name": local.map(|code| region_name(status, code)),
        "basis": basis,
        "state": state,
        "canWithdraw": request.is_some(),
        "request": request,
        "suggestion": suggestion,
        "targets": targets,
        "canRequest": can_request,
        "backbone": own.get("backbone").filter(|backbone| backbone.is_object()),
    })
}

/// The path view (`mesh-ui.md`, 路径): `{ policy, links, custom }`. `links` is `null` when the kernel
/// reports none (a single region), `custom` is `null` before custom paths exist.
pub fn paths(status: &Value) -> Value {
    let members = all_members(status);
    let names = |ids: &[Value]| -> Vec<String> {
        ids.iter()
            .filter_map(Value::as_str)
            .map(|id| member_name(&members, id).to_owned())
            .collect()
    };
    let links = status.get("links").and_then(Value::as_array).map(|links| {
        links
            .iter()
            .map(|link| {
                let via = opt_text(link, "via");
                json!({
                    "from": text(link, "from"),
                    "fromName": region_name(status, text(link, "from")),
                    "to": text(link, "to"),
                    "toName": region_name(status, text(link, "to")),
                    "via": via,
                    "viaName": via.map(|code| region_name(status, code)),
                    "borders": names(list(link, "borders")),
                    "latencyMs": latency(link),
                    "tone": json!(latency_tone(latency(link))),
                    "loss": link.get("loss").and_then(Value::as_f64),
                    "state": opt_text(link, "state").unwrap_or("ok"),
                })
            })
            .collect::<Vec<_>>()
    });
    let custom = status.get("paths").and_then(Value::as_array).map(|paths| {
        paths
            .iter()
            .map(|path| {
                let target = path.get("target").unwrap_or(&Value::Null);
                let (kind, value, label, advertiser) = if let Some(peer) = opt_text(target, "peer")
                {
                    (
                        "member",
                        peer,
                        member_name(&members, peer).to_owned(),
                        Value::Null,
                    )
                } else {
                    let subnet = text(target, "subnet");
                    let advertiser = list(status, "subnets")
                        .iter()
                        .find(|known| text(known, "cidr") == subnet)
                        .and_then(|known| {
                            list(known, "advertisers")
                                .iter()
                                .find(|advertiser| {
                                    advertiser.get("inUse").and_then(Value::as_bool) == Some(true)
                                })
                                .or_else(|| list(known, "advertisers").first())
                        })
                        .map(|advertiser| json!(member_name(&members, text(advertiser, "member"))))
                        .unwrap_or(Value::Null);
                    ("subnet", subnet, subnet.to_owned(), advertiser)
                };
                let unreachable = opt_text(path, "unreachable");
                json!({
                    "id": text(path, "id"),
                    "targetKind": kind,
                    "target": value,
                    "targetLabel": label,
                    "advertiser": advertiser,
                    "via": names(list(path, "via")),
                    "state": opt_text(path, "state").unwrap_or("active"),
                    "reason": opt_text(path, "reason"),
                    "unreachable": unreachable.map(|id| member_name(&members, id)),
                })
            })
            .collect::<Vec<_>>()
    });
    let network = status.get("network").unwrap_or(&Value::Null);
    json!({
        "policy": opt_text(network, "routingPolicy").unwrap_or("latency"),
        "links": links,
        "custom": custom,
    })
}

/// The banners of the page, in the order of `mesh-ui.md` (提示): a join failure, fallen-back custom
/// paths, cross-region detours, unreachable members, then the region ones: a refused change request
/// (`mesh.notice.region_rejected`, the only one of them with `notify`), a suggested move
/// (`mesh.notice.region_suggestion`, action `request_region`, not while a request is pending), a region that
/// is down (`mesh.notice.link_down`) and an unreachable host group (`mesh.notice.host_group_unreachable`). Each: `{ id, key, values, action, notify }`.
/// `id` names the sentence; `values` fill it in order; `action` is the one button (`network_settings`,
/// `take_over`) or `null`; `notify` asks for one system notification, which a window sends once per `key`.
pub fn notices(status: &Value) -> Value {
    let members = all_members(status);
    let network = status.get("network").unwrap_or(&Value::Null);
    let mut notices = Vec::new();
    let mut push =
        |id: &str, key: String, values: Vec<Value>, action: Option<&str>, notify: bool| {
            notices.push(json!({
                "id": id,
                "key": key,
                "values": values,
                "action": action,
                "notify": notify,
            }));
        };
    // `network.failure` stands on its own (mesh-status.md): a network that runs carries
    // `unreachable_peers` or `peer_not_native` with its state unchanged, and a paused managed
    // network `native_keys_pending` while `not_joined`. A failed network without a code is
    // `other`.
    let failure = network
        .get("failure")
        .filter(|failure| !failure.is_null())
        .or_else(|| (text(network, "state") == "failed").then_some(&Value::Null));
    if let Some(failure) = failure {
        let code = opt_text(failure, "code").unwrap_or("other");
        let detail = failure.get("detail").cloned().unwrap_or(Value::Null);
        let (id, action) = match code {
            "unencrypted_refused" => ("mesh.notice.unencrypted_refused", Some("network_settings")),
            "secret_mismatch" => ("mesh.notice.secret_mismatch", None),
            "security_mismatch" => ("mesh.notice.security_mismatch", Some("network_settings")),
            "port_in_use" => ("mesh.notice.port_in_use", None),
            "tun_in_use" => ("mesh.notice.tun_in_use", None),
            "route_overlap" => ("mesh.notice.route_overlap", None),
            "address_in_use" => ("mesh.notice.address_in_use", None),
            "permission_denied" => ("mesh.notice.permission_denied", None),
            "identity_online" => ("mesh.notice.identity_online", None),
            // Not in the kernel's snapshot: the desktop client or the installer finds the existing
            // program and puts this code in before asking.
            "existing_program" => ("mesh.notice.existing_program", Some("take_over")),
            "unreachable_peers" => ("mesh.notice.unreachable_peers", None),
            // A managed network accepts only its own nodes' static keys (`network.native_keys`).
            // `detail` is the refused peer's node ID.
            "peer_not_native" => ("mesh.notice.peer_not_native", None),
            // Paused until the panel lists this device's key; `detail` (`empty`, `self_missing`)
            // is for the log, not the page.
            "native_keys_pending" => ("mesh.notice.native_keys_pending", None),
            _ => ("mesh.notice.join_failed", None),
        };
        // A refused peer is named by its node ID, with the member name where the snapshot lists that
        // ID (a refused peer may claim a member's ID, so the name alone would point at the member).
        // Each peer has a notice key of its own, so that one closed notice does not hide the next
        // peer's.
        let peer = (code == "peer_not_native")
            .then(|| detail.as_str())
            .flatten();
        let (key, values) = if let Some(peer) = peer {
            (
                format!("failure:{code}:{peer}"),
                vec![json!(match member_name(&members, peer) {
                    name if name == peer => peer.to_string(),
                    name => format!("{name} ({peer})"),
                })],
            )
        } else if detail.is_null() || code == "native_keys_pending" {
            (format!("failure:{code}"), vec![])
        } else {
            (format!("failure:{code}"), vec![detail])
        };
        push(id, key, values, action, false);
    }
    for path in list(status, "paths") {
        if text(path, "state") != "fallback" {
            continue;
        }
        let unreachable = text(path, "unreachable");
        let target = path.get("target").unwrap_or(&Value::Null);
        let target = opt_text(target, "subnet")
            .map(str::to_owned)
            .or_else(|| opt_text(target, "peer").map(|peer| member_name(&members, peer).to_owned()))
            .unwrap_or_default();
        // mesh-status.md `paths[].reason`: a node that does not forward on this path is not an
        // unreachable one; `unreachable` names it either way (for `unsupported`, maybe the target).
        let (id, values) = match text(path, "reason") {
            "unsupported" | "not_relay" => (
                "mesh.notice.path_unsupported",
                vec![json!(member_name(&members, unreachable)), json!(target)],
            ),
            "hop_limit" => ("mesh.notice.path_hop_limit", vec![json!(target)]),
            // Its forwarding was not confirmed within 60 s of `pending`.
            "pending_timeout" => (
                "mesh.notice.path_pending_timeout",
                vec![json!(member_name(&members, unreachable)), json!(target)],
            ),
            // Probes through it got no echo although the topology still reaches it.
            "no_echo" => (
                "mesh.notice.path_no_echo",
                vec![json!(member_name(&members, unreachable)), json!(target)],
            ),
            _ => (
                "mesh.notice.path_fallback",
                vec![json!(member_name(&members, unreachable)), json!(target)],
            ),
        };
        push(
            id,
            format!("fallback:{}:{unreachable}", text(path, "id")),
            values,
            None,
            true,
        );
    }
    for link in list(status, "links") {
        if text(link, "state") != "detour" {
            continue;
        }
        let reason = link.get("reason").unwrap_or(&Value::Null);
        let (id, why) = if let Some(loss) = reason.get("loss").and_then(Value::as_f64) {
            (
                "mesh.notice.detour_loss",
                json!((loss * 100.0).round() as u64),
            )
        } else {
            ("mesh.notice.detour_latency", json!(latency(reason)))
        };
        let (from, to, via) = (text(link, "from"), text(link, "to"), text(link, "via"));
        push(
            id,
            format!("detour:{from}:{to}:{via}"),
            vec![
                json!(region_name(status, from)),
                json!(region_name(status, to)),
                json!(region_name(status, via)),
                why,
            ],
            None,
            false,
        );
    }
    if text(network, "state") == "partial" {
        let names: Vec<Value> = members
            .iter()
            .filter(|member| text(member, "state") == "unreachable")
            .map(|member| json!(text(member, "name")))
            .collect();
        if !names.is_empty() {
            let count = names.len();
            push(
                "mesh.notice.partial",
                "partial".into(),
                vec![json!(count), Value::Array(names)],
                None,
                false,
            );
        }
    }
    // Region notices (`mesh-ui.md`, 提示): a refused change request, a suggested move, then another
    // region that cannot be reached.
    let own = status.get("self").unwrap_or(&Value::Null);
    let request = own
        .get("regionRequest")
        .filter(|request| request.is_object());
    if let Some(request) = request.filter(|request| text(request, "state") == "rejected") {
        let region = text(request, "region");
        let mut values = vec![json!(region_name(status, region))];
        if let Some(reason) = opt_text(request, "rejectReason") {
            values.push(json!(reason));
        }
        let which = opt_text(request, "id").unwrap_or(region);
        push(
            "mesh.notice.region_rejected",
            format!("region_rejected:{which}"),
            values,
            None,
            true,
        );
    }
    let pending = request.is_some_and(|request| text(request, "state") == "pending");
    if let Some(suggestion) = own
        .get("regionSuggestion")
        .filter(|suggestion| suggestion.is_object() && !pending)
    {
        let region = text(suggestion, "region");
        push(
            "mesh.notice.region_suggestion",
            format!("region_suggestion:{region}"),
            vec![json!(region_name(status, region))],
            Some("request_region"),
            false,
        );
    }
    for (code, summary) in region_summaries(status, &members) {
        if text(&summary, "state") == "down" {
            push(
                "mesh.notice.link_down",
                format!("link_down:{code}"),
                vec![json!(region_name(status, &code))],
                None,
                false,
            );
        }
    }
    for subnet in list(status, "subnets") {
        if subnet_kind(subnet) == "host_group" && text(subnet, "state") == "unreachable" {
            let group = host_group(&members, subnet);
            let cidr = text(subnet, "cidr");
            push(
                "mesh.notice.host_group_unreachable",
                format!("host_group:{cidr}"),
                vec![json!(cidr), group["gateway"]["name"].clone()],
                None,
                false,
            );
        }
    }
    Value::Array(notices)
}

/// The entry on the overview (phones) and in the sidebar, menu bar and tray (desktops): `{ available,
/// network, state, security, online, total, attention }`. `available` is `false` when the kernel has no
/// mesh; the entry is then not shown. `security` is `forward_secrecy`, `standard` (labelled
/// 「未启用前向保密」) or `none`. `online` and `total` count the other members. `attention` is whether a
/// notice stands, for the entry's status dot.
pub fn entry(status: &Value) -> Value {
    if status.is_null() || status.get("available").and_then(Value::as_bool) == Some(false) {
        return json!({ "available": false });
    }
    let network = status.get("network").unwrap_or(&Value::Null);
    if !network.is_object() {
        return json!({ "available": true, "network": null, "state": "not_joined" });
    }
    let others = list(status, "members");
    let online = others
        .iter()
        .filter(|member| text(member, "state") == "online")
        .count();
    let mut answer = Map::new();
    answer.insert("available".into(), json!(true));
    answer.insert(
        "network".into(),
        json!(opt_text(network, "name").or_else(|| opt_text(network, "id"))),
    );
    answer.insert(
        "state".into(),
        json!(opt_text(network, "state").unwrap_or("not_joined")),
    );
    answer.insert("security".into(), json!(opt_text(network, "security")));
    answer.insert("online".into(), json!(online));
    answer.insert("total".into(), json!(others.len()));
    // The suggestion, a refused request and an unreachable host group show where they belong, not on
    // the entry's dot.
    let attention = notices(status).as_array().is_some_and(|notices| {
        notices.iter().any(|notice| {
            !matches!(
                text(notice, "id"),
                "mesh.notice.region_rejected"
                    | "mesh.notice.region_suggestion"
                    | "mesh.notice.host_group_unreachable"
            )
        })
    });
    answer.insert("attention".into(), json!(attention));
    Value::Object(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_peer_is_named_while_the_network_keeps_running() {
        // mesh-status.md: `peer_not_native` leaves `network.state` as it is; `detail` is the
        // refused peer's node ID.
        let status = json!({
            "network": {
                "state": "joined",
                "failure": { "code": "peer_not_native", "detail": "node-7f3a" },
            },
        });
        let notices = notices(&status);
        assert_eq!(notices.as_array().map(Vec::len), Some(1));
        assert_eq!(notices[0]["id"], "mesh.notice.peer_not_native");
        assert_eq!(notices[0]["key"], "failure:peer_not_native:node-7f3a");
        assert_eq!(notices[0]["values"], json!(["node-7f3a"]));
        assert_eq!(entry(&status)["attention"], true);

        // A listed member is named; another refused peer is a notice of its own.
        let listed = json!({
            "network": {
                "state": "joined",
                "failure": { "code": "peer_not_native", "detail": "node-9b1c" },
            },
            "members": [{ "id": "node-9b1c", "name": "lon-host" }],
        });
        let named = super::notices(&listed);
        assert_eq!(named[0]["key"], "failure:peer_not_native:node-9b1c");
        assert_eq!(named[0]["values"], json!(["lon-host (node-9b1c)"]));
    }

    #[test]
    fn a_paused_managed_network_waits_for_its_key_without_naming_the_reason_code() {
        let status = json!({
            "network": {
                "state": "not_joined",
                "failure": { "code": "native_keys_pending", "detail": "self_missing" },
            },
        });
        let notices = notices(&status);
        assert_eq!(notices[0]["id"], "mesh.notice.native_keys_pending");
        assert_eq!(notices[0]["values"], json!([]));
    }

    #[test]
    fn a_path_that_fell_back_names_why() {
        // mesh-status.md: `pending` (capability check) is no notice; `reason` picks the sentence.
        let path = |state: &str, reason: Option<&str>| {
            json!({
                "id": "subnet:192.168.30.0/24",
                "target": { "subnet": "192.168.30.0/24" },
                "via": ["m-1"],
                "state": state,
                "reason": reason,
                "unreachable": "m-4",
            })
        };
        let status = |path: Value| {
            json!({
                "members": [{ "id": "m-4", "name": "tyo-border" }],
                "paths": [path],
            })
        };
        assert_eq!(notices(&status(path("pending", None))), json!([]));
        let unsupported = notices(&status(path("fallback", Some("unsupported"))));
        assert_eq!(unsupported[0]["id"], "mesh.notice.path_unsupported");
        assert_eq!(
            unsupported[0]["values"],
            json!(["tyo-border", "192.168.30.0/24"])
        );
        let not_relay = notices(&status(path("fallback", Some("not_relay"))));
        assert_eq!(not_relay[0]["id"], "mesh.notice.path_unsupported");
        let hops = notices(&status(path("fallback", Some("hop_limit"))));
        assert_eq!(hops[0]["id"], "mesh.notice.path_hop_limit");
        assert_eq!(hops[0]["values"], json!(["192.168.30.0/24"]));
        let silent = notices(&status(path("fallback", Some("no_echo"))));
        assert_eq!(silent[0]["id"], "mesh.notice.path_no_echo");
        assert_eq!(
            silent[0]["values"],
            json!(["tyo-border", "192.168.30.0/24"])
        );
        let late = notices(&status(path("fallback", Some("pending_timeout"))));
        assert_eq!(late[0]["id"], "mesh.notice.path_pending_timeout");
        assert_eq!(late[0]["values"], json!(["tyo-border", "192.168.30.0/24"]));
        // An unknown reason reads as unreachable.
        let unknown = notices(&status(path("fallback", Some("later_reason"))));
        assert_eq!(unknown[0]["id"], "mesh.notice.path_fallback");
        let gone = notices(&status(path("fallback", Some("unreachable"))));
        assert_eq!(gone[0]["id"], "mesh.notice.path_fallback");
        // An engine before `reason` still says unreachable.
        let older = notices(&status(path("fallback", None)));
        assert_eq!(older[0]["id"], "mesh.notice.path_fallback");
        let row = &paths(&status(path("fallback", Some("not_relay"))))["custom"][0];
        assert_eq!(row["reason"], "not_relay");
        assert_eq!(row["unreachable"], "tyo-border");
        assert_eq!(
            paths(&status(path("pending", None)))["custom"][0]["state"],
            "pending"
        );
    }

    #[test]
    fn a_network_without_a_failure_has_no_failure_notice() {
        let status = json!({ "network": { "state": "joined", "failure": null } });
        assert_eq!(notices(&status), json!([]));
        let failed = json!({ "network": { "state": "failed" } });
        assert_eq!(notices(&failed)[0]["id"], "mesh.notice.join_failed");
    }

    #[test]
    fn addresses_fall_inside_their_networks() {
        let net = Cidr::parse("192.168.20.0/24").unwrap();
        assert!(net.contains("192.168.20.9".parse().unwrap()));
        assert!(!net.contains("192.168.21.9".parse().unwrap()));
        assert!(!net.contains("fd00::1".parse().unwrap()));
        let all = Cidr::parse("10.0.0.0/0").unwrap();
        assert!(all.contains("10.1.2.3".parse().unwrap()));
        let v6 = Cidr::parse("fd00:aa::/64").unwrap();
        assert!(v6.contains("fd00:aa::5".parse().unwrap()));
        assert!(!v6.contains("fd00:ab::5".parse().unwrap()));
        assert_eq!(Cidr::parse("10.0.0.0/33"), None);
        assert_eq!(Cidr::parse("example"), None);
    }

    #[test]
    fn latency_tones_follow_the_node_list() {
        assert_eq!(latency_tone(Some(119)), "fast");
        assert_eq!(latency_tone(Some(120)), "medium");
        assert_eq!(latency_tone(Some(250)), "slow");
        assert_eq!(latency_tone(None), "timeout");
    }

    #[test]
    fn region_domain_drops_the_host_name() {
        assert_eq!(
            region_domain("par-host.eu.mesh.internal"),
            Some("eu.mesh.internal")
        );
        assert_eq!(region_domain("par-host"), None);
    }

    #[test]
    fn children_fold_after_five_unless_a_later_one_matches() {
        let subnets: Vec<String> = (1..=7).map(|n| format!("192.168.{n}.0/24")).collect();
        let gateway = json!({ "id": "g", "name": "gw", "role": "gateway", "state": "online", "subnets": subnets });
        let row = member_row(&gateway, &[], "", None).unwrap();
        assert_eq!(row["visibleChildren"], 5);
        assert_eq!(row["contains"], json!({ "noun": "subnets", "count": 7 }));
        let address = query_address("192.168.7.4");
        let row = member_row(&gateway, &[], "192.168.7.4", address).unwrap();
        assert_eq!(row["visibleChildren"], 7);
        assert_eq!(row["open"], true);
        assert_eq!(row["children"][6]["match"], true);
    }
}
