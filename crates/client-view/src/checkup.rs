//! The node checkup (节点体检) result as rows: which measures a window shows, in which order, and which it
//! marks. A window words each row from its id (`checkup.row.<id>` in the catalogue) and formats the value by
//! its kind; `null` is a measure with no successful sample.

use serde_json::{json, Value};

/// A request latency this much higher under load is marked.
const BLOAT_MARK_MS: f64 = 100.0;

fn number(section: &Value, name: &str) -> Value {
    section
        .get(name)
        .and_then(Value::as_f64)
        .map_or(Value::Null, Value::from)
}

fn row(id: &str, kind: &str, value: Value, warn: bool) -> Value {
    json!({ "id": id, "kind": kind, "value": value, "warn": warn })
}

fn failure(section: &Value, rows: &mut Vec<Value>) {
    if let Some(code) = section.get("failure").and_then(Value::as_str) {
        rows.push(row("failure", "failure", json!(code), true));
    }
}

fn latency(latency: &Value) -> Value {
    let loss = latency
        .get("handshake_loss_pct")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .max(
            latency
                .get("request_loss_pct")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        );
    let mut rows = vec![
        row("handshake", "ms", number(latency, "handshake_ms"), false),
        row(
            "cold_handshake",
            "ms",
            number(latency, "cold_handshake_ms"),
            false,
        ),
    ];
    if latency.get("samples").and_then(Value::as_u64).unwrap_or(0) > 1 {
        rows.push(row(
            "warm_handshake",
            "ms",
            number(latency, "warm_handshake_ms"),
            false,
        ));
    }
    rows.push(row("request", "ms", number(latency, "request_ms"), false));
    rows.push(row(
        "jitter",
        "ms",
        number(latency, "request_jitter_ms"),
        false,
    ));
    rows.push(row("loss", "pct", json!(loss), loss > 0.0));
    failure(latency, &mut rows);
    json!({ "id": "latency", "rows": rows, "capped": false })
}

fn download(throughput: &Value) -> Value {
    let bloat = number(throughput, "bloat_ms");
    let loss = throughput
        .get("loaded_loss_pct")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let mut rows = vec![
        row("mbps", "mbps", number(throughput, "mbps"), false),
        row("peak_mbps", "mbps", number(throughput, "peak_mbps"), false),
        row(
            "bytes",
            "bytes",
            throughput.get("bytes").cloned().unwrap_or(json!(0)),
            false,
        ),
        row("idle", "ms", number(throughput, "idle_ms"), false),
        row("loaded", "ms", number(throughput, "loaded_ms"), false),
        row(
            "bloat",
            "ms",
            bloat.clone(),
            bloat.as_f64().is_some_and(|ms| ms > BLOAT_MARK_MS),
        ),
        row("loaded_loss", "pct", json!(loss), loss > 0.0),
    ];
    failure(throughput, &mut rows);
    let capped = throughput
        .get("capped")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    json!({ "id": "download", "rows": rows, "capped": capped })
}

/// The sections of a checkup: `latency`, then `download` when the download ran. `answer` is the core's
/// answer to `node_checkup` (`{ "name", "report" }`) or the report itself.
pub fn checkup(answer: &Value) -> Value {
    let report = answer.get("report").unwrap_or(answer);
    let mut sections = vec![latency(report.get("latency").unwrap_or(&Value::Null))];
    if let Some(throughput) = report.get("throughput").filter(|value| value.is_object()) {
        sections.push(download(throughput));
    }
    json!({
        "name": answer.get("name").and_then(Value::as_str).unwrap_or_default(),
        "sections": sections,
    })
}
