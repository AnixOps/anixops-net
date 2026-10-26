//! The DNS choices the Windows and Linux clients offer and the engine `dns`
//! section each one adds to the envelope. The resolvers are addressed by IP
//! with a TLS name, so the engine never has to resolve a resolver's own name.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Rule set the China routing modes define; the split choice sends its names
/// to the domestic resolver too.
const CHINA_RULE_SET: &str = "china";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsPreset {
    /// No `dns` section: the engine's built-in default.
    #[default]
    System,
    /// Everything over Cloudflare DNS over HTTPS, through the proxy.
    Encrypted,
    /// Mainland names over AliDNS (direct), everything else over Cloudflare
    /// (through the proxy).
    Split,
}

impl DnsPreset {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Encrypted => "encrypted",
            Self::Split => "split",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "system" => Some(Self::System),
            "encrypted" => Some(Self::Encrypted),
            "split" => Some(Self::Split),
            _ => None,
        }
    }
}

/// `envelope` with the preset's `dns` section in place of its own; the
/// system choice leaves the envelope as it is.
pub fn apply(envelope: &str, preset: DnsPreset) -> Result<String, String> {
    if preset == DnsPreset::System {
        return Ok(envelope.to_string());
    }
    let mut envelope: Value = serde_json::from_str(envelope).map_err(|error| error.to_string())?;
    let has_china = envelope
        .pointer(&format!("/routing/rule_sets/{CHINA_RULE_SET}"))
        .is_some();
    let object = envelope
        .as_object_mut()
        .ok_or("the envelope must be a JSON object")?;
    object.insert("dns".to_string(), section(preset, has_china));
    Ok(envelope.to_string())
}

fn section(preset: DnsPreset, has_china: bool) -> Value {
    let cloudflare = json!({
        "name": "cloudflare",
        "url": "https://1.1.1.1/dns-query",
        "server_name": "cloudflare-dns.com",
    });
    if preset != DnsPreset::Split {
        return json!({ "servers": [cloudflare], "default": "cloudflare" });
    }
    let mut rules = vec![json!({ "domain_suffix": ["cn"], "server": "alidns" })];
    if has_china {
        rules.push(json!({ "rule_set": CHINA_RULE_SET, "server": "alidns" }));
    }
    json!({
        "servers": [
            cloudflare,
            {
                "name": "alidns",
                "url": "https://223.5.5.5/dns-query",
                "server_name": "dns.alidns.com",
                "via": "direct",
            },
        ],
        "default": "cloudflare",
        "rules": rules,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINGLE: &str = r#"{"schema_version":1,"profile_id":"p","outbound":{"type":"direct"}}"#;

    #[test]
    fn system_keeps_the_envelope() {
        assert_eq!(apply(SINGLE, DnsPreset::System).unwrap(), SINGLE);
    }

    #[test]
    fn encrypted_sends_everything_to_cloudflare() {
        let envelope: Value =
            serde_json::from_str(&apply(SINGLE, DnsPreset::Encrypted).unwrap()).unwrap();
        assert_eq!(envelope["dns"]["default"], "cloudflare");
        assert_eq!(envelope["dns"]["servers"].as_array().unwrap().len(), 1);
        assert!(envelope["dns"].get("rules").is_none());
        assert_eq!(envelope["outbound"]["type"], "direct");
    }

    #[test]
    fn split_uses_the_china_rule_set_when_the_mode_defines_it() {
        let plain: Value = serde_json::from_str(&apply(SINGLE, DnsPreset::Split).unwrap()).unwrap();
        assert_eq!(plain["dns"]["rules"].as_array().unwrap().len(), 1);
        assert_eq!(plain["dns"]["servers"][1]["via"], "direct");

        let routed = r#"{"routing":{"rule_sets":{"china":{"domain_suffix":["cn"]}}}}"#;
        let routed: Value =
            serde_json::from_str(&apply(routed, DnsPreset::Split).unwrap()).unwrap();
        assert_eq!(routed["dns"]["rules"][1]["rule_set"], "china");
    }

    #[test]
    fn names_round_trip() {
        for preset in [DnsPreset::System, DnsPreset::Encrypted, DnsPreset::Split] {
            assert_eq!(DnsPreset::parse(preset.as_str()), Some(preset));
        }
        assert_eq!(DnsPreset::parse("custom"), None);
    }
}
