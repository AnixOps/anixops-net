//! The open connections of the overview page: what each one's row says, in which order, and which ones a
//! search leaves.

use serde_json::{json, Value};

fn text<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

fn number(value: &Value, name: &str) -> u64 {
    value.get(name).and_then(Value::as_u64).unwrap_or(0)
}

/// The port a destination ends with (`1.2.3.4:443`, `[::1]:443`, `example.com:443`), if it has one.
fn port(destination: &str) -> Option<&str> {
    let (address, port) = destination.rsplit_once(':')?;
    let bracketed = address.starts_with('[') && address.ends_with(']');
    ((bracketed || !address.contains(':')) && port.parse::<u16>().is_ok()).then_some(port)
}

/// One open connection as the page lists it, from an entry of the engine's `connections` answer:
///
/// - `id`: what the core is told when the connection is closed.
/// - `target`: where it goes, by name when the engine knows one (the requested domain, or the one sniffed
///   from TLS or HTTP) with the destination's port, else the destination as it was asked for.
/// - `network`: `tcp` or `udp`.
/// - `rule`: the rule that matched, as the engine names it (`DOMAIN-SUFFIX,google.com`, `FINAL`).
/// - `route`: the policy groups passed through and the exit, by name: a node's name for a node's id, the
///   id itself for a group and for the built-in exits. Never empty for a connection that has an exit.
/// - `upload`, `download`: bytes moved so far.
/// - `startedAtMs`: when it was opened, in milliseconds since 1970.
fn row(connection: &Value, name: &dyn Fn(&str) -> String) -> Value {
    let destination = text(connection, "destination").unwrap_or_default();
    let target = match (text(connection, "host"), port(destination)) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_owned(),
        (None, _) => destination.to_owned(),
    };
    let mut route: Vec<String> = connection
        .get("chain")
        .and_then(Value::as_array)
        .map(|chain| chain.iter().filter_map(Value::as_str).map(name).collect())
        .unwrap_or_default();
    if route.is_empty() {
        if let Some(exit) = text(connection, "outbound") {
            route.push(name(exit));
        }
    }
    json!({
        "id": number(connection, "id"),
        "target": target,
        "network": text(connection, "network").unwrap_or_default(),
        "rule": text(connection, "rule").unwrap_or_default(),
        "route": route,
        "upload": number(connection, "upload"),
        "download": number(connection, "download"),
        "startedAtMs": number(connection, "started_at_ms"),
    })
}

/// Whether a row has `query` (lower case) in where it goes, the rule that matched or the route it takes.
fn matches(row: &Value, query: &str) -> bool {
    let has = |text: &str| text.to_lowercase().contains(query);
    has(row["target"].as_str().unwrap_or_default())
        || has(row["rule"].as_str().unwrap_or_default())
        || row["route"]
            .as_array()
            .is_some_and(|route| route.iter().filter_map(Value::as_str).any(has))
}

/// What the page's list of connections shows, from the engine's answer to `{"cmd":"connections"}` (`null`
/// while the engine is not running) and the core's nodes:
///
/// `{ "running", "total", "connections": [row, …] }`: whether there is an engine to ask, how many
/// connections are open, and the ones `query` leaves (all of them for an empty query), the newest first.
pub fn connections(list: &Value, nodes: &[Value], query: &str) -> Value {
    let name = |id: &str| {
        nodes
            .iter()
            .find(|node| text(node, "id") == Some(id))
            .and_then(|node| text(node, "label"))
            .unwrap_or(id)
            .to_owned()
    };
    let open = list
        .get("active")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let query = query.trim().to_lowercase();
    let mut rows: Vec<Value> = open
        .iter()
        .map(|connection| row(connection, &name))
        .filter(|row| query.is_empty() || matches(row, &query))
        .collect();
    rows.sort_by_key(|row| std::cmp::Reverse((number(row, "startedAtMs"), number(row, "id"))));
    json!({
        "running": list.is_object(),
        "total": open.len(),
        "connections": rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> Value {
        json!({
            "traffic": { "active": 3 },
            "active": [
                { "id": 1, "network": "tcp", "host": "www.google.com", "destination": "142.250.1.1:443",
                  "rule": "DOMAIN-SUFFIX,google.com", "chain": ["Proxy", "n1"], "outbound": "n1",
                  "started_at_ms": 1_700_000_000_000_u64, "upload": 5, "download": 6 },
                { "id": 2, "network": "udp", "host": null, "destination": "[2001:db8::1]:53",
                  "rule": "FINAL", "chain": [], "outbound": "DIRECT",
                  "started_at_ms": 1_700_000_002_000_u64, "upload": 1, "download": 2 },
                { "id": 3, "network": "tcp", "host": "example.org", "destination": "example.org",
                  "rule": "MODE,global", "outbound": "n2",
                  "started_at_ms": 1_700_000_002_000_u64 },
            ],
            "closed": [{ "id": 9, "destination": "old.example:80" }],
        })
    }

    fn nodes() -> Vec<Value> {
        vec![json!({ "id": "n1", "label": "Tokyo" })]
    }

    #[test]
    fn open_connections_are_listed_newest_first_with_their_route_by_name() {
        let answer = connections(&list(), &nodes(), "");
        assert_eq!(answer["running"], true);
        assert_eq!(answer["total"], 3);
        assert_eq!(
            answer["connections"],
            json!([
                { "id": 3, "target": "example.org", "network": "tcp", "rule": "MODE,global",
                  "route": ["n2"], "upload": 0, "download": 0, "startedAtMs": 1_700_000_002_000_u64 },
                { "id": 2, "target": "[2001:db8::1]:53", "network": "udp", "rule": "FINAL",
                  "route": ["DIRECT"], "upload": 1, "download": 2, "startedAtMs": 1_700_000_002_000_u64 },
                { "id": 1, "target": "www.google.com:443", "network": "tcp",
                  "rule": "DOMAIN-SUFFIX,google.com", "route": ["Proxy", "Tokyo"], "upload": 5,
                  "download": 6, "startedAtMs": 1_700_000_000_000_u64 },
            ])
        );
    }

    #[test]
    fn a_search_looks_at_the_target_the_rule_and_the_route() {
        let ids = |query: &str| -> Vec<u64> {
            connections(&list(), &nodes(), query)["connections"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["id"].as_u64().unwrap())
                .collect()
        };
        assert_eq!(ids(" GOOGLE "), [1]);
        assert_eq!(ids("tokyo"), [1]);
        assert_eq!(ids("final"), [2]);
        assert_eq!(ids("direct"), [2]);
        assert_eq!(ids("2001:db8"), [2]);
        assert_eq!(ids("nothing"), Vec::<u64>::new());
        // The count is of all that are open, whatever the search leaves.
        assert_eq!(connections(&list(), &nodes(), "nothing")["total"], 3);
    }

    #[test]
    fn without_an_engine_there_is_nothing_to_list() {
        assert_eq!(
            connections(&Value::Null, &nodes(), ""),
            json!({ "running": false, "total": 0, "connections": [] })
        );
        assert_eq!(
            connections(&json!({}), &[], ""),
            json!({ "running": true, "total": 0, "connections": [] })
        );
    }

    #[test]
    fn a_port_is_read_off_any_kind_of_destination() {
        assert_eq!(port("1.2.3.4:443"), Some("443"));
        assert_eq!(port("[::1]:8080"), Some("8080"));
        assert_eq!(port("example.com:53"), Some("53"));
        assert_eq!(port("example.com"), None);
        assert_eq!(port("2001:db8::1"), None);
        assert_eq!(port("example.com:http"), None);
    }
}
