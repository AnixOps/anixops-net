import Foundation
import NetworkCoreBridge
import NetworkCoreRuntimeC

/// Node checkup (节点体检): handshake and request latency, jitter and loss, and optionally download speed and
/// loaded latency, through the node's own protocol (crates/engine-embedded `networkcore_ios_checkup`, see
/// docs/network-behavior.md). It runs in the app, never through the tunnel, and only when the user starts it.
enum NodeCheckupFFI {
    /// Downloads are capped so a checkup on a metered node spends at most this many bytes.
    static let maxDownloadBytes: UInt64 = 200_000_000

    /// Runs `run` on a dispatch queue, so the blocking call never holds a Swift concurrency thread.
    static func runOffPool(
        _ node: NativeImportReport.Node,
        profileID: String,
        throughput: Bool
    ) async -> Result<NodeCheckupReport, NodeCheckupFailure> {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                continuation.resume(returning: run(node, profileID: profileID, throughput: throughput))
            }
        }
    }

    /// The options document: the kernel's defaults, plus the download suite when asked.
    static func options(throughput: Bool) -> Data {
        guard throughput else {
            return Data()
        }
        return Data(#"{"throughput":{"max_bytes":\#(maxDownloadBytes)}}"#.utf8)
    }

    /// Blocks the calling thread for the whole checkup (about 10 s, about 20 s with the download).
    static func run(
        _ node: NativeImportReport.Node,
        profileID: String,
        throughput: Bool
    ) -> Result<NodeCheckupReport, NodeCheckupFailure> {
        guard let config = NodeDelayFFI.envelope(for: node, profileID: profileID) else {
            return .failure(.unsupported)
        }
        let configBytes = Array(config)
        let optionBytes = Array(options(throughput: throughput))
        var buffer = NetworkCoreBuffer(ptr: nil, len: 0, cap: 0)
        let code = configBytes.withUnsafeBufferPointer { configBuffer in
            optionBytes.withUnsafeBufferPointer { optionBuffer in
                networkcore_ios_checkup(
                    configBuffer.baseAddress, configBuffer.count,
                    optionBuffer.baseAddress, optionBuffer.count,
                    &buffer
                )
            }
        }
        defer { networkcore_ios_runtime_free_buffer(buffer) }
        switch code {
        case NETWORKCORE_IOS_OK:
            guard let base = buffer.ptr, buffer.len > 0, let report = decode(Data(bytes: base, count: buffer.len)) else {
                return .failure(.failed)
            }
            return .success(report)
        case NETWORKCORE_IOS_ERR_CONFIG_REJECTED, NETWORKCORE_IOS_ERR_SCHEMA_MISMATCH:
            // WireGuard and other packet-level outbounds cannot carry a single HTTP request.
            return .failure(.unsupported)
        default:
            return .failure(.failed)
        }
    }

    static func decode(_ data: Data) -> NodeCheckupReport? {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try? decoder.decode(NodeCheckupReport.self, from: data)
    }
}

enum NodeCheckupFailure: Error, Equatable {
    case unsupported
    case failed

    var message: String {
        switch self {
        case .unsupported:
            String(localized: "这种节点不支持体检")
        case .failed:
            String(localized: "体检没有完成")
        }
    }
}

/// `CheckupReport` of crates/engine-embedded/src/checkup.rs. Times are milliseconds, loss is percent.
struct NodeCheckupReport: Decodable, Equatable {
    struct Latency: Decodable, Equatable {
        let samples: Int
        let handshakeMs: Double?
        let coldHandshakeMs: Double?
        let warmHandshakeMs: Double?
        let handshakeLossPct: Double
        let requestMs: Double?
        let requestJitterMs: Double?
        let requestLossPct: Double
        let failure: String?
    }

    struct Throughput: Decodable, Equatable {
        let connections: Int
        let idleMs: Double?
        let mbps: Double
        let peakMbps: Double
        let bytes: UInt64
        let seconds: Double
        let perSecondMbps: [Double]
        let loadedMs: Double?
        let loadedP90Ms: Double?
        let bloatMs: Double?
        let loadedLossPct: Double
        let capped: Bool
        let failure: String?
    }

    let latency: Latency
    let throughput: Throughput?

    /// Why a probe failed, from the kernel's fixed codes.
    static func failureText(_ code: String?) -> String? {
        switch code {
        case nil:
            nil
        case "timeout":
            String(localized: "超时")
        case "connect_failed":
            String(localized: "连接失败")
        case "tls_failed":
            String(localized: "TLS 握手失败")
        case "not_http":
            String(localized: "不是 HTTP 应答")
        case "http_status":
            String(localized: "HTTP 状态异常")
        default:
            String(localized: "失败")
        }
    }
}
