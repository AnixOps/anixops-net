//! What a subscription server reports besides the node list, as Loon, Clash and Shadowrocket read it:
//! traffic and expiry (`subscription-userinfo`), a title (`profile-title` or the `content-disposition` file
//! name) and the update interval it suggests (`profile-update-interval`). The desktop clients keep this per
//! subscription; the iOS app has the same rules in Swift (`SubscriptionGroup.swift`).

use serde::{Deserialize, Serialize};

/// Hours between automatic updates when the server suggests none.
pub const DEFAULT_UPDATE_INTERVAL_HOURS: u32 = 24;
/// Longest interval accepted, from the server or the user: 30 days.
pub const MAX_UPDATE_INTERVAL_HOURS: u32 = 24 * 30;
/// Longest subscription name kept.
pub const MAX_NAME_CHARS: usize = 64;

/// Traffic in bytes and expiry in Unix seconds, for example
/// `upload=1234; download=5678; total=107374182400; expire=1798761600`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserInfo {
    pub upload: Option<u64>,
    pub download: Option<u64>,
    pub total: Option<u64>,
    /// Unix seconds; `expire=0` means none.
    pub expire: Option<u64>,
}

impl UserInfo {
    /// `None` when the header carries none of the four fields.
    pub fn parse(header: &str) -> Option<Self> {
        let mut info = Self::default();
        for field in header.split(';') {
            let Some((key, value)) = field.split_once('=') else {
                continue;
            };
            let Ok(number) = value.trim().parse::<f64>() else {
                continue;
            };
            if !number.is_finite() || number < 0.0 {
                continue;
            }
            // Saturates at u64::MAX.
            let number = number as u64;
            match key.trim().to_ascii_lowercase().as_str() {
                "upload" => info.upload = Some(number),
                "download" => info.download = Some(number),
                "total" => info.total = Some(number),
                "expire" => info.expire = (number > 0).then_some(number),
                _ => {}
            }
        }
        (info != Self::default()).then_some(info)
    }

    /// Upload plus download, when the server sent either.
    pub fn used(&self) -> Option<u64> {
        (self.upload.is_some() || self.download.is_some()).then(|| {
            self.upload
                .unwrap_or(0)
                .saturating_add(self.download.unwrap_or(0))
        })
    }
}

/// The headers of one subscription download.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResponseMeta {
    pub user_info: Option<UserInfo>,
    /// `profile-update-interval`, in hours.
    pub update_interval_hours: Option<u32>,
    /// `profile-title`, or the file name from `content-disposition`.
    pub title: Option<String>,
}

impl ResponseMeta {
    /// `header` looks a response header up by name, ignoring case.
    pub fn from_headers(header: impl Fn(&str) -> Option<String>) -> Self {
        Self {
            user_info: header("subscription-userinfo")
                .as_deref()
                .and_then(UserInfo::parse),
            update_interval_hours: header("profile-update-interval")
                .as_deref()
                .and_then(parse_hours),
            title: header("profile-title")
                .as_deref()
                .and_then(profile_title)
                .or_else(|| header("content-disposition").as_deref().and_then(file_name)),
        }
    }
}

fn parse_hours(value: &str) -> Option<u32> {
    value
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|hours| *hours > 0)
        .map(|hours| hours.min(MAX_UPDATE_INTERVAL_HOURS))
}

/// Plain text, or `base64:` followed by base64 UTF-8.
fn profile_title(value: &str) -> Option<String> {
    let value = value.trim();
    match value.get(..7) {
        Some(prefix) if prefix.eq_ignore_ascii_case("base64:") => {
            clean_name(&String::from_utf8(base64_decode(&value[7..])?).ok()?)
        }
        _ => clean_name(value),
    }
}

/// `filename*=UTF-8''<percent-encoded>` wins over `filename="<name>"`; the extension is dropped.
fn file_name(value: &str) -> Option<String> {
    let mut plain = None;
    let mut extended = None;
    for part in value.split(';') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "filename*" => {
                let encoded = value.rsplit("''").next().unwrap_or(value);
                extended = percent_decode(encoded);
            }
            "filename" => plain = Some(value.trim_matches('"').to_string()),
            _ => {}
        }
    }
    let name = extended.or(plain)?;
    let stem = match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name.as_str(),
    };
    clean_name(stem)
}

