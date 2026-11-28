//! Refusals a window words itself. A refusal stays a `String` (the `{"error": …}` of every channel and the `Err` of
//! native client commands); a coded one reads `<code>: <English>`, the form the kernel's `DomainError` already has
//! (`subscription.core.clash_yaml_unsupported: …`). A window that knows the code shows its own words with the English
//! as detail; the app log and diagnostics keep the English. Codes the client sets itself are `error.<name>`.

use std::fmt::Display;

/// `<code>: <message>`.
pub fn coded(code: &'static str, message: impl Display) -> String {
    format!("{code}: {message}")
}

/// The code of a coded refusal and its English, or `None` for a plain one.
pub fn split(refusal: &str) -> Option<(&str, &str)> {
    let (code, message) = refusal.split_once(": ")?;
    let mut parts = code.split('.');
    let first = parts.next()?;
    let starts_with_letter = first.starts_with(|c: char| c.is_ascii_lowercase());
    let word = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    let mut segments = 1;
    for part in parts {
        if !word(part) {
            return None;
        }
        segments += 1;
    }
    (starts_with_letter && word(first) && segments >= 2).then_some((code, message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_coded_refusal_splits_into_its_code_and_english() {
        let refusal = coded("error.name_required", "enter a name");
        assert_eq!(refusal, "error.name_required: enter a name");
        assert_eq!(
            split(&refusal),
            Some(("error.name_required", "enter a name"))
        );
        // The kernel's DomainError reads the same way.
        assert_eq!(
            split("subscription.core.clash_yaml_unsupported: clash proxy type must be ss"),
            Some((
                "subscription.core.clash_yaml_unsupported",
                "clash proxy type must be ss"
            ))
        );
    }

    #[test]
    fn a_plain_refusal_has_no_code() {
        assert_eq!(split("enter a name"), None);
        assert_eq!(split("Disconnect first: the engine runs"), None);
        assert_eq!(split("/dev/net/tun: No such device"), None);
        assert_eq!(split("TUNSETIFF: Operation not permitted"), None);
        assert_eq!(split("error: something"), None);
    }
}
