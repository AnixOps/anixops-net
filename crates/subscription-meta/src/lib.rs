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

/// The first retry after a failed automatic update waits this long; each further failure in a row doubles the
/// wait, which never grows past the subscription's own interval.
pub const RETRY_AFTER_FAILURE_SECS: u64 = 15 * 60;

/// What the schedule of one subscription's automatic updates is made of. The desktop clients keep these per
/// subscription, so the time they show, the time the updater acts on and the time after a restart are one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UpdateSchedule {
    /// The last successful update, Unix seconds; `None` when there was none.
    pub last_success: Option<u64>,
    /// The last attempt, successful or not, Unix seconds.
    pub last_attempt: Option<u64>,
    /// Failed attempts since the last success.
    pub failures: u32,
    /// Hours between updates; 0 means by hand only.
    pub interval_hours: u32,
}

impl UpdateSchedule {
    /// When the next automatic update may run, Unix seconds: `interval_hours` after the last success (at once when
    /// there was none), and after a failure not before the last attempt plus the retry wait
    /// ([`RETRY_AFTER_FAILURE_SECS`], doubled for each further failure, at most the interval). `None` when updates
    /// are by hand only.
    pub fn next_update(&self) -> Option<u64> {
        if self.interval_hours == 0 {
            return None;
        }
        let interval = u64::from(self.interval_hours) * 3600;
        let regular = self
            .last_success
            .map_or(0, |last| last.saturating_add(interval));
        match (self.failures, self.last_attempt) {
            (0, _) | (_, None) => Some(regular),
            (failures, Some(attempt)) => {
                let doublings = (failures - 1).min(20);
                let wait = RETRY_AFTER_FAILURE_SECS
                    .saturating_mul(1 << doublings)
                    .min(interval);
                Some(regular.max(attempt.saturating_add(wait)))
            }
        }
    }

    /// Whether an automatic update is due at `now` (Unix seconds).
    pub fn is_due(&self, now: u64) -> bool {
        self.next_update().is_some_and(|at| now >= at)
    }
}

/// Whether a failed automatic update is worth a notice: only the first failure of a run of failures (`failures`
/// counts this one), so a subscription that keeps failing is reported once until it works again.
pub fn notify_failure(failures: u32) -> bool {
    failures == 1
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
                Some("attachment; filename*=UTF-8''%E6%9C%8D%E5%8A%A1%E5%95%86.yaml".into())
            }
            _ => None,
        });
        assert_eq!(meta.user_info.and_then(|info| info.total), Some(3));
        assert_eq!(meta.update_interval_hours, Some(12));
        assert_eq!(meta.title.as_deref(), Some("服务商"));

        let titled = ResponseMeta::from_headers(|name| match name {
            "profile-title" => Some("base64:5pyN5Yqh5ZWGQg==".into()),
            "content-disposition" => Some("attachment; filename=\"x.txt\"".into()),
            _ => None,
        });
        assert_eq!(titled.title.as_deref(), Some("服务商B"));
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
        let schedule = |last_success, interval_hours| UpdateSchedule {
            last_success,
            last_attempt: last_success,
            failures: 0,
            interval_hours,
        };
        assert!(!schedule(Some(1000), 0).is_due(u64::MAX));
        assert_eq!(schedule(Some(1000), 0).next_update(), None);
        assert!(schedule(None, 24).is_due(0));
        assert!(!schedule(Some(1000), 1).is_due(1000 + 3599));
        assert!(schedule(Some(1000), 1).is_due(1000 + 3600));
    }

    #[test]
    fn failed_updates_back_off_instead_of_running_every_check() {
        // Due since long ago, then the attempt at 100_000 failed: the next one waits the retry time, not the
        // updater's next one-minute check.
        let mut schedule = UpdateSchedule {
            last_success: Some(1000),
            last_attempt: Some(100_000),
            failures: 1,
            interval_hours: 24,
        };
        assert!(!schedule.is_due(100_000 + 60));
        assert_eq!(
            schedule.next_update(),
            Some(100_000 + RETRY_AFTER_FAILURE_SECS)
        );
        // Each failure in a row doubles the wait.
        schedule.failures = 3;
        assert_eq!(
            schedule.next_update(),
            Some(100_000 + 4 * RETRY_AFTER_FAILURE_SECS)
        );
        // The wait never exceeds the interval, however many failures.
        schedule.failures = 40;
        assert_eq!(schedule.next_update(), Some(100_000 + 24 * 3600));
        schedule.interval_hours = 1;
        assert_eq!(schedule.next_update(), Some(100_000 + 3600));
        // A failure never brings the regular time forward.
        let early = UpdateSchedule {
            last_success: Some(100_000),
            last_attempt: Some(100_010),
            failures: 1,
            interval_hours: 24,
        };
        assert_eq!(early.next_update(), Some(100_000 + 24 * 3600));
        // A subscription never updated and failing waits too.
        let never = UpdateSchedule {
            last_success: None,
            last_attempt: Some(500),
            failures: 2,
            interval_hours: 24,
        };
        assert_eq!(
            never.next_update(),
            Some(500 + 2 * RETRY_AFTER_FAILURE_SECS)
        );
    }

    #[test]
    fn only_the_first_failure_in_a_row_is_notified() {
        assert!(notify_failure(1));
        assert!(!notify_failure(2));
        assert!(!notify_failure(0));
    }
}
