//! Single request replay (debug client plan E1): the user's edited copy of a captured request is sent again through
//! the engine's local proxy, so it leaves through the current egress and shows up in the capture like any other
//! request. Redirects are not followed and no cookies are kept, as on the Apple clients.

use std::io::Read;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::capture::{decode_base64, encode_base64};

/// How much of a replayed response the window gets back.
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Headers the HTTP client sets itself or that belong to one hop only.
const SKIPPED_HEADERS: [&str; 8] = [
    "host",
    "content-length",
    "connection",
    "keep-alive",
    "proxy-connection",
    "proxy-authorization",
    "transfer-encoding",
    "upgrade",
];

/// The request to send, as the window edited it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// The body as base64; empty for none.
    #[serde(default)]
    pub body_base64: String,
}

/// What came back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body_base64: String,
    /// The body was longer than [`MAX_RESPONSE_BYTES`] and was cut there.
    pub truncated: bool,
    pub elapsed_ms: u64,
}

/// Whether `name` is left to the HTTP client.
pub fn skipped_header(name: &str) -> bool {
    SKIPPED_HEADERS.contains(&name.trim().to_ascii_lowercase().as_str())
}

/// Sends `request` through the HTTP proxy at `proxy`. `root_pem` is the HTTPS decryption root, trusted on top of the
/// usual roots so decrypted hosts answer with a certificate the client accepts.
pub fn send(
    proxy: SocketAddr,
    root_pem: Option<&str>,
    request: &ReplayRequest,
) -> Result<ReplayResponse, String> {
    let url =
        reqwest::Url::parse(request.url.trim()).map_err(|error| format!("invalid URL: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("only http and https requests can be replayed".to_string());
    }
    let method = reqwest::Method::from_bytes(request.method.trim().as_bytes())
        .map_err(|_| format!("invalid method {:?}", request.method))?;
    let mut builder = reqwest::blocking::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://{proxy}")).map_err(|error| error.to_string())?)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30));
    if let Some(pem) = root_pem {
        let root =
            reqwest::Certificate::from_pem(pem.as_bytes()).map_err(|error| error.to_string())?;
        builder = builder.add_root_certificate(root);
    }
    let client = builder.build().map_err(|error| error.to_string())?;
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in &request.headers {
        if skipped_header(name) {
            continue;
        }
        let name = reqwest::header::HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|_| format!("invalid header name {name:?}"))?;
        let value = reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| format!("invalid value for header {name}"))?;
        headers.append(name, value);
    }
    let body = decode_base64(&request.body_base64)?;
    let started = Instant::now();
    let mut response = client
        .request(method, url)
        .headers(headers)
        .body(body)
        .send()
        .map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    let mut body = Vec::new();
    (&mut response)
        .take(MAX_RESPONSE_BYTES as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    let truncated = body.len() > MAX_RESPONSE_BYTES;
    body.truncate(MAX_RESPONSE_BYTES);
    Ok(ReplayResponse {
        status,
        headers,
        body_base64: encode_base64(&body),
        truncated,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn hop_by_hop_headers_are_left_to_the_client() {
        assert!(skipped_header("Host"));
        assert!(skipped_header(" Content-Length "));
        assert!(skipped_header("Proxy-Authorization"));
        assert!(!skipped_header("Authorization"));
        assert!(!skipped_header("Cookie"));
    }

    #[test]
    fn rejects_other_schemes_and_bad_methods() {
        let proxy = SocketAddr::from(([127, 0, 0, 1], 9));
        let request = |method: &str, url: &str| ReplayRequest {
            method: method.to_string(),
            url: url.to_string(),
            headers: Vec::new(),
            body_base64: String::new(),
        };
        assert!(send(proxy, None, &request("GET", "ftp://example.com/")).is_err());
        assert!(send(proxy, None, &request("G ET", "http://example.com/")).is_err());
        assert!(send(proxy, None, &request("GET", "not a url")).is_err());
    }

    /// A one-shot HTTP proxy stand-in: answers the first request with a redirect and hands back what it got.
    #[test]
    fn sends_through_the_proxy_without_following_redirects() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                seen.extend_from_slice(&buffer[..read]);
                let complete =
                    seen.windows(4).any(|window| window == b"\r\n\r\n") && seen.ends_with(b"hello");
                if read == 0 || complete {
                    break;
                }
            }
            stream
                .write_all(b"HTTP/1.1 302 Found\r\nLocation: http://example.com/next\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
            String::from_utf8_lossy(&seen).into_owned()
        });
        let response = send(
            proxy,
            None,
            &ReplayRequest {
                method: "POST".to_string(),
                url: "http://example.com/api?x=1".to_string(),
                headers: vec![
                    ("Host".to_string(), "ignored".to_string()),
                    ("X-Test".to_string(), "1".to_string()),
                ],
                body_base64: encode_base64(b"hello"),
            },
        )
        .unwrap();
        let seen = server.join().unwrap();
        assert!(
            seen.starts_with("POST http://example.com/api?x=1 HTTP/1.1"),
            "{seen}"
        );
        assert!(seen.to_ascii_lowercase().contains("x-test: 1"), "{seen}");
        assert!(!seen.contains("ignored"), "{seen}");
        assert_eq!(response.status, 302);
        assert_eq!(decode_base64(&response.body_base64).unwrap(), b"ok");
        assert!(!response.truncated);
    }
}
