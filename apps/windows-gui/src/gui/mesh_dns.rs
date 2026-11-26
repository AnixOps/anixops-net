//! Internal domain names (内网域名) while the engine runs for the 组网 mesh alone (mesh-status.md `tunnel.dns`, R28):
//! the adapter for the mesh alone announces no DNS, so each suffix the engine names is sent to the mesh's DNS server
//! with a Name Resolution Policy Table rule, and every other name keeps the device's own DNS. Also:
//!
//! - the suffixes are held to a mesh's own namespace ([`internal_suffix`]): at least two labels under `internal`
//!   (`mesh.internal`), since a rule applies to every program and user of the device, and a suffix such as `com`
//!   would hand the device's lookups to whoever runs the network;
//! - the reverse zones the engine names among them (`*.in-addr.arpa`, `*.ip6.arpa`; the kernel derives them from the
//!   network's ranges and the private allowlist, core#375) go to the same server ([`reverse_zone`]), so that a mesh
//!   address reads back as its name; the client derives none itself;
//! - a bare host name resolves in the device's own region: the region's suffix (`eu.mesh.internal`) is the mesh
//!   adapter's connection suffix, which Windows appends to a single-label name;
//! - the DNS cache is cleared after each change, so that a failed lookup from before does not linger;
//! - rules a domain's group policy sets make Windows ignore the device's own, which is reported ([`Applied`]).
//!
//! The rules carry one fixed comment and only those are ever removed. They are machine-wide while each signed-in
//! user runs a window, so they are changed only while this process holds the adapter (`tun_mode::hold_adapter`):
//! by the engine for the mesh alone, which holds it while it runs, and by [`clear_leftovers`] for rules a crash
//! left behind, which takes the hold first and leaves the rules to whoever runs the mesh now when it cannot. The
//! uninstaller removes them too.
//!
//! Only one network is joined at a time (`client_core::backend::mesh::set_joined`), so every suffix goes to the
//! same servers; networks joined side by side would need each network's suffixes on its own servers.

use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};

use platform_windows::managed::append_managed_log;
use platform_windows::tun::WindowsTunSettings;
use serde_json::{json, Value};

/// Marks the rules this program adds, and only those.
const COMMENT: &str = "AnixOps Net mesh";

/// The label every internal suffix ends in (`mesh.internal`, R28).
const PARENT_LABEL: &str = "internal";

/// The adapter's fixed interface GUID (platform-windows `tun::ADAPTER_GUID`, which the proxy's adapter shares). Its
/// connection suffix is kept by GUID, also while no adapter exists: taken away by GUID too, it cannot pass from a
/// crashed engine for the mesh alone to the next adapter, the proxy's included.
const ADAPTER_GUID: &str = "{4E3C5B8A-7D21-4F0B-9A6E-1C2D3E4F5A6B}";

/// Removes this program's rules, then sets the adapter's connection suffix (or takes it away, by GUID too), adds one rule per
/// namespace, clears the DNS cache and answers whether a group policy sets the rules (`policy`) or the device's
/// own rules apply (`local`). The values come through the environment, never through the script's text.
const SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$comment = $env:ANIXOPS_MESH_DNS_COMMENT
Get-DnsClientNrptRule | Where-Object { $_.Comment -eq $comment } | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }
$adapters = if ($env:ANIXOPS_MESH_DNS_INDEX) { @(Get-DnsClient -InterfaceIndex ([int]$env:ANIXOPS_MESH_DNS_INDEX)) } else { @(Get-DnsClient -InterfaceAlias $env:ANIXOPS_MESH_DNS_ADAPTER -ErrorAction SilentlyContinue) }
foreach ($adapter in $adapters) {
  if ($env:ANIXOPS_MESH_DNS_SEARCH) {
    Set-DnsClient -InterfaceIndex $adapter.InterfaceIndex -ConnectionSpecificSuffix $env:ANIXOPS_MESH_DNS_SEARCH
  } else {
    Set-DnsClient -InterfaceIndex $adapter.InterfaceIndex -ResetConnectionSpecificSuffix
  }
}
if (-not $env:ANIXOPS_MESH_DNS_SEARCH) {
  $interface = "HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces\$env:ANIXOPS_MESH_DNS_GUID"
  if ((Test-Path $interface) -and (Get-ItemProperty -Path $interface -Name Domain -ErrorAction SilentlyContinue).Domain) {
    Set-ItemProperty -Path $interface -Name Domain -Value ''
  }
}
$servers = @($env:ANIXOPS_MESH_DNS_SERVERS -split ',' | Where-Object { $_ })
foreach ($namespace in @($env:ANIXOPS_MESH_DNS_NAMESPACES -split ',' | Where-Object { $_ })) {
  Add-DnsClientNrptRule -Namespace $namespace -NameServers $servers -Comment $comment | Out-Null
}
Clear-DnsClientCache
$policy = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\DNSClient\DnsPolicyConfig'
if ((Test-Path $policy) -and @(Get-ChildItem $policy).Count -gt 0) { 'policy' } else { 'local' }"#;

