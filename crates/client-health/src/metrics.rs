//! The local Prometheus endpoint of the desktop clients.
//!
//! `GET /metrics` on a loopback address answers the engine's metrics (the kernel's `metrics` control command,
//! when the engine runs) followed by the client's own gauges from [`ClientMetrics`]. Any other path is 404,
//! any other method 405. A request whose `Host` is not the loopback address it reached (`127.0.0.1:<port>`,
//! `localhost:<port>`, `[::1]:<port>`) is 403: a web page that rebinds its own name to 127.0.0.1 cannot read the
//! node and policy group names. A request head is at most 8 KiB and a connection lasts at most 5 seconds, so one
//! slow client cannot hold the endpoint. One small thread serves it with the standard library; no HTTP or metrics
//! crate.

use std::fmt::Write as _;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";
const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// The longest a connection is served, however slowly its request arrives.
const CONNECTION_DEADLINE: Duration = Duration::from_secs(5);
/// The largest request line and headers read, as the macOS endpoint.
const MAX_HEAD_BYTES: usize = 8 * 1024;

/// The client's own gauges.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientMetrics {
    /// `windows` or `linux`.
    pub platform: &'static str,
    pub version: &'static str,
    pub connected: bool,
    pub crash_reports_enabled: bool,
    pub crash_reports_kept: usize,
    /// [`crate::integrity::Integrity::state`]; empty while the check runs.
    pub integrity: &'static str,
    /// Milliseconds from process start to the window's first page load.
    pub cold_start_ms: Option<u64>,
}

impl ClientMetrics {
    /// The gauges in the Prometheus text format.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut gauge = |name: &str, help: &str, samples: &[(String, String)]| {
            let _ = writeln!(out, "# HELP {name} {help}\n# TYPE {name} gauge");
            for (labels, value) in samples {
                let _ = writeln!(out, "{name}{labels} {value}");
            }
        };
        gauge(
            "anixops_client_info",
            "Client version and platform; always 1.",
            &[(
                format!(
                    "{{platform=\"{}\",version=\"{}\"}}",
                    self.platform, self.version
                ),
                "1".to_string(),
            )],
        );
        gauge(
            "anixops_client_connected",
            "1 while the client is connected.",
            &[(String::new(), u8::from(self.connected).to_string())],
        );
        gauge(
            "anixops_client_crash_reports_enabled",
            "1 when crash reports are kept.",
            &[(
                String::new(),
                u8::from(self.crash_reports_enabled).to_string(),
            )],
        );
        gauge(
            "anixops_client_crash_reports_kept",
            "Crash reports kept on disk.",
            &[(String::new(), self.crash_reports_kept.to_string())],
        );
        let states: Vec<(String, String)> = ["verified", "unverified", "damaged", "invalid"]
            .iter()
            .map(|state| {
                (
                    format!("{{state=\"{state}\"}}"),
                    u8::from(self.integrity == *state).to_string(),
                )
            })
            .collect();
        gauge(
            "anixops_client_integrity",
            "Install integrity check result; 1 for the current state.",
            &states,
        );
        if let Some(ms) = self.cold_start_ms {
            gauge(
                "anixops_client_cold_start_seconds",
                "Seconds from process start to the first page load of the window.",
                &[(String::new(), format!("{}.{:03}", ms / 1000, ms % 1000))],
            );
        }
        out
    }
}

/// The loopback endpoint; stops when dropped.
pub struct MetricsServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MetricsServer {
    /// Serves `render()` at `GET /metrics` on `address`, which must be a loopback address.
    pub fn start(
        address: SocketAddr,
        render: Arc<dyn Fn() -> String + Send + Sync>,
    ) -> io::Result<Self> {
        if !address.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the metrics endpoint listens on loopback only",
            ));
        }
        let listener = TcpListener::bind(address)?;
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = thread::Builder::new()
            .name("metrics".to_string())
            .spawn(move || {
                for stream in listener.incoming() {
                    if stopping.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Ok(stream) = stream {
                        let _ = serve(stream, address, render.as_ref());
                    }
                }
            })?;
        Ok(Self {
            address,
            stop,
            thread: Some(thread),
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }
}