/// Trims a name and cuts it to [`MAX_NAME_CHARS`]; `None` when nothing is left.
pub fn clean_name(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.chars().take(MAX_NAME_CHARS).collect())
}

/// What to call a subscription: its saved name, else the link's host, else the location itself.
pub fn display_name(name: Option<&str>, location: &str) -> String {
    if let Some(name) = name.and_then(clean_name) {
        return name;
    }
    location
        .split_once("://")
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .map(|authority| authority.rsplit('@').next().unwrap_or(authority))
        .filter(|host| !host.is_empty())
        .unwrap_or(location)
        .to_string()
}

/// Whether an automatic update is due: `interval_hours` 0 means by hand only, and a subscription never
/// updated successfully is due at once.
pub fn is_due(last_success_secs: Option<u64>, interval_hours: u32, now_secs: u64) -> bool {
    if interval_hours == 0 {
        return false;
    }
    match last_success_secs {
        Some(last) => now_secs >= last.saturating_add(u64::from(interval_hours) * 3600),
        None => true,
    }
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = value.get(index + 1..index + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Standard or URL-safe base64, padding optional.
fn base64_decode(value: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(value.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in value.trim().bytes() {
        let sextet = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(sextet);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_info_header_parses() {
        let info = UserInfo::parse(
            "upload=1073741824; download=2147483648; total=10737418240; expire=1798761600",
        )
        .unwrap();
        assert_eq!(info.used(), Some(3_221_225_472));
        assert_eq!(info.total, Some(10_737_418_240));
        assert_eq!(info.expire, Some(1_798_761_600));
        let loose = UserInfo::parse("Upload=1.0e3 ;download=0;total=0;expire=0").unwrap();
        assert_eq!(loose.used(), Some(1000));
        assert_eq!(loose.expire, None);
        assert_eq!(UserInfo::parse("nothing=1"), None);
        assert_eq!(UserInfo::parse(""), None);
    }

    #[test]
    fn headers_give_title_and_interval() {
        let meta = ResponseMeta::from_headers(|name| match name {
            "subscription-userinfo" => Some("upload=1; download=2; total=3".into()),
            "profile-update-interval" => Some("12".into()),
            "content-disposition" => {
                Some("attachment; filename*=UTF-8''%E6%9C%BA%E5%9C%BA.yaml".into())
            }
            _ => None,
        });
        assert_eq!(meta.user_info.and_then(|info| info.total), Some(3));
        assert_eq!(meta.update_interval_hours, Some(12));
        assert_eq!(meta.title.as_deref(), Some("机场"));

        let titled = ResponseMeta::from_headers(|name| match name {
            "profile-title" => Some("base64:5py65Zy6Qg==".into()),
            "content-disposition" => Some("attachment; filename=\"x.txt\"".into()),
            _ => None,
        });
        assert_eq!(titled.title.as_deref(), Some("机场B"));
        let plain = ResponseMeta::from_headers(|name| {
            (name == "content-disposition").then(|| "attachment; filename=\"Sub.txt\"".to_string())
        });
        assert_eq!(plain.title.as_deref(), Some("Sub"));
        let zero = ResponseMeta::from_headers(|name| {
            (name == "profile-update-interval").then(|| "0".to_string())
        });
        assert_eq!(zero.update_interval_hours, None);
        assert_eq!(
            ResponseMeta::from_headers(|_| None),
            ResponseMeta::default()
        );
    }

    #[test]
    fn names_fall_back_to_the_host() {
        assert_eq!(display_name(Some(" My "), "https://a.example/x"), "My");
        assert_eq!(
            display_name(None, "https://user@sub.example:8443/api?token=t"),
            "sub.example:8443"
        );
        assert_eq!(
            display_name(Some("  "), "/home/me/sub.yaml"),
            "/home/me/sub.yaml"
        );
    }

    #[test]
    fn due_follows_the_interval() {
        assert!(!is_due(Some(1000), 0, u64::MAX));
        assert!(is_due(None, 24, 0));
        assert!(!is_due(Some(1000), 1, 1000 + 3599));
        assert!(is_due(Some(1000), 1, 1000 + 3600));
    }
}
