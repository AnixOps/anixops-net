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

/// Whether the engine may still fill in this request's response: its `state` says, and a row from a kernel
/// without `state` is under way while it has no status, no end and no note.
fn pending(request: &Value) -> bool {
    match request.get("state").and_then(Value::as_str) {
        Some(state) => state == "in_progress",
        None => {
            number(request, "status").is_none()
                && number(request, "ended_at_ms").is_none()
                && text(request, "note").is_empty()
        }
    }
}

/// Where the next read by `since_seq` starts (kernels without `since_rev`): before the oldest request still
/// waiting for its response, so it comes back with the response, or after the newest one seen.
pub fn next_since(rows: &[Value], last_seq: u64) -> u64 {
    rows.iter()
        .find(|row| seq(row) > 0 && pending(row))
        .map_or(last_seq, |row| seq(row).saturating_sub(1))
}

/// The `since_rev` of the next read after `answer`, or `None` for a kernel without `since_rev`: a full page
/// (`page` rows) carries on from its last row, since the engine lists in the order things changed;
/// otherwise the read starts after the newest change.
pub fn next_rev(current: Option<u64>, answer: &Value, page: usize) -> Option<u64> {
    let last_rev = answer.get("last_rev").and_then(Value::as_u64)?;
    let rows = answer
        .get("requests")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let last = if page > 0 && rows.len() >= page {
        rows.last()
            .and_then(|row| row.get("rev"))
            .and_then(Value::as_u64)
    } else {
        None
    };
    Some(current.unwrap_or(0).max(last.unwrap_or(last_rev)))
}

/// A request's place in the capture: its connection and its place there, the same while it is under way
/// (and has no `seq` yet) and after.
fn key(request: &Value) -> (i64, i64) {
    (
        number(request, "connection_id").unwrap_or(0),
        number(request, "index").unwrap_or(0),
    )
}

/// The order of the list: when requests started, then by connection and place.
fn arrival(request: &Value) -> (i64, i64, i64) {
    let (connection, index) = key(request);
    (
        number(request, "started_at_ms").unwrap_or(0),
        connection,
        index,
    )
}

/// The list after one answer of the engine: newer copies of a request replace older ones (a request under
/// way is updated in place as its response arrives), ended requests the engine pushed out leave, oldest
/// first, and no more than [`MAX_REQUESTS`] stay. A request still under way has no `seq` yet; if its
/// connection went, it is simply not updated again.
pub fn merge(rows: &[Value], answer: &Value) -> Vec<Value> {
    let mut by_key: BTreeMap<(i64, i64), &Value> = rows.iter().map(|row| (key(row), row)).collect();
    if let Some(requests) = answer.get("requests").and_then(Value::as_array) {
        for request in requests {
            by_key.insert(key(request), request);
        }
    }
    let dropped = answer
        .get("dropped_before_seq")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut merged: Vec<&Value> = by_key
        .into_values()
        .filter(|row| seq(row) == 0 || seq(row) > dropped)
        .collect();
    merged.sort_by_key(|row| arrival(row));
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

/// Compares two requests for the list. `Seq` is the time column: when requests started, so a request still
/// under way (no `seq` yet) has its place too. Ties keep that order, and requests without a value in the
/// column come last whichever way the list runs.
pub fn compare(a: &Value, b: &Value, by: Column, descending: bool) -> Ordering {
    let directed = |order: Ordering| if descending { order.reverse() } else { order };
    let by_seq = directed(arrival(a).cmp(&arrival(b)));
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

/// The requests in the list's order, each with its `path` and status `tone` added: what a window that
/// cannot afford a command per row (one second's poll of up to [`MAX_REQUESTS`]) shows.
pub fn rows(rows: &[Value], by: Column, descending: bool) -> Vec<Value> {
    sort(rows, by, descending)
        .into_iter()
        .map(|mut row| {
            let path = path(&row);
            let tone = status_tone(&row);
            if let Some(fields) = row.as_object_mut() {
                fields.insert("path".into(), Value::String(path));
                fields.insert("tone".into(), Value::String(tone.into()));
            }
            row
        })
        .collect()
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
            .map(|seq| serde_json::json!({ "seq": seq, "connection_id": 1, "index": seq }))
            .collect();
        let merged = merge(&[], &serde_json::json!({ "requests": many }));
        assert_eq!(merged.len(), MAX_REQUESTS);
        assert_eq!(seq(&merged[0]), 6);
    }

    #[test]
    fn a_request_under_way_is_updated_in_place() {
        let under_way = serde_json::json!({
            "seq": 0, "connection_id": 2, "index": 0, "started_at_ms": 5, "state": "in_progress", "rev": 1,
        });
        let older = serde_json::json!({
            "seq": 1, "connection_id": 1, "index": 0, "started_at_ms": 1, "state": "done", "rev": 2,
        });
        let done = serde_json::json!({
            "seq": 2, "connection_id": 2, "index": 0, "started_at_ms": 5, "state": "done", "rev": 3,
        });
        let first = merge(&[], &serde_json::json!({ "requests": [under_way, older] }));
        assert_eq!(first.len(), 2);
        assert_eq!(key(&first[0]), (1, 0));
        // The engine pushed out seq 1; the row still under way has no seq and stays until it ends.
        let next = merge(&first, &serde_json::json!({ "requests": [done], "dropped_before_seq": 1 }));
        assert_eq!(next, vec![done]);
    }

    #[test]
    fn the_next_read_follows_the_newest_change() {
        let answer = serde_json::json!({ "requests": [{ "rev": 7 }, { "rev": 9 }], "last_rev": 12 });
        // A full page carries on from its last row; a short one from the newest change.
        assert_eq!(next_rev(Some(3), &answer, 2), Some(9));
        assert_eq!(next_rev(Some(3), &answer, 10), Some(12));
        assert_eq!(next_rev(Some(20), &answer, 10), Some(20));
        assert_eq!(next_rev(None, &serde_json::json!({ "requests": [] }), 10), None);
    }
}
