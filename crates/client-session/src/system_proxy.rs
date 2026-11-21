//! Points the desktop's proxy settings at the local proxy and puts them back afterwards.
//!
//! GNOME-family desktops read `org.gnome.system.proxy` through `gsettings`; KDE Plasma reads `kioslaverc`
//! through `kwriteconfig6` (or `kwriteconfig5`). Other desktops are reported as unsupported, and the user can
//! point applications at the local address by hand. Every key the client writes is backed up before it is
//! written, not only the mode: a desktop that used a manual proxy of its own gets its servers, ports and
//! exceptions back, not ours (third-party review F01).

use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Desktop {
    Gnome,
    Kde,
}

impl Desktop {
    /// From `XDG_CURRENT_DESKTOP`, a colon-separated list such as `ubuntu:GNOME`.
    pub fn detect(current_desktop: Option<&str>) -> Option<Self> {
        let value = current_desktop?.to_ascii_lowercase();
        let names: Vec<&str> = value.split(':').collect();
        if names.contains(&"kde") {
            return Some(Self::Kde);
        }
        const GNOME_FAMILY: [&str; 6] = [
            "gnome",
            "unity",
            "cinnamon",
            "budgie",
            "pantheon",
            "x-cinnamon",
        ];
        names
            .iter()
            .any(|name| GNOME_FAMILY.contains(name))
            .then_some(Self::Gnome)
    }

    pub fn from_env() -> Option<Self> {
        Self::detect(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref())
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Gnome => "GNOME",
            Self::Kde => "KDE Plasma",
        }
    }
}

/// The desktop's proxy settings before the client changed them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyBackup {
    pub desktop: Desktop,
    /// GNOME: the `mode` string such as `none`. KDE: the `ProxyType` number such as `0`.
    pub previous_mode: String,
    /// Every other key [`apply_commands`] writes, with what it held before. Empty in a backup made before the
    /// client kept them; such a backup puts back the mode only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous_keys: Vec<SavedKey>,
}

/// One proxy key and what it held before the client wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedKey {
    /// GNOME: the schema, such as `org.gnome.system.proxy.http`. KDE: the `kioslaverc` group.
    pub group: String,
    pub key: String,
    /// GNOME: the value as `gsettings get` prints it, which `gsettings set` reads back. KDE: the text, or `None`
    /// when the key was not set, so it is deleted again rather than left set to an empty value.
    pub value: Option<String>,
}

const GNOME_SCHEMA: &str = "org.gnome.system.proxy";
const KDE_GROUP: &str = "Proxy Settings";
/// What `kreadconfig` answers for a key that is not set: no proxy setting can hold this text.
const KDE_UNSET: &str = "<anixops-net:unset>";

/// The keys [`apply_commands`] writes besides the mode, by group.
pub fn written_keys(desktop: Desktop) -> Vec<(String, &'static str)> {
    match desktop {
        Desktop::Gnome => {
            let mut keys = Vec::new();
            for scheme in ["http", "https", "socks"] {
                for key in ["host", "port"] {
                    keys.push((format!("{GNOME_SCHEMA}.{scheme}"), key));
                }
            }
            keys.push((GNOME_SCHEMA.to_string(), "ignore-hosts"));
            keys
        }
        Desktop::Kde => ["httpProxy", "httpsProxy", "socksProxy", "NoProxyFor"]
            .into_iter()
            .map(|key| (KDE_GROUP.to_string(), key))
            .collect(),
    }
}

/// The command that reads one key for the backup.
pub fn read_key_command(desktop: Desktop, kwrite: &str, group: &str, key: &str) -> Invocation {
    match desktop {
        Desktop::Gnome => invocation(&["gsettings", "get", group, key]),
        Desktop::Kde => invocation(&[
            kwrite.replace("kwrite", "kread").as_str(),
            "--file",
            "kioslaverc",
            "--group",
            group,
            "--key",
            key,
            "--default",
            KDE_UNSET,
        ]),
    }
}

/// Parses the output of [`read_key_command`].
pub fn parse_key(desktop: Desktop, output: &str) -> Option<String> {
    let value = output.trim_end_matches(['\n', '\r']);
    match desktop {
        Desktop::Gnome => Some(value.trim().to_string()),
        Desktop::Kde => (value != KDE_UNSET).then(|| value.to_string()),
    }
}

/// One external command: program and arguments.
pub type Invocation = Vec<String>;

fn invocation(parts: &[&str]) -> Invocation {
    parts.iter().map(|part| part.to_string()).collect()
}

