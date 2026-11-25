//! URLs and log text, masked before they reach an AI tool.

/// What a tool shows of a URL: the host and the path, and only the names of the query parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UrlParts {
    pub host: String,
    pub path: String,
    pub query_names: Vec<String>,
}

/// Splits `url` (`scheme://user@host:port/path?query#fragment`, or a bare authority and path) into what a
/// tool may show. The user name and password, the query values and the fragment are dropped.
pub fn split_url(url: &str) -> UrlParts {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, rest) = rest.split_at(authority_end);
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let rest = rest.split_once('#').map_or(rest, |(before, _)| before);
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut query_names: Vec<String> = Vec::new();
    for pair in query.split('&') {
        let name = pair.split_once('=').map_or(pair, |(name, _)| name);
        if !name.is_empty() && !query_names.iter().any(|known| known == name) {
            query_names.push(name.to_string());
        }
    }
    UrlParts {
        host: host.to_string(),
        path: if path.is_empty() { "/" } else { path }.to_string(),
        query_names,
    }
}

/// What stands in for a masked part.
pub const MASK: &str = "<redacted>";

/// `text` with every `http://` and `https://` URL in it masked: the user name and password removed, the
/// query and fragment replaced by [`MASK`]. A URL ends at white space or a quote or angle bracket.
pub fn scrub_text(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(start) = next_url(&lower, at) {
        out.push_str(&text[at..start]);
        let end = text[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`'))
            .map_or(text.len(), |length| start + length);
        out.push_str(&scrub_url(&text[start..end]));
        at = end;
    }
    out.push_str(&text[at..]);
    out
}

/// Where the next URL starts at or after `from`, in the lower-cased text.
fn next_url(lower: &str, from: usize) -> Option<usize> {
    let tail = &lower[from..];
    [tail.find("http://"), tail.find("https://")]
        .into_iter()
        .flatten()
        .min()
        .map(|offset| from + offset)
}

/// One URL with its user information, query and fragment masked.
fn scrub_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, rest) = rest.split_at(authority_end);
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let cut = rest.find(['?', '#']);
    let path = cut.map_or(rest, |cut| &rest[..cut]);
    let mut scrubbed = format!("{scheme}://{host}{path}");
    if cut.is_some() {
        scrubbed.push('?');
        scrubbed.push_str(MASK);
    }
    scrubbed
}

/// `text` cut to at most `max` bytes at a character boundary, and whether it was cut.
pub fn cut(text: &str, max: usize) -> (&str, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_keep_host_path_and_query_names() {
        let parts =
            split_url("https://user:pw@api.test:8443/v1/items?token=abc&page=2&page=3#frag");
        assert_eq!(parts.host, "api.test:8443");
        assert_eq!(parts.path, "/v1/items");
        assert_eq!(parts.query_names, ["token", "page"]);

        let bare = split_url("https://a.test");
        assert_eq!(bare.host, "a.test");
        assert_eq!(bare.path, "/");
        assert!(bare.query_names.is_empty());

        let fragment_only = split_url("https://a.test/p#access_token=abc");
        assert_eq!(fragment_only.path, "/p");
        assert!(fragment_only.query_names.is_empty());
    }

    #[test]
    fn log_text_has_its_urls_masked() {
        let text =
            "update https://me:secret@sub.test/api/v1?token=abc failed; see HTTP://b.test/x#k=v.\n\
                    \"https://c.test/plain\" ok";
        let scrubbed = scrub_text(text);
        assert!(!scrubbed.contains("secret"), "{scrubbed}");
        assert!(!scrubbed.contains("abc"), "{scrubbed}");
        assert!(!scrubbed.contains("k=v"), "{scrubbed}");
        assert!(
            scrubbed.contains("https://sub.test/api/v1?<redacted> failed"),
            "{scrubbed}"
        );
        assert!(
            scrubbed.contains("HTTP://b.test/x?<redacted>"),
            "{scrubbed}"
        );
        assert!(
            scrubbed.contains("\"https://c.test/plain\" ok"),
            "{scrubbed}"
        );
        assert_eq!(scrub_text("no links here"), "no links here");
    }

    #[test]
    fn text_is_cut_at_a_character_boundary() {
        assert_eq!(cut("abc", 5), ("abc", false));
        assert_eq!(cut("中文", 4), ("中", true));
        assert_eq!(cut("abcdef", 3), ("abc", true));
    }
}
