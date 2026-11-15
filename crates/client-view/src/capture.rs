//! The captured-requests list: the filter sent to the engine, keeping the list up to date, and its order.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde_json::Value;
use url::Url;

/// Most requests the list keeps; older ones fall off the top.
pub const MAX_REQUESTS: usize = 2000;

/// One-tap filters, by id: each adds a condition of the engine's filter expression. The window names them.
pub const QUICK_FILTERS: [(&str, &str); 5] = [
    ("errors", "status:>=400"),
    ("json", "type:json"),
    ("images", "type:image"),
    ("h2", "version:h2"),
    ("large", "size:>100k"),
];

/// The expression sent to the engine: the conditions of the quick filters in `quick` and what the user
/// typed, all of which must hold. Typed `OR` groups each get the quick conditions, since the expression
/// has no parentheses.
pub fn combine_filter(quick: &[&str], typed: &str) -> String {
    let conditions: Vec<&str> = QUICK_FILTERS
        .iter()
        .filter(|(id, _)| quick.contains(id))
        .map(|(_, condition)| *condition)
        .collect();
    let conditions = conditions.join(" ");
    let typed = typed.trim();
    if conditions.is_empty() {
        return typed.to_string();
    }
    if typed.is_empty() {
        return conditions;
    }
    split_or(typed)
        .iter()
        .map(|group| format!("{conditions} {group}"))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// The `OR` groups of an expression. A quoted value (closed or running to the end) is one word, so an
/// `OR` inside double quotes never splits.
pub fn split_or(text: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let mut current = String::new();
    let mut rest = text;
    while let Some(first) = rest.chars().next() {
        let length = if first == '"' {
            rest[1..].find('"').map_or(rest.len(), |end| end + 2)
        } else if first.is_whitespace() {
            rest.find(|character: char| !character.is_whitespace())
                .unwrap_or(rest.len())
        } else {
            rest.find(|character: char| character.is_whitespace() || character == '"')
                .unwrap_or(rest.len())
        };
        let (word, after) = rest.split_at(length);
        rest = after;
        if word == "OR" {
            groups.push(std::mem::take(&mut current));
        } else {
            current.push_str(word);
        }
    }
    groups.push(current);
    groups
        .iter()
        .map(|group| group.trim().to_string())
        .filter(|group| !group.is_empty())
        .collect()
}

fn number(request: &Value, name: &str) -> Option<i64> {
    request.get(name).and_then(Value::as_i64)
}

fn text<'a>(request: &'a Value, name: &str) -> &'a str {
    request
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn seq(request: &Value) -> u64 {
    request.get("seq").and_then(Value::as_u64).unwrap_or(0)
}

/// Whether the engine may still fill in this request's response.
fn pending(request: &Value) -> bool {
    number(request, "status").is_none()
        && number(request, "ended_at_ms").is_none()
        && text(request, "note").is_empty()
}

/// Where the next read starts: before the oldest request still waiting for its response, so it comes back
/// with the response, or after the newest one seen.
pub fn next_since(rows: &[Value], last_seq: u64) -> u64 {
    rows.iter()
        .find(|row| pending(row))
        .map_or(last_seq, |row| seq(row).saturating_sub(1))
}

/// The list after one answer of the engine: newer copies replace older ones, requests the engine pushed
/// out leave, oldest first, and no more than [`MAX_REQUESTS`] stay.
pub fn merge(rows: &[Value], answer: &Value) -> Vec<Value> {
    let mut by_seq: BTreeMap<u64, &Value> = rows.iter().map(|row| (seq(row), row)).collect();
    if let Some(requests) = answer.get("requests").and_then(Value::as_array) {
        for request in requests {
            by_seq.insert(seq(request), request);
        }
    }
    let dropped = answer
        .get("dropped_before_seq")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let merged: Vec<&Value> = by_seq
        .into_iter()
        .filter(|(seq, _)| *seq > dropped)
        .map(|(_, row)| row)
        .collect();
    let skip = merged.len().saturating_sub(MAX_REQUESTS);
    merged.into_iter().skip(skip).cloned().collect()
}

