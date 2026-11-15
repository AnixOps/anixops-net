import Foundation

/// The local Prometheus endpoint of the macOS app, the same as the desktop clients' (`crates/client-health`
/// metrics.rs): `GET /metrics` on 127.0.0.1 answers the engine's metrics (the `metrics` control command, while
/// connected) followed by the client's own gauges. Any other path is 404, any other method 405. This type holds the
/// HTTP and text parts; the app owns the listener.
public enum MetricsEndpoint {
    /// The desktop clients' default port.
    public static let defaultPort: UInt16 = 9477
    public static let contentType = "text/plain; version=0.0.4; charset=utf-8"
    /// Longest request head read before answering 400.
    public static let maxHeadBytes = 8192

    public enum Route: Equatable {
        case metrics
        case notFound
        case methodNotAllowed
        case badRequest
    }

    /// Whether `head` holds the whole request head (up to the blank line).
    public static func isComplete(_ head: Data) -> Bool {
        head.range(of: Data("\r\n\r\n".utf8)) != nil
    }

    /// The answer to a request head.
    public static func route(_ head: Data) -> Route {
        guard let text = String(data: head, encoding: .utf8),
              let line = text.components(separatedBy: "\r\n").first
        else {
            return .badRequest
        }
        let parts = line.split(separator: " ", omittingEmptySubsequences: true)
        guard parts.count == 3, parts[2].hasPrefix("HTTP/1.") else {
            return .badRequest
        }
        let path = parts[1].split(separator: "?", maxSplits: 1, omittingEmptySubsequences: false).first ?? ""
        guard path == "/metrics" else {
            return .notFound
        }
        return parts[0] == "GET" ? .metrics : .methodNotAllowed
    }

    /// A complete HTTP/1.1 response that closes the connection.
    public static func response(_ route: Route, body: String) -> Data {
        let (status, type, text): (String, String, String)
        switch route {
        case .metrics:
            (status, type, text) = ("200 OK", contentType, body)
        case .notFound:
            (status, type, text) = ("404 Not Found", "text/plain; charset=utf-8", "not found\n")
        case .methodNotAllowed:
            (status, type, text) = ("405 Method Not Allowed", "text/plain; charset=utf-8", "method not allowed\n")
        case .badRequest:
            (status, type, text) = ("400 Bad Request", "text/plain; charset=utf-8", "bad request\n")
        }
        let payload = Data(text.utf8)
        var head = "HTTP/1.1 \(status)\r\nContent-Type: \(type)\r\nContent-Length: \(payload.count)\r\nConnection: close\r\n"
        if route == .methodNotAllowed {
            head += "Allow: GET\r\n"
        }
        return Data((head + "\r\n").utf8) + payload
    }

    /// The `text` of a `metrics` control answer.
    public static func engineText(_ answer: Data?) -> String? {
        guard let answer,
              let object = try? JSONSerialization.jsonObject(with: answer) as? [String: Any]
        else {
            return nil
        }
        return object["text"] as? String
    }

    /// The client's gauges, named as on Windows and Linux.
    public static func clientGauges(
        platform: String,
        version: String,
        connected: Bool,
        crashReportsEnabled: Bool,
        crashReportsKept: Int,
        integrity: String
    ) -> String {
        var out = ""
        func gauge(_ name: String, _ help: String, _ samples: [(String, String)]) {
            out += "# HELP \(name) \(help)\n# TYPE \(name) gauge\n"
            for (labels, value) in samples {
                out += "\(name)\(labels) \(value)\n"
            }
        }
        gauge("anixops_client_info", "Client version and platform; always 1.", [
            ("{platform=\"\(escape(platform))\",version=\"\(escape(version))\"}", "1"),
        ])
        gauge("anixops_client_connected", "1 while the client is connected.", [("", connected ? "1" : "0")])
        gauge("anixops_client_crash_reports_enabled", "1 when crash reports are kept.", [("", crashReportsEnabled ? "1" : "0")])
        gauge("anixops_client_crash_reports_kept", "Crash reports kept on disk.", [("", String(crashReportsKept))])
        gauge(
            "anixops_client_integrity",
            "Install integrity check result; 1 for the current state.",
            ["verified", "unverified", "damaged", "invalid"].map { ("{state=\"\($0)\"}", integrity == $0 ? "1" : "0") }
        )
        return out
    }

    /// The engine's text (when connected) followed by the client's gauges.
    public static func combine(engine: String?, client: String) -> String {
        var out = engine ?? ""
        if !out.isEmpty, !out.hasSuffix("\n") {
            out += "\n"
        }
        return out + client
    }

    private static func escape(_ value: String) -> String {
        value
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
            .replacingOccurrences(of: "\n", with: "\\n")
    }
}
