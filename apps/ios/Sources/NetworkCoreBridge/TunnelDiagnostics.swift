import Foundation

/// Messages the app sends to the packet tunnel with `sendProviderMessage`.
///
/// Any payload other than the diagnostics request, including the empty payload older apps send, asks for the
/// runtime status JSON, so an older app keeps working against a newer extension.
public enum TunnelAppMessage: Equatable {
    case runtimeStatus
    case diagnostics
    /// A runtime control request (`RuntimeControl`), answered with the runtime's JSON.
    case control(Data)
    /// The extension's `TrafficStats`, answered as JSON.
    case trafficStats
    /// Clears the extension's `TrafficStats`; answered with the empty stats.
    case clearTrafficStats

    private static let diagnosticsPayload = Data("diagnostics".utf8)
    private static let trafficStatsPayload = Data("traffic-stats".utf8)
    private static let clearTrafficStatsPayload = Data("traffic-stats-clear".utf8)
    private static let controlPrefix = Data("control:".utf8)

    public var encoded: Data {
        switch self {
        case .runtimeStatus:
            return Data()
        case .diagnostics:
            return Self.diagnosticsPayload
        case let .control(request):
            return Self.controlPrefix + request
        case .trafficStats:
            return Self.trafficStatsPayload
        case .clearTrafficStats:
            return Self.clearTrafficStatsPayload
        }
    }

    public static func decode(_ data: Data) -> TunnelAppMessage {
        if data == diagnosticsPayload {
            return .diagnostics
        }
        if data == trafficStatsPayload {
            return .trafficStats
        }
        if data == clearTrafficStatsPayload {
            return .clearTrafficStats
        }
        if data.starts(with: controlPrefix) {
            return .control(Data(data.dropFirst(controlPrefix.count)))
        }
        return .runtimeStatus
    }
}

/// One line of the extension's event log. Messages are fixed strings and codes, never profile content.
public struct TunnelDiagnosticEvent: Codable, Equatable {
    public let time: Date
    public let message: String

    public init(time: Date, message: String) {
        self.time = time
        self.message = message
    }
}

/// In-memory ring of the extension's most recent events, for developer mode.
public final class TunnelDiagnosticLog: @unchecked Sendable {
    public static let defaultCapacity = 200

    private let lock = NSLock()
    private let capacity: Int
    private var events: [TunnelDiagnosticEvent] = []

    public init(capacity: Int = TunnelDiagnosticLog.defaultCapacity) {
        self.capacity = max(1, capacity)
    }

    public func record(_ message: String, at time: Date = Date()) {
        lock.lock()
        defer { lock.unlock() }
        events.append(TunnelDiagnosticEvent(time: time, message: message))
        if events.count > capacity {
            events.removeFirst(events.count - capacity)
        }
    }

    public func snapshot() -> [TunnelDiagnosticEvent] {
        lock.lock()
        defer { lock.unlock() }
        return events
    }
}

/// The extension's answer to `TunnelAppMessage.diagnostics`.
public struct TunnelDiagnostics: Codable, Equatable {
    /// The runtime's sanitized status JSON as text, nil while no runtime session exists.
    public var runtimeStatus: String?
    public var memoryFootprintBytes: UInt64?
    /// Highest footprint since the extension started.
    public var peakMemoryFootprintBytes: UInt64?
    /// Remaining headroom before the system memory limit, nil where the system does not report it.
    public var memoryAvailableBytes: UInt64?
    public var uptimeSeconds: Double?
    public var runtimeABIVersion: UInt32
    public var runtimeSchemaVersion: UInt32
    public var runtimeFeatureSetVersion: UInt32
    public var events: [TunnelDiagnosticEvent]
    /// The engine's metrics with anonymous labels (Prometheus text), nil before a runtime starts or from an older
    /// extension.
    public var engineMetrics: String?

    enum CodingKeys: String, CodingKey {
        case runtimeStatus = "runtime_status"
        case memoryFootprintBytes = "memory_footprint_bytes"
        case peakMemoryFootprintBytes = "peak_memory_footprint_bytes"
        case memoryAvailableBytes = "memory_available_bytes"
        case uptimeSeconds = "uptime_seconds"
        case runtimeABIVersion = "runtime_abi_version"
        case runtimeSchemaVersion = "runtime_schema_version"
        case runtimeFeatureSetVersion = "runtime_feature_set_version"
        case events
        case engineMetrics = "engine_metrics"
    }

    public init(
        runtimeStatus: String?,
        memoryFootprintBytes: UInt64?,
        peakMemoryFootprintBytes: UInt64? = nil,
        memoryAvailableBytes: UInt64? = nil,
        uptimeSeconds: Double?,
        handshake: NetworkCoreRuntimeFFI.Handshake,
        events: [TunnelDiagnosticEvent],
        engineMetrics: String? = nil
    ) {
        self.runtimeStatus = runtimeStatus
        self.memoryFootprintBytes = memoryFootprintBytes
        self.peakMemoryFootprintBytes = peakMemoryFootprintBytes
        self.memoryAvailableBytes = memoryAvailableBytes
        self.uptimeSeconds = uptimeSeconds
        runtimeABIVersion = handshake.abiVersion
        runtimeSchemaVersion = handshake.dtoSchemaVersion
        runtimeFeatureSetVersion = handshake.featureSetVersion
        self.events = events
        self.engineMetrics = engineMetrics
    }

