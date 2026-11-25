//! Internal domain names (内网域名) while the engine runs for the 组网 mesh alone (mesh-status.md `tunnel.dns`, R28):
//! the adapter for the mesh alone announces no DNS, so each suffix the engine names is sent to the mesh's DNS server
//! with a Name Resolution Policy Table rule, and every other name keeps the device's own DNS. The rules carry one
//! fixed comment; they are removed when the engine for the mesh alone stops, and any left by a crash on the first
//! settle of the next start.

use std::net::IpAddr;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::Once;

use platform_windows::managed::append_managed_log;

/// Keeps PowerShell from flashing a console window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Marks the rules this program adds, and only those.
const COMMENT: &str = "AnixOps Net mesh";

/// Removes this program's rules, then adds one per namespace. The values come through the environment, never
/// through the script's text.
const SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
Get-DnsClientNrptRule | Where-Object { $_.Comment -eq $env:ANIXOPS_MESH_DNS_COMMENT } | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }
$servers = @($env:ANIXOPS_MESH_DNS_SERVERS -split ',' | Where-Object { $_ })
foreach ($namespace in @($env:ANIXOPS_MESH_DNS_NAMESPACES -split ',' | Where-Object { $_ })) {
  Add-DnsClientNrptRule -Namespace $namespace -NameServers $servers -Comment $env:ANIXOPS_MESH_DNS_COMMENT | Out-Null
}"#;

/// The rules for `domains` answered by `servers`: each suffix as an NRPT namespace (`.office.internal`, which also
/// covers its subdomains), each server as an address. A suffix that is not a plain DNS name, or a server that is not
/// an address, is left out; `None` when nothing is left.
pub fn rules(domains: &[String], servers: &[String]) -> Option<(Vec<String>, Vec<String>)> {
    let namespaces: Vec<String> = domains
        .iter()
        .filter_map(|domain| {
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
            plain.then(|| format!(".{domain}"))
        })
        .collect();
    let servers: Vec<String> = servers
        .iter()
        .filter_map(|server| server.trim().parse::<IpAddr>().ok())
        .map(|server| server.to_string())
        .collect();
    (!namespaces.is_empty() && !servers.is_empty()).then_some((namespaces, servers))
}

/// Puts the rules in place of this program's earlier ones; `None` only removes them.
pub fn apply(rules: Option<&(Vec<String>, Vec<String>)>) -> Result<(), String> {
    let (namespaces, servers) = rules
        .map(|(namespaces, servers)| (namespaces.join(","), servers.join(",")))
        .unwrap_or_default();
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            SCRIPT,
        ])
        .env("ANIXOPS_MESH_DNS_COMMENT", COMMENT)
        .env("ANIXOPS_MESH_DNS_NAMESPACES", namespaces)
        .env("ANIXOPS_MESH_DNS_SERVERS", servers)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("the name resolution rules could not be set: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "the name resolution rules could not be set: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// Removes rules a crash left behind, once per process.
pub fn clear_leftovers_once() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if let Err(error) = apply(None) {
            let _ = append_managed_log("tauri-gui", &format!("mesh: {error}"));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_internal_suffix_goes_to_the_mesh_dns_server() {
        let (namespaces, servers) = rules(
            &["office.internal".into(), ".EU.Mesh.Internal.".into()],
            &["198.18.0.2".into(), "fd00::53".into()],
        )
        .unwrap();
        assert_eq!(namespaces, [".office.internal", ".eu.mesh.internal"]);
        assert_eq!(servers, ["198.18.0.2", "fd00::53"]);
    }

    #[test]
    fn what_is_not_a_plain_name_or_address_is_left_out() {
        let (namespaces, servers) = rules(
            &[
                "office.internal".into(),
                "a,b.internal".into(),
                "x'; Remove-Item C:\\".into(),
                "-bad.internal".into(),
                "".into(),
            ],
            &["198.18.0.2".into(), "not an address".into()],
        )
        .unwrap();
        assert_eq!(namespaces, [".office.internal"]);
        assert_eq!(servers, ["198.18.0.2"]);
        assert_eq!(rules(&["office.internal".into()], &[]), None);
        assert_eq!(rules(&[], &["198.18.0.2".into()]), None);
    }
}