/// The rules for the internal domain names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// NRPT namespaces: each suffix (`.eu.mesh.internal`, which also covers its subdomains) and each reverse zone the
    /// engine names (`.144.144.10.in-addr.arpa`).
    pub namespaces: Vec<String>,
    /// The mesh's DNS servers, IPv4 first.
    pub servers: Vec<String>,
    /// The device's own region's suffix (`eu.mesh.internal`), for bare host names.
    pub search: Option<String>,
}

/// What the engine named and the rules leave out, for the log and the page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LeftOut {
    /// Names that are neither a mesh's suffix nor a reverse zone, as plain text.
    pub suffixes: Vec<String>,
}

/// The rules for the engine's `domains` (`tunnel.dns.matchDomains`: the mesh's suffixes and the reverse zones the
/// kernel derived) answered by `servers`, with the device's own name `own_domain` (`self.domain`) for bare host
/// names. `None` when no name or no server is left.
pub fn plan(
    domains: &[String],
    servers: &[String],
    own_domain: Option<&str>,
) -> (Option<Plan>, LeftOut) {
    let mut left_out = LeftOut::default();
    let mut suffixes: Vec<String> = Vec::new();
    let mut zones: Vec<String> = Vec::new();
    for domain in domains {
        if let Some(suffix) = internal_suffix(domain) {
            if !suffixes.contains(&suffix) {
                suffixes.push(suffix);
            }
        } else if let Some(zone) = reverse_zone(domain) {
            if !zones.contains(&zone) {
                zones.push(zone);
            }
        } else if !domain.trim().is_empty() {
            left_out.suffixes.push(printable(domain));
        }
    }
    let mut addresses: Vec<IpAddr> = Vec::new();
    for address in servers
        .iter()
        .filter_map(|server| server.trim().parse::<IpAddr>().ok())
    {
        if !addresses.contains(&address) {
            addresses.push(address);
        }
    }
    // An IPv4 server first: one reached over IPv6 alone fails on a device without IPv6.
    addresses.sort_by_key(IpAddr::is_ipv6);
    let servers: Vec<String> = addresses.iter().map(IpAddr::to_string).collect();
    if (suffixes.is_empty() && zones.is_empty()) || servers.is_empty() {
        return (None, left_out);
    }
    let search = own_domain
        .and_then(|domain| domain.split_once('.').map(|(_, region)| region))
        .and_then(plain_name)
        .filter(|region| {
            suffixes
                .iter()
                .any(|suffix| region == suffix || region.ends_with(&format!(".{suffix}")))
        });
    let namespaces = suffixes
        .iter()
        .chain(&zones)
        .map(|name| format!(".{name}"))
        .collect();
    (
        Some(Plan {
            namespaces,
            servers,
            search,
        }),
        left_out,
    )
}

/// The longest name a suffix may be (core#370, the panel's limit too).
const NAME_MAX: usize = 172;

/// `domain` as a plain lowercase DNS name (core#370): ASCII labels of letters, digits and hyphens (Punycode `xn--`
/// included), 1–63 bytes each, not starting or ending with a hyphen, at most [`NAME_MAX`] bytes in all; one trailing
/// dot is dropped. A leading dot, an empty label or a second trailing dot makes it no name.
fn plain_name(domain: &str) -> Option<String> {
    let domain = domain.trim();
    let domain = domain
        .strip_suffix('.')
        .unwrap_or(domain)
        .to_ascii_lowercase();
    let plain = !domain.is_empty()
        && domain.len() <= NAME_MAX
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        });
    plain.then_some(domain)
}

