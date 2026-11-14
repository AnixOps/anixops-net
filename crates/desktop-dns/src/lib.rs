//! The DNS choices the Windows and Linux clients offer and the engine `dns`
//! section each one adds to the envelope. The resolvers are addressed by IP
//! with a TLS name, so the engine never has to resolve a resolver's own name.
//! [`rules`] puts the user's own rule lines in front of the envelope's rules;
//! [`custom`] holds the user's own servers, hosts and the Fake-IP switch;
//! [`redirects`] the destination rewrites (the engine's `redirects`).

pub mod custom;
pub mod redirects;
pub mod rules;

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
    /// The user's own servers, per-domain servers and hosts ([`custom`]).
    Custom,
}

impl DnsPreset {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Encrypted => "encrypted",
            Self::Split => "split",
            Self::Custom => "custom",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "system" => Some(Self::System),
            "encrypted" => Some(Self::Encrypted),
            "split" => Some(Self::Split),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// `envelope` with the preset's `dns` section in place of its own, and
/// Fake-IP on when the user turned it on ([`custom`]). The system choice
/// without Fake-IP leaves the envelope as it is.
pub fn apply(envelope: &str, preset: DnsPreset) -> Result<String, String> {
    apply_with(envelope, preset, &custom::current()?)
}

/// [`apply`] with these settings instead of the saved ones.
pub fn apply_with(
    envelope: &str,
    preset: DnsPreset,
    settings: &custom::DnsSettings,
) -> Result<String, String> {
    if preset == DnsPreset::System && !settings.fake_ip {
        return Ok(envelope.to_string());
    }
    let mut envelope: Value = serde_json::from_str(envelope).map_err(|error| error.to_string())?;
    let has_china = envelope
        .pointer(&format!("/routing/rule_sets/{CHINA_RULE_SET}"))
        .is_some();
    let mut dns = match preset {
        DnsPreset::System => Some(json!({})),
        DnsPreset::Encrypted | DnsPreset::Split => Some(section(preset, has_china)),
        DnsPreset::Custom => settings.section(&envelope),
    };
    if settings.fake_ip {
        if let Some(Value::Object(dns)) = dns.as_mut() {
            dns.insert("fake_ip".to_string(), json!({ "enabled": true }));
        }
    }
    let object = envelope
        .as_object_mut()
        .ok_or("the envelope must be a JSON object")?;
    match dns {
        Some(dns) => object.insert("dns".to_string(), dns),
        None => object.remove("dns"),
    };
    Ok(envelope.to_string())
}

/// `envelope` refusing UDP to port 443 (the engine's `block_quic`) when
/// `on`, so apps fall back from QUIC to TCP where HTTPS decryption and domain
/// rules see them. Off removes the key, so the envelope still suits engines
/// without it.
pub fn apply_block_quic(envelope: &str, on: bool) -> Result<String, String> {
    let mut value: Value = serde_json::from_str(envelope).map_err(|error| error.to_string())?;
    let object = value
        .as_object_mut()
        .ok_or("the envelope must be a JSON object")?;
    if on {
        object.insert("block_quic".to_string(), Value::Bool(true));
    } else if object.remove("block_quic").is_none() {
        return Ok(envelope.to_string());
    }
    Ok(value.to_string())
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
        for preset in [
            DnsPreset::System,
            DnsPreset::Encrypted,
            DnsPreset::Split,
            DnsPreset::Custom,
        ] {
            assert_eq!(DnsPreset::parse(preset.as_str()), Some(preset));
        }
        assert_eq!(DnsPreset::parse("custom"), Some(DnsPreset::Custom));
        assert_eq!(DnsPreset::parse("fake-ip"), None);
    }

    #[test]
    fn custom_and_fake_ip_use_the_given_settings() {
        let settings = custom::DnsText {
            servers: "a = 1.1.1.1".to_string(),
            fake_ip: true,
            ..custom::DnsText::default()
        }
        .parse()
        .unwrap();
        let custom: Value =
            serde_json::from_str(&apply_with(SINGLE, DnsPreset::Custom, &settings).unwrap())
                .unwrap();
        assert_eq!(custom["dns"]["default"], "a");
        assert_eq!(custom["dns"]["fake_ip"]["enabled"], true);
        let system: Value =
            serde_json::from_str(&apply_with(SINGLE, DnsPreset::System, &settings).unwrap())
                .unwrap();
        assert_eq!(system["dns"], json!({ "fake_ip": { "enabled": true } }));
        let split: Value =
            serde_json::from_str(&apply_with(SINGLE, DnsPreset::Split, &settings).unwrap())
                .unwrap();
        assert_eq!(split["dns"]["default"], "cloudflare");
        assert_eq!(split["dns"]["fake_ip"]["enabled"], true);
        // Nothing set: the engine keeps its default resolver.
        let empty = custom::DnsSettings::default();
        let plain: Value =
            serde_json::from_str(&apply_with(SINGLE, DnsPreset::Custom, &empty).unwrap()).unwrap();
        assert!(plain.get("dns").is_none());
    }

    #[test]
    fn block_quic_is_written_only_when_on() {
        assert_eq!(apply_block_quic(SINGLE, false).unwrap(), SINGLE);
        let on: Value = serde_json::from_str(&apply_block_quic(SINGLE, true).unwrap()).unwrap();
        assert_eq!(on["block_quic"], Value::Bool(true));
        let off: Value =
            serde_json::from_str(&apply_block_quic(&on.to_string(), false).unwrap()).unwrap();
        assert!(off.get("block_quic").is_none());
        assert!(apply_block_quic("[]", true).is_err());
    }
}