/// The path and query of a request's URL, or the URL itself when it does not parse.
pub fn path(request: &Value) -> String {
    let raw = text(request, "url");
    match Url::parse(raw) {
        Ok(url) => match url.query().filter(|query| !query.is_empty()) {
            Some(query) => format!("{}?{query}", url.path()),
            None => url.path().to_string(),
        },
        Err(_) => raw.to_string(),
    }
}

/// The status column's colour: `success` for 2xx, `warning` for 3xx, `danger` for 4xx/5xx or a request
/// that ended without a response, `pending` while the response is on its way, and `neutral` otherwise
/// (the engine answered it itself).
pub fn status_tone(request: &Value) -> &'static str {
    match number(request, "status") {
        None if text(request, "note") == "answered_locally" => "neutral",
        None if pending(request) => "pending",
        None => "danger",
        Some(status) if status >= 400 => "danger",
        Some(status) if status >= 300 => "warning",
        Some(status) if status >= 200 => "success",
        Some(_) => "neutral",
    }
}

/// A column the request list can be ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Seq,
    Status,
    Host,
    Path,
    Size,
    Duration,
}

impl Column {
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "seq" => Self::Seq,
            "status" => Self::Status,
            "host" => Self::Host,
            "path" => Self::Path,
            "size" => Self::Size,
            "duration" => Self::Duration,
            _ => return None,
        })
    }
}

fn duration(request: &Value) -> Option<i64> {
    Some(number(request, "ended_at_ms")? - number(request, "started_at_ms")?)
}

/// Text in the list's order: without regard to case, then as written. Not the order of any one language;
/// a window that wants its user's language's order sorts the names itself.
pub(crate) fn compare_text(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// Compares two requests for the list. Ties keep the engine's order (`seq`), and requests without a value
/// in the column come last whichever way the list runs.
pub fn compare(a: &Value, b: &Value, by: Column, descending: bool) -> Ordering {
    let directed = |order: Ordering| if descending { order.reverse() } else { order };
    let by_seq = directed(seq(a).cmp(&seq(b)));
    let missing_last = |x: Option<i64>, y: Option<i64>| match (x, y) {
        (None, None) => by_seq,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => directed(x.cmp(&y)).then(by_seq),
    };
    match by {
        Column::Seq => by_seq,
        Column::Status => missing_last(number(a, "status"), number(b, "status")),
        Column::Size => missing_last(number(a, "response_size"), number(b, "response_size")),
        Column::Duration => missing_last(duration(a), duration(b)),
        Column::Host => directed(compare_text(text(a, "host"), text(b, "host"))).then(by_seq),
        Column::Path => directed(compare_text(&path(a), &path(b))).then(by_seq),
    }
}

/// The requests in the list's order.
pub fn sort(rows: &[Value], by: Column, descending: bool) -> Vec<Value> {
    let mut sorted: Vec<&Value> = rows.iter().collect();
    sorted.sort_by(|a, b| compare(a, b, by, descending));
    sorted.into_iter().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn or_splits_outside_quotes_only() {
        assert_eq!(split_or("a OR b c"), ["a", "b c"]);
        assert_eq!(
            split_or(r#"header:"x OR y" OR z"#),
            [r#"header:"x OR y""#, "z"]
        );
        assert_eq!(split_or("for ORDER"), ["for ORDER"]);
        // A quote left open runs to the end.
        assert_eq!(split_or(r#"a OR "b OR c"#), ["a", r#""b OR c"#]);
        assert_eq!(split_or("  OR  "), Vec::<String>::new());
    }

    #[test]
    fn the_list_is_capped_at_its_newest_requests() {
        let many: Vec<Value> = (1..=MAX_REQUESTS as u64 + 5)
            .map(|seq| serde_json::json!({ "seq": seq }))
            .collect();
        let merged = merge(&[], &serde_json::json!({ "requests": many }));
        assert_eq!(merged.len(), MAX_REQUESTS);
        assert_eq!(seq(&merged[0]), 6);
    }
}