impl Drop for MetricsServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wake the accept loop.
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_millis(200));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(
    stream: TcpStream,
    address: SocketAddr,
    render: &(dyn Fn() -> String + Send + Sync),
) -> io::Result<()> {
    stream.set_write_timeout(Some(READ_TIMEOUT))?;
    let deadline = Instant::now() + CONNECTION_DEADLINE;
    let (status, content_type, body) = match read_head(&stream, deadline) {
        Ok(head) => answer(&String::from_utf8_lossy(&head), address, render),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => (
            "431 Request Header Fields Too Large",
            "text/plain",
            "request header fields too large\n".to_string(),
        ),
        Err(error) => return Err(error),
    };
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

/// The request line and headers, up to the blank line that ends them (or the end of the stream). Longer than
/// [`MAX_HEAD_BYTES`] is `InvalidData`; still unfinished at `deadline` is `TimedOut`.
fn read_head(stream: &TcpStream, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut head = Vec::new();
    let mut buffer = [0u8; 1024];
    let mut reader = stream;
    loop {
        if let Some(end) = head_end(&head) {
            head.truncate(end);
            return Ok(head);
        }
        if head.len() > MAX_HEAD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the request head is too large",
            ));
        }
        let left = deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "the request took too long"))?;
        stream.set_read_timeout(Some(left.min(READ_TIMEOUT)))?;
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(head);
        }
        head.extend_from_slice(&buffer[..read]);
    }
}

/// Where the blank line that ends a request head starts.
fn head_end(head: &[u8]) -> Option<usize> {
    let crlf = head.windows(4).position(|window| window == b"\r\n\r\n");
    let lf = head.windows(2).position(|window| window == b"\n\n");
    match (crlf, lf) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// The status, type and body for a request head.
fn answer(
    head: &str,
    address: SocketAddr,
    render: &(dyn Fn() -> String + Send + Sync),
) -> (&'static str, &'static str, String) {
    let mut lines = head.lines().map(|line| line.trim_end_matches('\r'));
    let request_line = lines.next().unwrap_or_default();
    let host = lines.find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("host")
            .then(|| value.trim())
    });
    if !host.is_some_and(|host| host_allowed(host, address)) {
        return ("403 Forbidden", "text/plain", "forbidden\n".to_string());
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let path = path.split('?').next().unwrap_or_default();
    match (method, path) {
        ("GET", "/metrics") => ("200 OK", CONTENT_TYPE, render()),
        ("GET", _) => ("404 Not Found", "text/plain", "not found\n".to_string()),
        _ => (
            "405 Method Not Allowed",
            "text/plain",
            "method not allowed\n".to_string(),
        ),
    }
}

/// Whether `host` (a `Host` header) names the loopback endpoint at `address`: `localhost`, `127.0.0.1`, `[::1]` or
/// the address it listens on, with its port. Any other name, such as one a web page rebinds to 127.0.0.1, is not.
fn host_allowed(host: &str, address: SocketAddr) -> bool {
    let Some((name, port)) = host.rsplit_once(':') else {
        return false;
    };
    if port.parse::<u16>().ok() != Some(address.port()) {
        return false;
    }
    if name.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let literal = name
        .strip_prefix('[')
        .and_then(|name| name.strip_suffix(']'))
        .unwrap_or(name);
    match literal.parse::<IpAddr>() {
        Ok(ip) => {
            ip == address.ip()
                || ip == IpAddr::from([127, 0, 0, 1])
                || ip == IpAddr::from([0u16, 0, 0, 0, 0, 0, 0, 1])
        }
        Err(_) => false,
    }
}

/// The engine's text (if any) followed by the client's.
pub fn combine(engine: Option<String>, client: &ClientMetrics) -> String {
    let mut out = engine.unwrap_or_default();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&client.render());
    out
}