/// Commands that read the current mode, for the backup.
pub fn read_mode_command(desktop: Desktop, kwrite: &str) -> Invocation {
    match desktop {
        Desktop::Gnome => invocation(&["gsettings", "get", "org.gnome.system.proxy", "mode"]),
        Desktop::Kde => invocation(&[
            kwrite.replace("kwrite", "kread").as_str(),
            "--file",
            "kioslaverc",
            "--group",
            "Proxy Settings",
            "--key",
            "ProxyType",
        ]),
    }
}

/// Parses the output of [`read_mode_command`]; `gsettings` quotes strings.
pub fn parse_mode(desktop: Desktop, output: &str) -> String {
    let value = output.trim().trim_matches('\'').trim();
    match (desktop, value.is_empty()) {
        (Desktop::Gnome, true) => "none".to_string(),
        (Desktop::Kde, true) => "0".to_string(),
        _ => value.to_string(),
    }
}

/// Commands that route HTTP, HTTPS and SOCKS traffic through `127.0.0.1:port` and leave local addresses direct.
pub fn apply_commands(desktop: Desktop, kwrite: &str, port: u16) -> Vec<Invocation> {
    let port = port.to_string();
    match desktop {
        Desktop::Gnome => {
            let mut commands = Vec::new();
            for scheme in ["http", "https", "socks"] {
                let schema = format!("org.gnome.system.proxy.{scheme}");
                commands.push(invocation(&[
                    "gsettings",
                    "set",
                    schema.as_str(),
                    "host",
                    "127.0.0.1",
                ]));
                commands.push(invocation(&[
                    "gsettings",
                    "set",
                    schema.as_str(),
                    "port",
                    port.as_str(),
                ]));
            }
            commands.push(invocation(&[
                "gsettings",
                "set",
                "org.gnome.system.proxy",
                "ignore-hosts",
                "['localhost', '127.0.0.0/8', '::1', '10.0.0.0/8', '172.16.0.0/12', '192.168.0.0/16']",
            ]));
            commands.push(invocation(&[
                "gsettings",
                "set",
                "org.gnome.system.proxy",
                "mode",
                "manual",
            ]));
            commands
        }
        Desktop::Kde => {
            let http = format!("http://127.0.0.1 {port}");
            let socks = format!("socks://127.0.0.1 {port}");
            let mut commands: Vec<Invocation> = [
                ("httpProxy", http.as_str()),
                ("httpsProxy", http.as_str()),
                ("socksProxy", socks.as_str()),
                (
                    "NoProxyFor",
                    "localhost,127.0.0.0/8,::1,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16",
                ),
                ("ProxyType", "1"),
            ]
            .iter()
            .map(|(key, value)| kde_write(kwrite, key, value))
            .collect();
            commands.push(kde_reload());
            commands
        }
    }
}

/// Commands that put every backed-up key back, then the mode.
pub fn restore_commands(backup: &ProxyBackup, kwrite: &str) -> Vec<Invocation> {
    let mut commands: Vec<Invocation> = backup
        .previous_keys
        .iter()
        .map(|saved| match (backup.desktop, &saved.value) {
            (Desktop::Gnome, Some(value)) => invocation(&[
                "gsettings",
                "set",
                saved.group.as_str(),
                saved.key.as_str(),
                value.as_str(),
            ]),
            (Desktop::Gnome, None) => invocation(&[
                "gsettings",
                "reset",
                saved.group.as_str(),
                saved.key.as_str(),
            ]),
            (Desktop::Kde, Some(value)) => invocation(&[
                kwrite,
                "--file",
                "kioslaverc",
                "--group",
                saved.group.as_str(),
                "--key",
                saved.key.as_str(),
                value.as_str(),
            ]),
            (Desktop::Kde, None) => invocation(&[
                kwrite,
                "--file",
                "kioslaverc",
                "--group",
                saved.group.as_str(),
                "--key",
                saved.key.as_str(),
                "--delete",
            ]),
        })
        .collect();
    match backup.desktop {
        Desktop::Gnome => commands.push(invocation(&[
            "gsettings",
            "set",
            GNOME_SCHEMA,
            "mode",
            backup.previous_mode.as_str(),
        ])),
        Desktop::Kde => {
            commands.push(kde_write(
                kwrite,
                "ProxyType",
                backup.previous_mode.as_str(),
            ));
            commands.push(kde_reload());
        }
    }
    commands
}

fn kde_write(kwrite: &str, key: &str, value: &str) -> Invocation {
    invocation(&[
        kwrite,
        "--file",
        "kioslaverc",
        "--group",
        "Proxy Settings",
        "--key",
        key,
        value,
    ])
}

/// Tells running KDE applications to reread their proxy settings.
fn kde_reload() -> Invocation {
    invocation(&[
        "dbus-send",
        "--type=signal",
        "/KIO/Scheduler",
        "org.kde.KIO.Scheduler.reparseSlaveConfiguration",
        "string:",
    ])
}

