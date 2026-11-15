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

/// The DNS choices. Public builds ship no preset per-domain rules (docs/no-presets.md), so the 国内外分流
/// choice is gone: a state saved with it reads as [`DnsPreset::Split`] until [`adopt_legacy_split`] turns it
/// into the user's own "自定义" settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsPreset {
    /// No `dns` section: the engine's built-in default.
    #[default]
    System,
    /// Everything over Cloudflare DNS over HTTPS, through the proxy.
    Encrypted,
    /// Only read from a state an earlier version saved; never offered and never applied as a preset.
    Split,
    /// The user's own servers, per-domain servers and hosts ([`custom`]).
    Custom,
}

impl DnsPreset {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Encrypted => "encrypted",
            // Shown as what it is applied as.
            Self::Split | Self::Custom => "custom",
        }
    }

    /// A choice a client asks for; the removed 国内外分流 (`split`) is refused.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "system" => Some(Self::System),
            "encrypted" => Some(Self::Encrypted),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// The servers and per-domain servers the removed 国内外分流 choice used, as "自定义" text: what a user who had
/// picked it gets as their own settings, so their DNS keeps working the same way.
const LEGACY_SPLIT_SERVERS: &str = "cloudflare = https://1.1.1.1/dns-query, server-name=cloudflare-dns.com\nalidns = https://223.5.5.5/dns-query, server-name=dns.alidns.com, direct\n";
const LEGACY_SPLIT_RULES: &str = "DOMAIN-SUFFIX,cn,alidns\n";

/// `preset`, with the removed 国内外分流 choice of an earlier version turned into the user's own "自定义"
/// settings: the same servers and `.cn` rule are written as their text when they have no servers of their own
/// yet (their hosts and Fake-IP switch stay); when they do, their own settings are kept as they are. Call it
/// once the settings file is known ([`custom::use_file`]) and save the state with the answer.
pub fn adopt_legacy_split(preset: DnsPreset) -> Result<DnsPreset, String> {
    if preset != DnsPreset::Split {
        return Ok(preset);
    }
    let mut text = custom::text()?;
    if text.servers.trim().is_empty() && text.rules.trim().is_empty() {
        text.servers = LEGACY_SPLIT_SERVERS.to_string();
        text.rules = LEGACY_SPLIT_RULES.to_string();
        custom::replace(&text, || Ok(()))?;
    }
    Ok(DnsPreset::Custom)
}

/// `envelope` with the choice's `dns` section in place of its own, and
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
    let mut dns = match preset {
        DnsPreset::System => Some(json!({})),
        DnsPreset::Encrypted => Some(json!({
            "servers": [{
                "name": "cloudflare",
                "url": "https://1.1.1.1/dns-query",
                "server_name": "cloudflare-dns.com",
            }],
            "default": "cloudflare",
        })),
        // A legacy split not yet adopted is the user's own settings, never a preset.
        DnsPreset::Split | DnsPreset::Custom => settings.section(&envelope),
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
    fn a_legacy_split_is_never_applied_as_a_preset() {
        // Without settings of the user's it adds nothing: no built-in servers or per-domain rules.
        let empty = custom::DnsSettings::default();
        let plain: Value =
            serde_json::from_str(&apply_with(SINGLE, DnsPreset::Split, &empty).unwrap()).unwrap();
        assert!(plain.get("dns").is_none());
        assert_eq!(DnsPreset::parse("split"), None);
        assert_eq!(DnsPreset::Split.as_str(), "custom");
        let saved: DnsPreset = serde_json::from_str(r#""split""#).unwrap();
        assert_eq!(saved, DnsPreset::Split);
    }

    #[test]
    fn the_legacy_split_text_parses_as_custom_settings() {
        let settings = custom::DnsText {
            servers: LEGACY_SPLIT_SERVERS.to_string(),
            rules: LEGACY_SPLIT_RULES.to_string(),
            ..custom::DnsText::default()
        }
        .parse()
        .unwrap();
        let envelope: Value =
            serde_json::from_str(&apply_with(SINGLE, DnsPreset::Custom, &settings).unwrap())
                .unwrap();
        assert_eq!(envelope["dns"]["default"], "cloudflare");
        assert_eq!(envelope["dns"]["servers"][1]["via"], "direct");
        assert_eq!(envelope["dns"]["rules"][0]["domain_suffix"], json!(["cn"]));
        assert_eq!(envelope["dns"]["rules"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn names_round_trip() {
        for preset in [DnsPreset::System, DnsPreset::Encrypted, DnsPreset::Custom] {
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
        let legacy: Value =
            serde_json::from_str(&apply_with(SINGLE, DnsPreset::Split, &settings).unwrap())
                .unwrap();
        assert_eq!(legacy["dns"]["default"], "a");
        assert_eq!(legacy["dns"]["fake_ip"]["enabled"], true);
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
