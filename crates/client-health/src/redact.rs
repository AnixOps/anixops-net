//! Redaction for text that leaves the user's hands (crash reports, diagnostics).
//!
//! Each run of "word" characters is looked at on its own and replaced when it is a URL (`<url>`), an e-mail
//! address (`<email>`), an IPv4 or IPv6 address (`<ip>`), a host name (`<host>`), a UUID (`<id>`) or a long
//! key-like token (`<secret>`). The user's home directory and account name become `~` and `<user>` first.
//! Source file names (`main.rs:12`) and Rust paths (`std::panicking`) are kept, since a crash report needs them.

/// Characters that belong to one token. `:` stays inside so `host:port`, `[v6]:port` and URLs stay whole.
fn is_token_char(c: char) -> bool {
    c.is_alphanumeric()
        || matches!(
            c,
            '.' | '_' | '-' | ':' | '/' | '@' | '%' | '+' | '=' | '?' | '&' | '#' | '~' | '[' | ']'
        )
}

/// File extensions that mark a token as a file name rather than a host.
const FILE_EXTENSIONS: [&str; 16] = [
    "rs", "toml", "json", "txt", "swift", "so", "dll", "exe", "dylib", "md", "log", "ts", "tsx",
    "js", "c", "h",
];

/// `text` with personal data replaced; see the module documentation.
pub fn redact(text: &str) -> String {
    let text = redact_account(text);
    let mut out = String::with_capacity(text.len());
    let mut token = String::new();
    for c in text.chars() {
        if is_token_char(c) {
            token.push(c);
        } else {
            flush(&mut out, &mut token);
            out.push(c);
        }
    }
    flush(&mut out, &mut token);
    out
}

fn flush(out: &mut String, token: &mut String) {
    if !token.is_empty() {
        out.push_str(&redact_token(token));
        token.clear();
    }
}

/// The home directory and the account name, from the environment of this process.
fn redact_account(text: &str) -> String {
    let mut text = text.to_string();
    for key in ["HOME", "USERPROFILE"] {
        if let Some(home) = std::env::var(key).ok().filter(|home| home.len() > 3) {
            text = text.replace(&home, "~");
        }
    }
    for key in ["USER", "USERNAME", "LOGNAME"] {
        if let Some(user) = std::env::var(key).ok().filter(|user| user.len() >= 3) {
            text = replace_word(&text, &user, "<user>");
        }
    }
    text
}

/// Replaces `word` where it is not part of a longer alphanumeric run.
fn replace_word(text: &str, word: &str, with: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(word) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + word.len()..].chars().next();
        let bounded =
            !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric);
        out.push_str(&rest[..at]);
        out.push_str(if bounded { with } else { word });
        rest = &rest[at + word.len()..];
    }
    out.push_str(rest);
    out
}

fn redact_token(token: &str) -> String {
    // Keep trailing sentence punctuation outside the token.
    let core = token.trim_end_matches(['.', ':', ',']);
    let tail = &token[core.len()..];
    let replaced = classify(core).map_or_else(|| core.to_string(), str::to_string);
    format!("{replaced}{tail}")
}

fn classify(token: &str) -> Option<&'static str> {
    if token.is_empty() {
        return None;
    }
    if token.contains("://") {
        return Some("<url>");
    }
    if is_email(token) {
        return Some("<email>");
    }
    if is_ip_like(token) {
        return Some("<ip>");
    }
    if is_uuid(token) {
        return Some("<id>");
    }
    if is_secret(token) {
        return Some("<secret>");
    }
    if is_host(token) {
        return Some("<host>");
    }
    None
}

fn is_email(token: &str) -> bool {
    let Some((user, domain)) = token.split_once('@') else {
        return false;
    };
    !user.is_empty() && domain.contains('.') && !domain.starts_with('.')
}

/// IPv4 with an optional port, IPv6 (bracketed or bare) with an optional port.
fn is_ip_like(token: &str) -> bool {
    let host = strip_port(token);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.parse::<std::net::Ipv4Addr>().is_ok() || host.parse::<std::net::Ipv6Addr>().is_ok() {
        return true;
    }
    // A bare IPv6 address with a port appended (`2001:db8::1:443`) still parses as an address above; Rust paths
    // (`core::fmt`) never do because they hold non-hex letters.
    false
}

