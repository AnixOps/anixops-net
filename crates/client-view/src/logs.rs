//! The log viewer: which of the app's own log lines show, the modules to pick from, and the text a copy
//! puts on the clipboard.

use serde_json::{json, Value};

use crate::subscriptions::date;

fn text<'a>(entry: &'a Value, name: &str) -> &'a str {
    entry.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// `info` 0, `warn` 1, `error` 2; anything else counts as `info`.
fn rank(level: &str) -> u8 {
    match level {
        "warn" => 1,
        "error" => 2,
        _ => 0,
    }
}

/// A time in Unix milliseconds as ISO 8601 in UTC, `1970-01-01T00:00:00.003Z`.
fn iso_time(ms: u64) -> String {
    let seconds = ms / 1000;
    let of_day = seconds % 86_400;
    format!(
        "{}T{:02}:{:02}:{:02}.{:03}Z",
        date(seconds),
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60,
        ms % 1000
    )
}

/// One line as the copy writes it: time (`-` when the line has none), level, module and message.
fn line(entry: &Value) -> String {
    let time = match entry.get("timeMs").and_then(Value::as_u64) {
        Some(ms) if ms > 0 => iso_time(ms),
        _ => "-".to_string(),
    };
    format!(
        "{time} {} [{}] {}",
        text(entry, "level").to_uppercase(),
        text(entry, "module"),
        text(entry, "message")
    )
}

/// The log viewer's page for the core's `app_logs` lines (oldest first, `{ timeMs, module, level, message }`):
///
/// - `modules`: the modules present, in the order they first appear.
/// - `shown`: the lines at or above `level` (`all`, `warn` or `error`; anything else is `all`), of `module`
///   (empty for all) and containing `query` (trimmed, any case), newest first, passed through as they came.
/// - `text`: the shown lines as plain text, oldest first, one per line, for copying.
pub fn logs(entries: &[Value], level: &str, module: &str, query: &str) -> Value {
    let mut modules: Vec<&str> = Vec::new();
    for entry in entries {
        let name = text(entry, "module");
        if !modules.contains(&name) {
            modules.push(name);
        }
    }
    let floor = rank(level);
    let needle = query.trim().to_lowercase();
    let shown: Vec<&Value> = entries
        .iter()
        .filter(|entry| rank(text(entry, "level")) >= floor)
        .filter(|entry| module.is_empty() || text(entry, "module") == module)
        .filter(|entry| {
            needle.is_empty() || text(entry, "message").to_lowercase().contains(&needle)
        })
        .rev()
        .collect();
    let copy = shown
        .iter()
        .rev()
        .copied()
        .map(line)
        .collect::<Vec<_>>()
        .join("\n");
    json!({ "modules": modules, "shown": shown, "text": copy })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_iso_in_utc() {
        assert_eq!(iso_time(3), "1970-01-01T00:00:00.003Z");
        assert_eq!(iso_time(1_763_198_400_123), "2025-11-15T09:20:00.123Z");
    }
}
