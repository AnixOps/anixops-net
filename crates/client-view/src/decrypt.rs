//! HTTPS decryption in the request list: which hosts decryption can be turned on for, which the list already
//! covers, and the setup steps still to do before HTTPS requests show up.

use serde_json::{json, Value};

/// The name without a port or a trailing dot, lower-cased, as the decrypt list keeps it.
fn host_of(host: &str) -> String {
    let without_port = match host.rsplit_once(':') {
        Some((name, port))
            if !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            name
        }
        _ => host,
    };
    without_port
        .strip_suffix('.')
        .unwrap_or(without_port)
        .to_lowercase()
}

/// A domain name decryption can be turned on for: not an IP address, with a dot, made of letters, digits,
/// dots and hyphens, not starting with a dot or hyphen. The core checks again (`mappings::normalize_host`).
fn decryptable(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-')
        && name.contains('.')
        && !name.starts_with(['.', '-'])
        // An IPv4 address is digits and dots only; top-level domains are never all digits.
        && !name.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.')
}

/// Whether `pattern` matches `name`: `*` any run of characters (dots too), `?` one character, as the engine reads
/// `[MITM] hostname` items. `*.example.com` covers the subdomains, not `example.com`.
fn matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.trim().to_lowercase();
    let (pattern, text) = (pattern.as_bytes(), name.as_bytes());
    let (mut p, mut t, mut star, mut mark) = (0, 0, None, 0);
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            mark = t;
            p += 1;
        } else if let Some(at) = star {
            p = at + 1;
            mark += 1;
            t = mark;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

/// What the request list knows about a connection's `host` against the decrypt list's patterns `hosts` and its
/// exclusions `excluded`:
///
/// - `decryptable`: whether decryption can be turned on for it.
/// - `host`: the name the list would keep, or `null` when it is not decryptable.
/// - `covered`: whether a pattern of `hosts` decrypts it (the same name, or a `*` / `?` wildcard) and no
///   exclusion names it.
/// - `excluded`: whether an exclusion names it; exclusions win over every include.
pub fn decrypt_hosts(host: &str, hosts: &[Value], excluded: &[Value]) -> Value {
    let name = host_of(host);
    let can = decryptable(&name);
    let named = |list: &[Value]| {
        can && list
            .iter()
            .filter_map(Value::as_str)
            .any(|pattern| matches(pattern, &name))
    };
    let excluded = named(excluded);
    let covered = named(hosts) && !excluded;
    json!({ "decryptable": can, "host": can.then_some(name), "covered": covered, "excluded": excluded })
}

/// The steps of getting HTTPS requests into the list that are still to do, in the order they have to
/// happen: `connect`, `certificate` (only when `trusted` is `false`; `null` is a device where the app cannot
/// tell), `decryption`, `domains` (no decrypted domain yet) and `capture`. Empty once HTTPS requests can show.
pub fn decrypt_setup(state: &Value) -> Value {
    let flag = |name: &str| state.get(name).and_then(Value::as_bool);
    let mut steps = Vec::new();
    if flag("connected") != Some(true) {
        steps.push("connect");
    }
    if flag("trusted") == Some(false) {
        steps.push("certificate");
    }
    if flag("decrypting") != Some(true) {
        steps.push("decryption");
    }
    if state.get("domains").and_then(Value::as_u64).unwrap_or(0) == 0 {
        steps.push("domains");
    }
    if flag("capturing") != Some(true) {
        steps.push("capture");
    }
    json!({ "steps": steps })
}