/// The `text` of a kernel `metrics` control answer.
pub fn engine_text(answer: &serde_json::Value) -> Option<String> {
    answer
        .get("text")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::{combine, host_allowed, ClientMetrics, MetricsServer};
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    fn get(address: SocketAddr, request: &str) -> String {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        answer
    }

    #[test]
    fn client_gauges_follow_their_families() {
        let text = ClientMetrics {
            platform: "linux",
            version: "0.3.0",
            connected: true,
            crash_reports_enabled: true,
            crash_reports_kept: 2,
            integrity: "verified",
            cold_start_ms: Some(812),
        }
        .render();
        let lines: Vec<&str> = text.lines().collect();
        let at = |line: &str| {
            lines
                .iter()
                .position(|l| *l == line)
                .unwrap_or_else(|| panic!("{line} in\n{text}"))
        };
        assert!(at("# TYPE anixops_client_connected gauge") < at("anixops_client_connected 1"));
        assert!(
            at("anixops_client_connected 1")
                < at("# HELP anixops_client_crash_reports_enabled 1 when crash reports are kept.")
        );
        at("anixops_client_info{platform=\"linux\",version=\"0.3.0\"} 1");
        at("anixops_client_integrity{state=\"verified\"} 1");
        at("anixops_client_integrity{state=\"damaged\"} 0");
        at("anixops_client_cold_start_seconds 0.812");
        at("anixops_client_crash_reports_kept 2");
    }

    #[test]
    fn the_endpoint_serves_metrics_only() {
        let server = MetricsServer::start(
            "127.0.0.1:0".parse().unwrap(),
            Arc::new(|| {
                combine(
                    Some("networkcore_up 1".to_string()),
                    &ClientMetrics::default(),
                )
            }),
        )
        .unwrap();
        let port = server.address().port();
        let request = |line: &str, host: &str| format!("{line}\r\nHost: {host}\r\n\r\n");
        let local = format!("127.0.0.1:{port}");
        let answer = get(server.address(), &request("GET /metrics HTTP/1.1", &local));
        assert!(answer.starts_with("HTTP/1.1 200 OK\r\n"), "{answer}");
        assert!(answer.contains("text/plain; version=0.0.4"));
        assert!(answer.contains("\r\n\r\nnetworkcore_up 1\n# HELP anixops_client_info"));
        assert!(
            get(server.address(), &request("GET / HTTP/1.1", &local)).starts_with("HTTP/1.1 404")
        );
        assert!(
            get(server.address(), &request("POST /metrics HTTP/1.1", &local))
                .starts_with("HTTP/1.1 405")
        );
        for host in [format!("localhost:{port}"), format!("LOCALHOST:{port}")] {
            assert!(
                get(server.address(), &request("GET /metrics HTTP/1.1", &host))
                    .starts_with("HTTP/1.1 200"),
                "{host}"
            );
        }
        // A page that rebinds its own name to 127.0.0.1, another port, or no Host at all.
        for host in [
            format!("evil.example:{port}"),
            "127.0.0.1:1".to_string(),
            "127.0.0.1".to_string(),
        ] {
            assert!(
                get(server.address(), &request("GET /metrics HTTP/1.1", &host))
                    .starts_with("HTTP/1.1 403"),
                "{host}"
            );
        }
        assert!(get(server.address(), "GET /metrics HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 403"));
        assert!(
            MetricsServer::start("192.0.2.1:9477".parse().unwrap(), Arc::new(String::new)).is_err()
        );
        drop(server);
    }

    #[test]
    fn only_the_loopback_names_of_the_endpoint_are_hosts() {
        let address: SocketAddr = "127.0.0.1:9477".parse().unwrap();
        assert!(host_allowed("127.0.0.1:9477", address));
        assert!(host_allowed("localhost:9477", address));
        assert!(host_allowed("[::1]:9477", address));
        assert!(!host_allowed("127.0.0.1:9478", address));
        assert!(!host_allowed("rebound.example:9477", address));
        assert!(!host_allowed("127.0.0.1", address));
        assert!(!host_allowed("", address));
        let other: SocketAddr = "127.0.0.2:9477".parse().unwrap();
        assert!(host_allowed("127.0.0.2:9477", other));
    }

    #[test]
    fn a_huge_or_slow_request_does_not_hold_the_endpoint() {
        let server =
            MetricsServer::start("127.0.0.1:0".parse().unwrap(), Arc::new(String::new)).unwrap();
        let address = server.address();
        // A head past 8 KiB is refused without reading on. Exactly one byte past the limit, so the endpoint has
        // read all of it when it answers.
        let mut huge = TcpStream::connect(address).unwrap();
        let mut head = b"GET /metrics HTTP/1.1\r\nX: ".to_vec();
        head.resize(super::MAX_HEAD_BYTES + 1, b'a');
        huge.write_all(&head).unwrap();
        let mut answer = String::new();
        huge.read_to_string(&mut answer).unwrap();
        assert!(answer.starts_with("HTTP/1.1 431"), "{answer}");
        // A client that sends a byte now and then is dropped at the deadline; the next request is answered.
        let mut slow = TcpStream::connect(address).unwrap();
        let started = Instant::now();
        let trickle = std::thread::spawn(move || {
            for _ in 0..8 {
                if slow.write_all(b"G").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
        std::thread::sleep(Duration::from_millis(100));
        let answer = get(
            address,
            &format!(
                "GET /metrics HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
                address.port()
            ),
        );
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        assert!(started.elapsed() < Duration::from_secs(7));
        let _ = trickle.join();
        drop(server);
    }
}
