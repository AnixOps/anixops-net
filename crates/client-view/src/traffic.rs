//! The traffic statistics card: the period's totals, its bar chart, and the hosts and policies that moved
//! the most bytes.

use std::collections::BTreeMap;

use serde_json::{json, Value};

const HOUR: i64 = 3_600;
const DAY: i64 = 86_400;

/// How many hosts or policies the lists keep.
const SHARES: usize = 20;

fn number(hour: &Value, name: &str) -> i64 {
    hour.get(name).and_then(Value::as_i64).unwrap_or(0)
}

/// Unix seconds of the start of the period in the window's time zone: today's midnight, or six days before
/// it for `week`. `offset` is that zone's distance from UTC in seconds (east positive) at `now`.
fn period_start(period: &str, now: i64, offset: i64) -> i64 {
    let local = now + offset;
    let midnight = local - local.rem_euclid(DAY) - offset;
    if period == "week" {
        midnight - 6 * DAY
    } else {
        midnight
    }
}

/// The names under `key` (`hosts` or `policies`) by bytes, largest first, ties by name (code points).
fn shares(hours: &[&Value], key: &str) -> Vec<Value> {
    let mut sums: BTreeMap<&str, i64> = BTreeMap::new();
    for hour in hours {
        let Some(names) = hour.get(key).and_then(Value::as_object) else {
            continue;
        };
        for (name, bytes) in names {
            *sums.entry(name.as_str()).or_default() += bytes.as_i64().unwrap_or(0);
        }
    }
    let mut rows: Vec<(&str, i64)> = sums.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    rows.into_iter()
        .take(SHARES)
        .map(|(name, bytes)| json!({ "name": name, "bytes": bytes }))
        .collect()
}

/// The card for the core's `traffic_stats` hours (`[{ start, upload, download, hosts, policies }]`, oldest
/// first) over `period` (`today`, or `week` for the last seven days) at `now` (Unix seconds) in the window's
/// time zone `offset` seconds east of UTC:
///
/// - `start`: when the period starts. Hours before it are left out of everything below.
/// - `upload`, `download`: the period's bytes.
/// - `bars`: `[{ start, bytes }]`, the 24 hours of today or the 7 days of the week, oldest first; each hour's
///   bytes (up and down) land in the last bar that starts at or before it.
/// - `hosts`, `policies`: `[{ name, bytes }]`, the 20 that moved the most bytes. The host `*` stands for
///   all the hosts the core did not keep by name; the window writes its own words for it.
///
/// The bars step a fixed hour or day from the start, so on a day the clocks change one bar is an hour off.
pub fn traffic_stats(hours: &[Value], period: &str, now: i64, offset: i64) -> Value {
    let start = period_start(period, now, offset);
    let kept: Vec<&Value> = hours
        .iter()
        .filter(|hour| number(hour, "start") >= start)
        .collect();
    let (count, step) = if period == "week" {
        (7, DAY)
    } else {
        (24, HOUR)
    };
    let mut bars: Vec<(i64, i64)> = (0..count).map(|index| (start + index * step, 0)).collect();
    for hour in &kept {
        let at = number(hour, "start");
        let index = bars.iter().rposition(|bar| bar.0 <= at).unwrap_or(0);
        bars[index].1 += number(hour, "upload") + number(hour, "download");
    }
    json!({
        "start": start,
        "upload": kept.iter().map(|hour| number(hour, "upload")).sum::<i64>(),
        "download": kept.iter().map(|hour| number(hour, "download")).sum::<i64>(),
        "bars": bars.into_iter().map(|(start, bytes)| json!({ "start": start, "bytes": bytes })).collect::<Vec<_>>(),
        "hosts": shares(&kept, "hosts"),
        "policies": shares(&kept, "policies"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periods_start_at_local_midnight() {
        // 2026-11-14 15:30 in UTC+8 is 07:30 UTC; local midnight is 2026-11-13 16:00 UTC.
        let now = 1_794_641_400;
        assert_eq!(period_start("today", now, 8 * HOUR), 1_794_585_600);
        assert_eq!(period_start("week", now, 8 * HOUR), 1_794_585_600 - 6 * DAY);
        // West of UTC: 02:30 on the 14th, so the day started at 05:00 UTC.
        assert_eq!(period_start("today", now, -5 * HOUR), 1_794_632_400);
    }
}
