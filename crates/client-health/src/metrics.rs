//! The local Prometheus endpoint of the desktop clients.
//!
//! `GET /metrics` on a loopback address answers the engine's metrics (the kernel's `metrics` control command,
//! when the engine runs) followed by the client's own gauges from [`ClientMetrics`]. Any other path is 404,
//! any other method 405. One small thread serves it with the standard library; no HTTP or metrics crate.

use std::fmt::Write as _;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";
const READ_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_HEADER_LINES: usize = 64;

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
                        let _ = serve(stream, render.as_ref());
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

fn serve(stream: TcpStream, render: &(dyn Fn() -> String + Send + Sync)) -> io::Result<()> {
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    stream.set_write_timeout(Some(READ_TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    // Drain the headers so the client sees a clean close.
    for _ in 0..MAX_HEADER_LINES {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let path = path.split('?').next().unwrap_or_default();
    let (status, content_type, body) = match (method, path) {
        ("GET", "/metrics") => ("200 OK", CONTENT_TYPE, render()),
        ("GET", _) => ("404 Not Found", "text/plain", "not found\n".to_string()),
        _ => (
            "405 Method Not Allowed",
            "text/plain",
            "method not allowed\n".to_string(),
        ),
    };
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
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
    use super::{combine, ClientMetrics, MetricsServer};
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::sync::Arc;

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
        let answer = get(server.address(), "GET /metrics HTTP/1.1\r\nHost: x\r\n\r\n");
        assert!(answer.starts_with("HTTP/1.1 200 OK\r\n"), "{answer}");
        assert!(answer.contains("text/plain; version=0.0.4"));
        assert!(answer.contains("\r\n\r\nnetworkcore_up 1\n# HELP anixops_client_info"));
        assert!(get(server.address(), "GET / HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 404"));
        assert!(get(server.address(), "POST /metrics HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 405"));
        assert!(
            MetricsServer::start("192.0.2.1:9477".parse().unwrap(), Arc::new(String::new)).is_err()
        );
        drop(server);
    }
}
