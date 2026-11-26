//! Internal domain names (内网域名) while the engine runs for the 组网 mesh alone (mesh-status.md `tunnel.dns`, R28):
//! the adapter for the mesh alone announces no DNS, so each suffix the engine names is sent to the mesh's DNS server
//! with a Name Resolution Policy Table rule, and every other name keeps the device's own DNS. Also:
//!
//! - the suffixes are held to a mesh's own namespace ([`internal_suffix`]): at least two labels under `internal`
//!   (`mesh.internal`), since a rule applies to every program and user of the device, and a suffix such as `com`
//!   would hand the device's lookups to whoever runs the network;
//! - the reverse zones of the device's own mesh subnets go to the same server ([`reverse_zones`]), so that a mesh
//!   address reads back as its name;
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

/// The most reverse zones one subnet may take: a prefix between label boundaries spreads over the zones of the next
/// label (an IPv4 /12 over 16 zones); one wider than that is left out.
const ZONES_PER_SUBNET: usize = 16;

/// Removes this program's rules, then sets the adapter's connection suffix (or takes it away), adds one rule per
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
    /// NRPT namespaces: each suffix (`.eu.mesh.internal`, which also covers its subdomains) and each reverse zone
    /// (`.144.144.10.in-addr.arpa`).
    pub namespaces: Vec<String>,
    /// The mesh's DNS servers, IPv4 first.
    pub servers: Vec<String>,
    /// The device's own region's suffix (`eu.mesh.internal`), for bare host names.
    pub search: Option<String>,
}

/// What the engine named and the rules leave out, for the log and the page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LeftOut {
    /// Suffixes outside a mesh's namespace, as plain text.
    pub suffixes: Vec<String>,
    /// Own subnets whose reverse zones would be too many.
    pub reverse: Vec<String>,
}

