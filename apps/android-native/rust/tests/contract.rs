//! The shared answer samples (crates/client-core/tests/fixtures) match what the client really answers, so the
//! Kotlin tests that parse the same samples read the real shapes.

use std::collections::BTreeSet;
use std::sync::Arc;

use anixops_net::read_answer;
use client_core::client::{Client, Config, Platform};
use serde_json::Value;

const SNAPSHOT: &str =
    include_str!("../../../../crates/client-core/tests/fixtures/runtime_snapshot.json");
const CONNECT_OK: &str =
    include_str!("../../../../crates/client-core/tests/fixtures/connect_ok.json");
const CONNECT_ERROR: &str =
    include_str!("../../../../crates/client-core/tests/fixtures/connect_error.json");

struct Desktop;

impl Platform for Desktop {}

fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect()
}

#[test]
fn the_snapshot_sample_has_exactly_the_real_fields() {
    let dir = std::env::temp_dir().join(format!("anixops-net-contract-{}", std::process::id()));
    let client = Client::open(
        Config {
            dir: dir.clone(),
            executable: dir.join("anixops-net"),
        },
        Arc::new(Desktop),
    );
    let real = client.invoke("runtime_snapshot", Value::Null).unwrap();
    client.shutdown();
    let _ = std::fs::remove_dir_all(&dir);

    let sample = read_answer(SNAPSHOT).unwrap();
    assert_eq!(
        keys(&sample),
        keys(&real),
        "runtime_snapshot fields changed: update the sample"
    );
    for fact in ["core", "proxy"] {
        let allowed: BTreeSet<String> = ["label", "detail", "tone"].map(String::from).into();
        assert!(keys(&real[fact]).is_subset(&allowed));
        assert!(keys(&sample[fact]).is_subset(&allowed));
    }
}

#[test]
fn the_operation_samples_are_envelopes() {
    let ok = read_answer(CONNECT_OK).unwrap();
    assert_eq!(keys(&ok), BTreeSet::from(["message".to_string()]));
    assert_eq!(
        read_answer(CONNECT_ERROR),
        Err("the current subscription has no nodes".to_string())
    );
}