/// `kwriteconfig6` on Plasma 6, else `kwriteconfig5`.
pub fn kwrite_program() -> &'static str {
    let found = |name: &str| {
        std::env::var_os("PATH")
            .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
            .unwrap_or(false)
    };
    if found("kwriteconfig6") {
        "kwriteconfig6"
    } else {
        "kwriteconfig5"
    }
}

fn run(command: &Invocation) -> Result<String, String> {
    let (program, arguments) = command.split_first().ok_or("empty command")?;
    let output = Command::new(program)
        .args(arguments)
        .output()
        .map_err(|error| format!("{program} could not run: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Backs up every key it writes, then applies the proxy. Nothing is written unless the whole backup was read; on a
/// failure while writing, the desktop is put back as far as possible.
pub fn apply(desktop: Desktop, port: u16) -> Result<ProxyBackup, String> {
    let kwrite = kwrite_program();
    let previous = run(&read_mode_command(desktop, kwrite))?;
    let mut previous_keys = Vec::new();
    for (group, key) in written_keys(desktop) {
        let output = run(&read_key_command(desktop, kwrite, &group, key))?;
        previous_keys.push(SavedKey {
            group,
            key: key.to_string(),
            value: parse_key(desktop, &output),
        });
    }
    let backup = ProxyBackup {
        desktop,
        previous_mode: parse_mode(desktop, &previous),
        previous_keys,
    };
    for command in apply_commands(desktop, kwrite, port) {
        if let Err(error) = run(&command) {
            let _ = restore(&backup);
            return Err(error);
        }
    }
    Ok(backup)
}

pub fn restore(backup: &ProxyBackup) -> Result<(), String> {
    let kwrite = kwrite_program();
    let mut first_error = None;
    for command in restore_commands(backup, kwrite) {
        if let Err(error) = run(&command) {
            // The reload signal is best effort; the setting itself is what matters.
            if command.first().map(String::as_str) != Some("dbus-send") {
                first_error.get_or_insert(error);
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_desktops_from_the_environment_value() {
        assert_eq!(Desktop::detect(Some("ubuntu:GNOME")), Some(Desktop::Gnome));
        assert_eq!(Desktop::detect(Some("X-Cinnamon")), Some(Desktop::Gnome));
        assert_eq!(Desktop::detect(Some("KDE")), Some(Desktop::Kde));
        assert_eq!(Desktop::detect(Some("XFCE")), None);
        assert_eq!(Desktop::detect(None), None);
    }

    #[test]
    fn gnome_commands_set_every_scheme_then_switch_the_mode() {
        let commands = apply_commands(Desktop::Gnome, "kwriteconfig6", 7890);
        assert!(commands.contains(&invocation(&[
            "gsettings",
            "set",
            "org.gnome.system.proxy.socks",
            "port",
            "7890"
        ])));
        assert_eq!(
            commands.last().unwrap(),
            &invocation(&[
                "gsettings",
                "set",
                "org.gnome.system.proxy",
                "mode",
                "manual"
            ]),
            "the mode switches only after the addresses are in place"
        );
        let backup = ProxyBackup {
            desktop: Desktop::Gnome,
            previous_mode: parse_mode(Desktop::Gnome, "'auto'\n"),
            previous_keys: Vec::new(),
        };
        assert_eq!(
            restore_commands(&backup, "kwriteconfig6"),
            vec![invocation(&[
                "gsettings",
                "set",
                "org.gnome.system.proxy",
                "mode",
                "auto"
            ])]
        );
    }

    #[test]
    fn kde_commands_use_the_found_tool_and_reload() {
        let commands = apply_commands(Desktop::Kde, "kwriteconfig5", 1080);
        assert!(commands
            .iter()
            .all(|command| command[0] == "kwriteconfig5" || command[0] == "dbus-send"));
        assert!(commands
            .iter()
            .any(|command| command.last().unwrap() == "http://127.0.0.1 1080"));
        assert_eq!(
            read_mode_command(Desktop::Kde, "kwriteconfig5")[0],
            "kreadconfig5"
        );
        assert_eq!(parse_mode(Desktop::Kde, ""), "0");
        let restore = restore_commands(
            &ProxyBackup {
                desktop: Desktop::Kde,
                previous_mode: "0".to_string(),
                previous_keys: Vec::new(),
            },
            "kwriteconfig6",
        );
        assert_eq!(restore[0].last().unwrap(), "0");
    }

    /// Every key `apply_commands` writes, as (group, key).
    fn keys_written(desktop: Desktop) -> Vec<(String, String)> {
        apply_commands(desktop, "kwriteconfig6", 7890)
            .iter()
            .filter_map(|command| match desktop {
                Desktop::Gnome => Some((command[2].clone(), command[3].clone())),
                Desktop::Kde if command[0] == "kwriteconfig6" => {
                    Some((command[4].clone(), command[6].clone()))
                }
                Desktop::Kde => None,
            })
            .filter(|(_, key)| key != "mode" && key != "ProxyType")
            .collect()
    }

    #[test]
    fn every_key_written_is_backed_up_first() {
        for desktop in [Desktop::Gnome, Desktop::Kde] {
            let backed_up: Vec<(String, String)> = written_keys(desktop)
                .into_iter()
                .map(|(group, key)| (group, key.to_string()))
                .collect();
            assert_eq!(keys_written(desktop), backed_up, "{desktop:?}");
        }
    }

    #[test]
    fn a_manual_gnome_proxy_comes_back_whole() {
        // The user's own manual proxy, as `gsettings get` prints each key.
        let before = [
            ("org.gnome.system.proxy.http", "host", "'proxy.corp'"),
            ("org.gnome.system.proxy.http", "port", "3128"),
            ("org.gnome.system.proxy.https", "host", "'proxy.corp'"),
            ("org.gnome.system.proxy.https", "port", "3128"),
            ("org.gnome.system.proxy.socks", "host", "''"),
            ("org.gnome.system.proxy.socks", "port", "0"),
            (
                "org.gnome.system.proxy",
                "ignore-hosts",
                "['localhost', '*.corp']",
            ),
        ];
        let backup = ProxyBackup {
            desktop: Desktop::Gnome,
            previous_mode: parse_mode(Desktop::Gnome, "'manual'\n"),
            previous_keys: before
                .iter()
                .map(|(group, key, value)| SavedKey {
                    group: group.to_string(),
                    key: key.to_string(),
                    value: parse_key(Desktop::Gnome, &format!("{value}\n")),
                })
                .collect(),
        };
        let restore = restore_commands(&backup, "kwriteconfig6");
        for (group, key, value) in before {
            assert!(
                restore.contains(&invocation(&["gsettings", "set", group, key, value])),
                "{group} {key} goes back to {value}"
            );
        }
        // The mode goes back last, once the addresses are the user's again.
        assert_eq!(
            restore.last().unwrap(),
            &invocation(&[
                "gsettings",
                "set",
                "org.gnome.system.proxy",
                "mode",
                "manual"
            ])
        );
    }

    #[test]
    fn kde_keys_come_back_or_are_deleted_when_they_were_not_set() {
        assert_eq!(parse_key(Desktop::Kde, &format!("{KDE_UNSET}\n")), None);
        assert_eq!(
            parse_key(Desktop::Kde, "http://proxy.corp 3128\n"),
            Some("http://proxy.corp 3128".to_string())
        );
        assert_eq!(parse_key(Desktop::Kde, "\n"), Some(String::new()));
        let read = read_key_command(Desktop::Kde, "kwriteconfig5", KDE_GROUP, "httpProxy");
        assert_eq!(read[0], "kreadconfig5");
        assert!(read.windows(2).any(|pair| pair == ["--default", KDE_UNSET]));

        let backup = ProxyBackup {
            desktop: Desktop::Kde,
            previous_mode: "1".to_string(),
            previous_keys: vec![
                SavedKey {
                    group: KDE_GROUP.to_string(),
                    key: "httpProxy".to_string(),
                    value: Some("http://proxy.corp 3128".to_string()),
                },
                SavedKey {
                    group: KDE_GROUP.to_string(),
                    key: "NoProxyFor".to_string(),
                    value: None,
                },
            ],
        };
        let restore = restore_commands(&backup, "kwriteconfig6");
        assert_eq!(
            restore[0],
            invocation(&[
                "kwriteconfig6",
                "--file",
                "kioslaverc",
                "--group",
                "Proxy Settings",
                "--key",
                "httpProxy",
                "http://proxy.corp 3128"
            ])
        );
        assert_eq!(restore[1].last().unwrap(), "--delete");
        assert_eq!(restore[2].last().unwrap(), "1");
        assert_eq!(restore[3][0], "dbus-send");
    }

    #[test]
    fn a_backup_saved_before_the_keys_were_kept_still_loads() {
        let old: ProxyBackup =
            serde_json::from_str(r#"{"desktop":"Gnome","previous_mode":"none"}"#).unwrap();
        assert!(old.previous_keys.is_empty());
        assert_eq!(restore_commands(&old, "kwriteconfig6").len(), 1);
        assert_eq!(
            serde_json::to_string(&old).unwrap(),
            r#"{"desktop":"Gnome","previous_mode":"none"}"#
        );
    }
}