/// The rules for the engine's `domains` answered by `servers`, with the reverse zones of the device's own
/// `subnets` (`self.address`, `self.addressV6`) and its own name `own_domain` (`self.domain`). `None` when no suffix
/// or no server is left.
pub fn plan(
    domains: &[String],
    servers: &[String],
    subnets: &[String],
    own_domain: Option<&str>,
) -> (Option<Plan>, LeftOut) {
    let mut left_out = LeftOut::default();
    let mut suffixes: Vec<String> = Vec::new();
    for domain in domains {
        match internal_suffix(domain) {
            Some(suffix) if !suffixes.contains(&suffix) => suffixes.push(suffix),
            Some(_) => {}
            None if domain.trim().is_empty() => {}
            None => left_out.suffixes.push(printable(domain)),
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
    if suffixes.is_empty() || servers.is_empty() {
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
    let mut namespaces: Vec<String> = suffixes.iter().map(|suffix| format!(".{suffix}")).collect();
    for subnet in subnets {
        match reverse_zones(subnet) {
            Some(zones) => {
                for zone in zones {
                    let namespace = format!(".{zone}");
                    if !namespaces.contains(&namespace) {
                        namespaces.push(namespace);
                    }
                }
            }
            None => left_out.reverse.push(printable(subnet)),
        }
    }
    (
        Some(Plan {
            namespaces,
            servers,
            search,
        }),
        left_out,
    )
}

/// `domain` as a plain lowercase DNS name without the outer dots: labels of letters, digits and hyphens.
fn plain_name(domain: &str) -> Option<String> {
    let domain = domain.trim().trim_matches('.').to_ascii_lowercase();
    let plain = !domain.is_empty()
        && domain.len() <= 253
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

/// The reverse zones of a subnet (`10.144.144.5/24`, host bits ignored; a bare address is one host): the labels of
/// its prefix, spread over the next label's values when the prefix ends between labels. `None` when it is not a
/// subnet, wider than a mesh route may be (IPv4 /8, IPv6 /7), or would take more than [`ZONES_PER_SUBNET`] zones.
pub fn reverse_zones(subnet: &str) -> Option<Vec<String>> {
    let subnet = subnet.trim();
    let (address, length) = match subnet.split_once('/') {
        Some((address, length)) => (address, Some(length.trim().parse::<u8>().ok()?)),
        None => (subnet, None),
    };
    // Labels of `bits` bits each, most significant first, and the zone's tree.
    let (labels, bits, max, min, tree): (Vec<u8>, u8, u8, u8, &str) =
        match address.trim().parse::<IpAddr>().ok()? {
            IpAddr::V4(address) => (
                address.octets().to_vec(),
                8,
                32,
                client_core::backend::mesh::ROUTE_MIN_PREFIX_V4,
                "in-addr.arpa",
            ),
            IpAddr::V6(address) => (
                address
                    .octets()
                    .iter()
                    .flat_map(|byte| [byte >> 4, byte & 0x0f])
                    .collect(),
                4,
                128,
                client_core::backend::mesh::ROUTE_MIN_PREFIX_V6,
                "ip6.arpa",
            ),
        };
    let length = length.unwrap_or(max);
    if length < min || length > max {
        return None;
    }
    let full = usize::from(length / bits);
    let rest = length % bits;
    let zone = |labels: &[u8]| {
        let mut parts: Vec<String> = labels
            .iter()
            .rev()
            .map(|label| {
                if bits == 8 {
                    label.to_string()
                } else {
                    format!("{label:x}")
                }
            })
            .collect();
        parts.push(tree.to_string());
        parts.join(".")
    };
    if rest == 0 {
        return Some(vec![zone(&labels[..full])]);
    }
    let count = 1usize << (bits - rest);
    if count > ZONES_PER_SUBNET {
        return None;
    }
    let mask = (0xffu8 << (bits - rest)) & (0xffu8 >> (8 - bits));
    let base = labels[full] & mask;
    Some(
        (0..count)
            .map(|offset| {
                let mut prefix = labels[..full].to_vec();
                prefix.push(base + offset as u8);
                zone(&prefix[..])
            })
            .collect(),
    )
}

/// Text from the engine fit for the log and the page: printable ASCII, at most 64 characters.
fn printable(text: &str) -> String {
    text.trim()
        .chars()
        .filter(|character| character.is_ascii_graphic())
        .take(64)
        .collect()
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
    let output = super::system_programs::powershell(SCRIPT)
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
/// tried again), `policy_override` (a group policy's rules take their place) and `suffix_refused` (suffixes outside
/// a mesh's namespace; `detail` names them).
pub fn issues(failed: bool, applied: Option<Applied>, left_out: &LeftOut) -> Vec<Value> {
    let mut issues = Vec::new();
    if failed {
        issues.push(json!({ "code": "write_failed", "detail": null }));
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
            &strings(&["office.internal", ".EU.Mesh.Internal.", "office.internal"]),
            &strings(&["fd00::53", "198.18.0.2", "198.18.0.2"]),
            &[],
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
            &[],
            None,
        );
        assert_eq!(rules.unwrap().namespaces, [".mesh.internal"]);
        assert_eq!(left_out.suffixes, ["com", "in-addr.arpa", "local"]);
        // Nothing left: no rules at all, and the page names what was refused.
        let (rules, left_out) =
            super::plan(&strings(&["com"]), &strings(&["198.18.0.2"]), &[], None);
        assert_eq!(rules, None);
        assert_eq!(
            issues(false, None, &left_out),
            [json!({ "code": "suffix_refused", "detail": "com" })]
        );
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
            &[],
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
            super::plan(&strings(&["office.internal"]), &[], &[], None).0,
            None
        );
        assert_eq!(
            super::plan(&[], &strings(&["198.18.0.2"]), &[], None).0,
            None
        );
    }

    #[test]
    fn a_bare_host_name_resolves_in_the_own_region() {
        let domains = strings(&["mesh.internal"]);
        let servers = strings(&["198.18.0.2"]);
        let search = |own: Option<&str>| plan(&domains, &servers, &[], own).0.unwrap().search;
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
    fn the_own_subnets_read_back_through_the_mesh() {
        assert_eq!(
            reverse_zones("10.144.144.5/24").unwrap(),
            ["144.144.10.in-addr.arpa"]
        );
        assert_eq!(reverse_zones("10.0.0.0/8").unwrap(), ["10.in-addr.arpa"]);
        assert_eq!(
            reverse_zones("10.144.144.5").unwrap(),
            ["5.144.144.10.in-addr.arpa"]
        );
        let spread = reverse_zones("10.150.1.2/12").unwrap();
        assert_eq!(spread.len(), 16);
        assert_eq!(spread[0], "144.10.in-addr.arpa");
        assert_eq!(spread[15], "159.10.in-addr.arpa");
        assert_eq!(
            reverse_zones("10.144.0.0/22").unwrap(),
            [
                "0.144.10.in-addr.arpa",
                "1.144.10.in-addr.arpa",
                "2.144.10.in-addr.arpa",
                "3.144.10.in-addr.arpa"
            ]
        );
        assert_eq!(
            reverse_zones("fd12:3456:789a::5/64").unwrap(),
            ["0.0.0.0.a.9.8.7.6.5.4.3.2.1.d.f.ip6.arpa"]
        );
        assert_eq!(
            reverse_zones("fd12:3456:789a::5/62").unwrap(),
            [
                "0.0.0.0.a.9.8.7.6.5.4.3.2.1.d.f.ip6.arpa",
                "1.0.0.0.a.9.8.7.6.5.4.3.2.1.d.f.ip6.arpa",
                "2.0.0.0.a.9.8.7.6.5.4.3.2.1.d.f.ip6.arpa",
                "3.0.0.0.a.9.8.7.6.5.4.3.2.1.d.f.ip6.arpa"
            ]
        );
        // Wider than a mesh route, too many zones, or not a subnet.
        for refused in [
            "10.0.0.0/7",
            "0.0.0.0/0",
            "10.128.0.0/9",
            "fd00::/6",
            "10.0.0.0/33",
            "mesh",
            "10.0.0.0/x",
        ] {
            assert_eq!(reverse_zones(refused), None, "{refused}");
        }
        let (rules, left_out) = super::plan(
            &strings(&["mesh.internal"]),
            &strings(&["198.18.0.2"]),
            &strings(&["10.144.144.5/24", "10.128.0.0/9", "10.144.144.9/24"]),
            None,
        );
        assert_eq!(
            plan.unwrap().namespaces,
            [".mesh.internal", ".144.144.10.in-addr.arpa"]
        );
        assert_eq!(left_out.reverse, ["10.128.0.0/9"]);
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
            Some(Applied {
                policy_override: false
            }),
            &LeftOut::default()
        )
        .is_empty());
        assert_eq!(
            issues(
                true,
                Some(Applied {
                    policy_override: true
                }),
                &LeftOut {
                    suffixes: strings(&["com", "local"]),
                    reverse: strings(&["10.128.0.0/9"]),
                }
            ),
            [
                json!({ "code": "write_failed", "detail": null }),
                json!({ "code": "policy_override", "detail": null }),
                json!({ "code": "suffix_refused", "detail": "com, local" }),
            ]
        );
    }
}