    /// The extension's gauges and the engine's metrics in the Prometheus text format, for the diagnostics export.
    public var prometheusText: String {
        var lines: [String] = []
        func gauge(_ name: String, _ help: String, _ value: String?) {
            guard let value = value else {
                return
            }
            lines.append("# HELP \(name) \(help)")
            lines.append("# TYPE \(name) gauge")
            lines.append("\(name) \(value)")
        }
        gauge("anixops_extension_memory_bytes", "Memory footprint of the packet tunnel extension.", memoryFootprintBytes.map(String.init))
        gauge("anixops_extension_memory_peak_bytes", "Highest memory footprint since the extension started.", peakMemoryFootprintBytes.map(String.init))
        gauge("anixops_extension_memory_available_bytes", "Headroom before the system memory limit.", memoryAvailableBytes.map(String.init))
        gauge("anixops_extension_uptime_seconds", "Seconds since the extension started.", uptimeSeconds.map { String(format: "%.3f", $0) })
        var text = lines.joined(separator: "\n") + "\n"
        if let engineMetrics = engineMetrics, !engineMetrics.isEmpty {
            text += engineMetrics.hasSuffix("\n") ? engineMetrics : engineMetrics + "\n"
        }
        return text
    }

    public func encoded() throws -> Data {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .millisecondsSince1970
        return try encoder.encode(self)
    }

    public static func decoded(from data: Data) throws -> TunnelDiagnostics {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .millisecondsSince1970
        return try decoder.decode(TunnelDiagnostics.self, from: data)
    }
}

extension EmbeddedRuntimeConfig {
    /// The outbound protocol name, safe to show and copy: it carries no address or key.
    public var outboundKind: String {
        switch outbound {
        case let .shadowsocks(_, _, method, _):
            return "shadowsocks (\(method))"
        case .wireguard:
            return "wireguard"
        case let .vless(vless):
            return vless.flow.isEmpty ? "vless+reality" : "vless+reality (\(vless.flow))"
        case let .native(native):
            return native.type
        }
    }
}

/// Plain-text report developer mode copies to the clipboard.
///
/// It carries only what the app and extension already sanitize: versions, states, fixed failure reasons,
/// counters and event lines. Node names, addresses and keys stay out.
public struct DeveloperReport: Equatable {
    public var appVersion: String
    public var appBuild: String
    public var systemVersion: String
    public var connection: String
    public var outboundKind: String?
    public var lastDisconnectError: String?
    public var diagnostics: TunnelDiagnostics?
    public var generatedAt: Date
    /// The client health section: install integrity and the redacted crash reports (`ClientHealth`).
    public var health: String?

    public init(
        appVersion: String,
        appBuild: String,
        systemVersion: String,
        connection: String,
        outboundKind: String?,
        lastDisconnectError: String?,
        diagnostics: TunnelDiagnostics?,
        generatedAt: Date,
        health: String? = nil
    ) {
        self.appVersion = appVersion
        self.appBuild = appBuild
        self.systemVersion = systemVersion
        self.connection = connection
        self.outboundKind = outboundKind
        self.lastDisconnectError = lastDisconnectError
        self.diagnostics = diagnostics
        self.generatedAt = generatedAt
        self.health = health
    }

    public var text: String {
        var lines = [
            "NetworkCore diagnostics",
            "generated: \(Self.timestamp(generatedAt))",
            "app: \(appVersion) (\(appBuild))",
            "system: \(systemVersion)",
            "connection: \(connection)",
            "outbound: \(outboundKind ?? "none")",
        ]
        if let lastDisconnectError = lastDisconnectError {
            lines.append("last disconnect error:")
            lines.append(lastDisconnectError)
        }
        if let health = health, !health.isEmpty {
            lines.append(health.hasSuffix("\n") ? String(health.dropLast()) : health)
        }
        guard let diagnostics = diagnostics else {
            lines.append("extension: not reachable")
            return lines.joined(separator: "\n")
        }
        lines.append(
            "runtime abi/schema/features: \(diagnostics.runtimeABIVersion)/\(diagnostics.runtimeSchemaVersion)/\(diagnostics.runtimeFeatureSetVersion)"
        )
        lines.append("extension memory: \(diagnostics.memoryFootprintBytes.map(Self.megabytes) ?? "unknown")")
        lines.append("extension memory peak: \(diagnostics.peakMemoryFootprintBytes.map(Self.megabytes) ?? "unknown")")
        lines.append("extension memory headroom: \(diagnostics.memoryAvailableBytes.map(Self.megabytes) ?? "unknown")")
        lines.append("extension uptime: \(diagnostics.uptimeSeconds.map(Self.duration) ?? "unknown")")
        lines.append("runtime status:")
        lines.append(diagnostics.runtimeStatus.map(Self.prettyJSON) ?? "none")
        lines.append("events:")
        if diagnostics.events.isEmpty {
            lines.append("none")
        }
        for event in diagnostics.events {
            lines.append("\(Self.timestamp(event.time)) \(event.message)")
        }
        lines.append("metrics:")
        lines.append(diagnostics.prometheusText.trimmingCharacters(in: .newlines))
        return lines.joined(separator: "\n")
    }

    /// Re-indents a JSON object with sorted keys; text that is not JSON comes back unchanged.
    public static func prettyJSON(_ text: String) -> String {
        guard let data = text.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data),
              let pretty = try? JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys]),
              let result = String(data: pretty, encoding: .utf8)
        else {
            return text
        }
        return result
    }

    public static func megabytes(_ bytes: UInt64) -> String {
        String(format: "%.1f MB", Double(bytes) / 1_048_576)
    }

    public static func duration(_ seconds: Double) -> String {
        let total = Int(seconds.rounded(.down))
        let hours = total / 3600
        let minutes = (total % 3600) / 60
        let secs = total % 60
        return hours > 0 ? "\(hours)h \(minutes)m \(secs)s" : minutes > 0 ? "\(minutes)m \(secs)s" : "\(secs)s"
    }

    public static func timestamp(_ date: Date) -> String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.string(from: date)
    }
}