/// `domain` as a mesh's suffix: a plain name of at least two labels whose last is `internal` (`mesh.internal`,
/// `eu.mesh.internal`). A single label (`internal`, `com`, `local`), the root, a public suffix (`com`, `co.uk`) and
/// the reverse lookup tree (`arpa`) are never one.
pub fn internal_suffix(domain: &str) -> Option<String> {
    plain_name(domain).filter(|name| {
        name.rsplit_once('.')
            .is_some_and(|(_, last)| last == PARENT_LABEL)
    })
}

/// `domain` as a reverse zone the engine named (core#375): a plain name under `in-addr.arpa` with one to four
/// decimal labels of 0–255 (an IPv4 prefix of /8 or longer), or under `ip6.arpa` with two to 32 single hexadecimal
/// labels (an IPv6 prefix of /7 or longer, which the kernel spreads over the zones of /8). The trees themselves
/// (`in-addr.arpa`, `ip6.arpa`, `arpa`) and anything wider are never one.
pub fn reverse_zone(domain: &str) -> Option<String> {
    let name = plain_name(domain)?;
    let (labels, ipv4) = if let Some(labels) = name.strip_suffix(".in-addr.arpa") {
        (labels, true)
    } else if let Some(labels) = name.strip_suffix(".ip6.arpa") {
        (labels, false)
    } else {
        return None;
    };
    let labels: Vec<&str> = labels.split('.').collect();
    let fits = if ipv4 {
        (1..=4).contains(&labels.len())
            && labels.iter().all(|label| {
                label.len() <= 3
                    && label.bytes().all(|byte| byte.is_ascii_digit())
                    && (label.len() == 1 || !label.starts_with('0'))
                    && label.parse::<u16>().is_ok_and(|value| value <= 255)
            })
    } else {
        (2..=32).contains(&labels.len())
            && labels
                .iter()
                .all(|label| label.len() == 1 && label.bytes().all(|byte| byte.is_ascii_hexdigit()))
    };
    fits.then_some(name)
}

/// Text from the engine fit for the log and the page: printable ASCII, at most 64 characters.
fn printable(text: &str) -> String {
    text.trim()
        .chars()
        .filter(|character| character.is_ascii_graphic())
        .take(64)
        .collect()
}

/// Whether the rules' servers are all IPv6 and none can be reached from this device (IPv6 is off, or nothing routes
/// to them), so that the names cannot resolve although the rules are in place. `reachable` tells for one server.
pub fn ipv6_only_unreachable(plan: &Plan, reachable: impl Fn(IpAddr) -> bool) -> bool {
    let servers: Vec<IpAddr> = plan
        .servers
        .iter()
        .filter_map(|server| server.parse().ok())
        .collect();
    !servers.is_empty()
        && servers.iter().all(IpAddr::is_ipv6)
        && !servers.iter().any(|server| reachable(*server))
}

/// Whether this device can send to `server` on the DNS port: a socket of its family opens and a route leads there.
/// Nothing is sent.
pub fn reachable(server: IpAddr) -> bool {
    let local: std::net::SocketAddr = if server.is_ipv6() {
        (std::net::Ipv6Addr::UNSPECIFIED, 0).into()
    } else {
        (std::net::Ipv4Addr::UNSPECIFIED, 0).into()
    };
    std::net::UdpSocket::bind(local)
        .and_then(|socket| socket.connect((server, 53)))
        .is_ok()
}

/// What putting rules in place found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    /// A group policy sets the name resolution rules, so Windows ignores the device's own.
    pub policy_override: bool,
}