fn strip_port(token: &str) -> &str {
    if let Some(rest) = token.strip_prefix('[') {
        return rest.split_once(']').map_or(token, |(host, _)| host);
    }
    match token.rsplit_once(':') {
        Some((host, port))
            if host.matches(':').count() == 0
                && !port.is_empty()
                && port.bytes().all(|b| b.is_ascii_digit()) =>
        {
            host
        }
        _ => token,
    }
}

fn is_uuid(token: &str) -> bool {
    let groups: Vec<&str> = token.split('-').collect();
    groups.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(&groups)
            .all(|(len, group)| group.len() == *len && group.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// 20 or more base64/hex-like characters mixing letters and digits: keys, passwords, tokens.
fn is_secret(token: &str) -> bool {
    let core = token.trim_start_matches(['/', '=']);
    let run = core
        .split(['/', ':', '?', '&', '#', '@', '[', ']'])
        .find(|part| part.len() >= 20)
        .unwrap_or("");
    run.len() >= 20
        && run
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'=' | b'_' | b'-' | b'.'))
        && run.bytes().any(|b| b.is_ascii_digit())
        && run.bytes().any(|b| b.is_ascii_alphabetic())
        && !run.contains("..")
        && !looks_like_rust_symbol(run)
}

/// Mangled or hashed Rust symbols in backtraces (`h1a2b3c4d5e6f7a8b`) are kept.
fn looks_like_rust_symbol(run: &str) -> bool {
    run.len() == 17 && run.starts_with('h') && run[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// `name.tld` or `name.tld:port`, where the last label is alphabetic and not a file extension.
fn is_host(token: &str) -> bool {
    if token.contains("::") || token.contains('/') || token.contains('@') {
        return false;
    }
    let host = strip_port(token);
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|label| label.is_empty()) {
        return false;
    }
    let tld = labels[labels.len() - 1];
    tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic())
        && !FILE_EXTENSIONS.contains(&tld.to_ascii_lowercase().as_str())
        && labels.iter().all(|label| {
            label
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        })
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn urls_hosts_addresses_and_keys_are_removed() {
        let text = "failed to fetch https://sub.example.com/api/v1/client/subscribe?token=abc123 from node hk.example.net:443 (203.0.113.7:8443, [2001:db8::1]:443) for a@b.cn key=Zm9vYmFyYmF6cXV4MTIzNDU2Nzg5MA== id 3f2504e0-4f89-11d3-9a0c-0305e82c3301";
        let redacted = redact(text);
        for gone in [
            "example",
            "203.0.113.7",
            "2001:db8",
            "a@b.cn",
            "Zm9v",
            "3f2504e0",
            "token",
        ] {
            assert!(!redacted.contains(gone), "{gone} in {redacted}");
        }
        assert!(redacted.contains("<url>"));
        assert!(redacted.contains("<host>"));
        assert!(redacted.contains("<ip>"));
        assert!(redacted.contains("<email>"));
        assert!(redacted.contains("<secret>"));
        assert!(redacted.contains("<id>"));
    }

    /// Also in the Swift port's tests (`ClientHealthTests.testRedactionMarksEachTokenLikeRust`).
    #[test]
    fn each_token_is_marked() {
        for (input, expected) in [
            ("node hk.example.net:443.", "node <host>."),
            ("dial [2001:db8::1]:443 and 2001:db8::2", "dial <ip> and <ip>"),
            ("peer 203.0.113.7:8443, done", "peer <ip>, done"),
            ("key=Zm9vYmFyYmF6cXV4MTIzNDU2Nzg5MA==", "<secret>"),
            ("id 3F2504E0-4F89-11D3-9A0C-0305E82C3301", "id <id>"),
            ("mail a@b.cn", "mail <email>"),
            ("see ss://YWVzLTI1Ni1nY20@host:8388#name", "see <url>"),
        ] {
            assert_eq!(redact(input), expected, "{input}");
        }
    }

    #[test]
    fn code_locations_and_symbols_are_kept() {
        let text = "panicked at crates/client-health/src/lib.rs:42:7:\nindex out of bounds\n   3: std::panicking::begin_panic::h1a2b3c4d5e6f7a8b\n   4: core::fmt::write";
        assert_eq!(redact(text), text);
    }

    #[test]
    fn plain_words_and_numbers_are_kept() {
        let text = "connection reset after 1500 ms; version 0.3.0-beta.4";
        assert_eq!(redact(text), text);
    }
}