/// Puts `plan` in place of this program's earlier rules on the mesh adapter `interface` (its index; `None` finds it
/// by name); `None` only removes them. Called only while this process holds the adapter.
pub fn apply(plan: Option<&Plan>, interface: Option<u32>) -> Result<Applied, String> {
    let (namespaces, servers, search) = plan
        .map(|plan| {
            (
                plan.namespaces.join(","),
                plan.servers.join(","),
                plan.search.clone().unwrap_or_default(),
            )
        })
        .unwrap_or_default();
    let output = super::system_programs::powershell(SCRIPT)?
        .env("ANIXOPS_MESH_DNS_COMMENT", COMMENT)
        .env("ANIXOPS_MESH_DNS_NAMESPACES", namespaces)
        .env("ANIXOPS_MESH_DNS_SERVERS", servers)
        .env("ANIXOPS_MESH_DNS_SEARCH", search)
        .env(
            "ANIXOPS_MESH_DNS_INDEX",
            interface.map(|index| index.to_string()).unwrap_or_default(),
        )
        .env(
            "ANIXOPS_MESH_DNS_ADAPTER",
            WindowsTunSettings::default().adapter_name,
        )
        .env("ANIXOPS_MESH_DNS_GUID", ADAPTER_GUID)
        .output()
        .map_err(|error| format!("the name resolution rules could not be set: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "the name resolution rules could not be set: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(Applied {
        policy_override: String::from_utf8_lossy(&output.stdout).trim() == "policy",
    })
}

/// Set once the rules a crash left behind are gone.
static LEFTOVERS_CLEARED: AtomicBool = AtomicBool::new(false);

/// Removes the rules a crash left behind, once per process, and only while this process can hold the adapter: when
/// another signed-in user's window holds it, the rules are that window's, and this is tried again on the next call.
pub fn clear_leftovers() {
    clear_leftovers_with(
        &LEFTOVERS_CLEARED,
        || super::tun_mode::hold_adapter().ok(),
        || apply(None, None).map(|_| ()),
    );
}

/// [`clear_leftovers`] with the hold and the removal given: answers whether the rules were removed now.
fn clear_leftovers_with<Hold>(
    cleared: &AtomicBool,
    hold: impl FnOnce() -> Option<Hold>,
    remove: impl FnOnce() -> Result<(), String>,
) -> bool {
    if cleared.load(Ordering::Acquire) {
        return false;
    }
    let Some(held) = hold() else {
        return false;
    };
    let removed = match remove() {
        Ok(()) => {
            cleared.store(true, Ordering::Release);
            true
        }
        Err(error) => {
            let _ = append_managed_log("tauri-gui", &format!("mesh: {error}"));
            false
        }
    };
    drop(held);
    removed
}

/// The state of the internal domain names while the engine for the mesh alone runs, as the 组网 page reads it
/// (`network.local_dns`, Windows only): `[{code, detail}]` with `write_failed` (the rules could not be put in place;
/// tried again), `ipv6_unavailable` (the servers are IPv6 only and this device reaches none, [`ipv6_only_unreachable`]),
/// `policy_override` (a group policy's rules take their place) and `suffix_refused` (suffixes outside a mesh's
/// namespace; `detail` names them).
pub fn issues(
    failed: bool,
    ipv6_unavailable: bool,
    applied: Option<Applied>,
    left_out: &LeftOut,
) -> Vec<Value> {
    let mut issues = Vec::new();
    if failed {
        issues.push(json!({ "code": "write_failed", "detail": null }));
    }
    if ipv6_unavailable {
        issues.push(json!({ "code": "ipv6_unavailable", "detail": null }));
    }
    if applied.is_some_and(|applied| applied.policy_override) {
        issues.push(json!({ "code": "policy_override", "detail": null }));
    }
    if !left_out.suffixes.is_empty() {
        issues.push(json!({ "code": "suffix_refused", "detail": left_out.suffixes.join(", ") }));
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn each_internal_suffix_goes_to_the_mesh_dns_server() {
        let (rules, left_out) = super::plan(
            &strings(&["office.internal", "EU.Mesh.Internal.", "office.internal"]),
            &strings(&["fd00::53", "198.18.0.2", "198.18.0.2"]),
            None,
        );
        let rules = rules.unwrap();
        assert_eq!(rules.namespaces, [".office.internal", ".eu.mesh.internal"]);
        assert_eq!(rules.servers, ["198.18.0.2", "fd00::53"], "IPv4 first");
        assert_eq!(left_out, LeftOut::default());
    }

    #[test]
    fn only_a_mesh_namespace_takes_a_rule() {
        for refused in [
            "com",
            "co.uk",
            "example.com",
            "internal",
            ".internal.",
            "local",
            "mesh.local",
            "arpa",
            "in-addr.arpa",
            "home.arpa",
            "10.in-addr.arpa",
            "internal.com",
            ".",
            "",
        ] {
            assert_eq!(internal_suffix(refused), None, "{refused:?}");
        }
        assert_eq!(
            internal_suffix("mesh.internal").as_deref(),
            Some("mesh.internal")
        );
        assert_eq!(
            internal_suffix("EU.mesh.internal.").as_deref(),
            Some("eu.mesh.internal")
        );
        let (rules, left_out) = super::plan(
            &strings(&["com", "mesh.internal", "in-addr.arpa", "local"]),
            &strings(&["198.18.0.2"]),
            None,
        );
        assert_eq!(rules.unwrap().namespaces, [".mesh.internal"]);
        assert_eq!(left_out.suffixes, ["com", "in-addr.arpa", "local"]);
        // Nothing left: no rules at all, and the page names what was refused.
        let (rules, left_out) = super::plan(&strings(&["com"]), &strings(&["198.18.0.2"]), None);
        assert_eq!(rules, None);
        assert_eq!(
            issues(false, false, None, &left_out),
            [json!({ "code": "suffix_refused", "detail": "com" })]
        );
    }

    #[test]
    fn a_suffix_is_held_to_the_spec_names() {
        // Punycode labels, and hyphens inside a label.
        assert_eq!(
            internal_suffix("xn--bcher-kva.mesh.internal").as_deref(),
            Some("xn--bcher-kva.mesh.internal")
        );
        assert_eq!(
            internal_suffix("eu-west.mesh.internal").as_deref(),
            Some("eu-west.mesh.internal")
        );
        // One trailing dot is dropped; a leading dot, a second trailing dot or an empty label is no name.
        assert_eq!(
            internal_suffix("mesh.internal.").as_deref(),
            Some("mesh.internal")
        );
        for refused in [
            ".mesh.internal",
            "..mesh.internal",
            "mesh.internal..",
            "eu..mesh.internal",
            "-eu.mesh.internal",
            "eu-.mesh.internal",
            "mesh.-internal",
            "bücher.mesh.internal",
            "eu_west.mesh.internal",
            " ",
        ] {
            assert_eq!(internal_suffix(refused), None, "{refused:?}");
        }
        // Labels of 1–63 bytes.
        let label63 = "a".repeat(63);
        assert!(internal_suffix(&format!("{label63}.internal")).is_some());
        assert_eq!(internal_suffix(&format!("{label63}b.internal")), None);
        // 172 bytes in all, not 173.
        // Two labels of 62 bytes, then one that brings the whole name to `length` bytes.
        let name = |length: usize| {
            let long = "a".repeat(62);
            let last = length - 2 * 63 - ".internal".len();
            format!("{long}.{long}.{}.internal", "a".repeat(last))
        };
        assert_eq!(name(172).len(), 172);
        assert_eq!(name(173).len(), 173);
        assert!(internal_suffix(&name(172)).is_some());
        assert_eq!(internal_suffix(&name(173)), None);
        assert!(internal_suffix(&format!("{}.", name(172))).is_some());
    }

    #[test]
    fn what_is_not_a_plain_name_or_address_is_left_out() {
        let (rules, left_out) = super::plan(
            &strings(&[
                "office.internal",
                "a,b.internal",
                "x'; Remove-Item C:\\",
                "-bad.internal",
                "",
            ]),
            &strings(&["198.18.0.2", "not an address"]),
            None,
        );
        let rules = rules.unwrap();
        assert_eq!(rules.namespaces, [".office.internal"]);
        assert_eq!(rules.servers, ["198.18.0.2"]);
        assert_eq!(
            left_out.suffixes,
            ["a,b.internal", "x';Remove-ItemC:\\", "-bad.internal"]
        );
        assert_eq!(
            super::plan(&strings(&["office.internal"]), &[], None).0,
            None
        );
        assert_eq!(super::plan(&[], &strings(&["198.18.0.2"]), None).0, None);
    }

    #[test]
    fn a_bare_host_name_resolves_in_the_own_region() {
        let domains = strings(&["mesh.internal"]);
        let servers = strings(&["198.18.0.2"]);
        let search = |own: Option<&str>| plan(&domains, &servers, own).0.unwrap().search;
        assert_eq!(
            search(Some("par-laptop.EU.mesh.internal")).as_deref(),
            Some("eu.mesh.internal")
        );
        // A name outside the suffixes the rules cover, or none, sets no suffix.
        assert_eq!(search(Some("par-laptop.example.com")), None);
        assert_eq!(search(Some("par-laptop")), None);
        assert_eq!(search(None), None);
    }

    #[test]
    fn the_reverse_zones_the_engine_names_read_back_through_the_mesh() {
        for zone in [
            "10.in-addr.arpa",
            "144.10.in-addr.arpa",
            "144.144.10.in-addr.arpa",
            "5.144.144.10.in-addr.arpa",
            "0.255.10.in-addr.arpa",
            "d.f.ip6.arpa",
            "0.0.0.0.a.9.8.7.6.5.4.3.2.1.d.f.ip6.arpa",
            "C.F.IP6.ARPA.",
        ] {
            assert!(reverse_zone(zone).is_some(), "{zone}");
        }
        assert_eq!(
            reverse_zone("144.144.10.IN-ADDR.ARPA.").as_deref(),
            Some("144.144.10.in-addr.arpa")
        );
        for refused in [
            "arpa",
            "in-addr.arpa",
            "ip6.arpa",
            "home.arpa",
            "f.ip6.arpa",
            "256.10.in-addr.arpa",
            "010.10.in-addr.arpa",
            "1.2.3.4.5.in-addr.arpa",
            "a.10.in-addr.arpa",
            "-1.10.in-addr.arpa",
            "fd.ip6.arpa",
            "g.f.ip6.arpa",
            ".10.in-addr.arpa",
            "10..in-addr.arpa",
            "10.in-addr.arpa..",
            "10.in-addr.arpa.com",
        ] {
            assert_eq!(reverse_zone(refused), None, "{refused}");
        }
        let (rules, left_out) = super::plan(
            &strings(&[
                "mesh.internal",
                "144.144.10.in-addr.arpa",
                "in-addr.arpa",
                "144.144.10.in-addr.arpa",
                "d.f.ip6.arpa",
            ]),
            &strings(&["198.18.0.2"]),
            None,
        );
        assert_eq!(
            rules.unwrap().namespaces,
            [
                ".mesh.internal",
                ".144.144.10.in-addr.arpa",
                ".d.f.ip6.arpa"
            ]
        );
        assert_eq!(left_out.suffixes, ["in-addr.arpa"]);
        // A bare host name takes a mesh's suffix, never a reverse zone.
        let (rules, _) = super::plan(
            &strings(&["144.144.10.in-addr.arpa"]),
            &strings(&["198.18.0.2"]),
            Some("host.144.144.10.in-addr.arpa"),
        );
        assert_eq!(rules.unwrap().search, None);
    }

    #[test]
    fn an_ipv6_only_resolver_this_device_cannot_reach_is_named() {
        let with = |servers: &[&str]| Plan {
            namespaces: vec![".mesh.internal".into()],
            servers: strings(servers),
            search: None,
        };
        let none = |_: IpAddr| false;
        let all = |_: IpAddr| true;
        assert!(ipv6_only_unreachable(&with(&["fd00::53"]), none));
        assert!(!ipv6_only_unreachable(&with(&["fd00::53"]), all));
        // An IPv4 server is there to fall back on.
        assert!(!ipv6_only_unreachable(
            &with(&["198.18.0.2", "fd00::53"]),
            none
        ));
        assert!(!ipv6_only_unreachable(&with(&[]), none));
        // The loopback address is reachable wherever the family is.
        assert!(reachable("127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn leftovers_are_cleared_only_while_this_process_can_hold_the_adapter() {
        let cleared = AtomicBool::new(false);
        let removals = Cell::new(0);
        let remove = || {
            removals.set(removals.get() + 1);
            Ok(())
        };
        // Another signed-in user's window holds the adapter: its rules stay.
        assert!(!clear_leftovers_with(&cleared, || None::<()>, remove));
        assert_eq!(removals.get(), 0);
        // A removal that failed is tried again.
        assert!(!clear_leftovers_with(
            &cleared,
            || Some(()),
            || Err("refused".into())
        ));
        assert!(clear_leftovers_with(&cleared, || Some(()), remove));
        assert_eq!(removals.get(), 1);
        // Once per process.
        assert!(!clear_leftovers_with(&cleared, || Some(()), remove));
        assert_eq!(removals.get(), 1);
    }

    #[test]
    fn a_second_users_window_leaves_the_first_ones_rules() {
        use super::super::tun_mode::{open_exclusive, ProcessLock};
        static LOCK: ProcessLock = ProcessLock::new();
        let dir = std::env::temp_dir().join(format!("anixops-dns-lock-{}", std::process::id()));
        let path = dir.join("tun-adapter.lock");
        // The first user's window, another process, holds the adapter.
        let first = open_exclusive(&path).expect("the first window holds the adapter");
        let cleared = AtomicBool::new(false);
        let removed = Cell::new(false);
        assert!(!clear_leftovers_with(
            &cleared,
            || LOCK.acquire(&path).ok(),
            || {
                removed.set(true);
                Ok(())
            }
        ));
        assert!(!removed.get());
        // Its window is gone (signed out, or crashed): what is left is a leftover.
        drop(first);
        assert!(clear_leftovers_with(
            &cleared,
            || LOCK.acquire(&path).ok(),
            || {
                removed.set(true);
                Ok(())
            }
        ));
        assert!(removed.get());
        // The hold was let go again.
        drop(open_exclusive(&path).expect("free after the removal"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_page_reads_what_keeps_the_names_from_resolving() {
        assert!(issues(
            false,
            false,
            Some(Applied {
                policy_override: false
            }),
            &LeftOut::default()
        )
        .is_empty());
        assert_eq!(
            issues(
                true,
                true,
                Some(Applied {
                    policy_override: true
                }),
                &LeftOut {
                    suffixes: strings(&["com", "local"]),
                }
            ),
            [
                json!({ "code": "write_failed", "detail": null }),
                json!({ "code": "ipv6_unavailable", "detail": null }),
                json!({ "code": "policy_override", "detail": null }),
                json!({ "code": "suffix_refused", "detail": "com, local" }),
            ]
        );
    }

    /// Writes and removes real rules on this Windows device (needs administrator rights; CI runs it in its own
    /// step): only the rules with this program's comment go and another program's rule stays,
    /// the DNS cache is cleared, and a connection suffix kept for the adapter's GUID is taken away while no adapter
    /// exists, so that the next adapter with that GUID (the proxy's included) cannot inherit it.
    #[test]
    #[ignore = "changes this device's name resolution rules; run with --ignored as administrator"]
    fn the_rules_are_written_and_removed_on_windows() {
        let query = |script: &str| -> String {
            let output = super::super::system_programs::powershell(script)
                .unwrap()
                .env("ANIXOPS_MESH_DNS_COMMENT", COMMENT)
                .env("ANIXOPS_MESH_DNS_GUID", ADAPTER_GUID)
                .output()
                .expect("PowerShell runs");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        let ours = r#"@(Get-DnsClientNrptRule | Where-Object { $_.Comment -eq $env:ANIXOPS_MESH_DNS_COMMENT } | ForEach-Object { $_.Namespace } | Sort-Object) -join ' '"#;
        let interface = r#"$key = "HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces\$env:ANIXOPS_MESH_DNS_GUID""#;
        // Another program's rule.
        query(
            r#"Add-DnsClientNrptRule -Namespace '.ci-other.mesh.internal' -NameServers '10.255.255.54' -Comment 'AnixOps CI other' | Out-Null"#,
        );
        let (rules, _) = super::plan(
            &strings(&["ci.mesh.internal", "255.255.10.in-addr.arpa"]),
            &strings(&["10.255.255.53"]),
            Some("host.ci.mesh.internal"),
        );
        let rules = Plan {
            search: None,
            ..rules.unwrap()
        };
        apply(Some(&rules), None).expect("the rules are written");
        assert_eq!(query(ours), ".255.255.10.in-addr.arpa .ci.mesh.internal");
        // A suffix left for the adapter's GUID by a crash (no adapter exists here), as the next start finds it.
        query(&format!(
            "{interface}; if (-not (Test-Path $key)) {{ New-Item -Path $key -Force | Out-Null }}; Set-ItemProperty -Path $key -Name Domain -Value 'eu.mesh.internal'"
        ));
        apply(None, None).expect("the rules are removed");
        assert_eq!(query(ours), "");
        assert_eq!(
            query(
                r#"@(Get-DnsClientNrptRule | Where-Object { $_.Comment -eq 'AnixOps CI other' }).Count"#
            ),
            "1",
            "another program's rule stays"
        );
        assert_eq!(
            query(&format!(
                "{interface}; (Get-ItemProperty -Path $key -Name Domain).Domain"
            )),
            "",
            "the suffix kept for the adapter's GUID is gone"
        );
        query(
            r#"Get-DnsClientNrptRule | Where-Object { $_.Comment -eq 'AnixOps CI other' } | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }"#,
        );
    }
}
