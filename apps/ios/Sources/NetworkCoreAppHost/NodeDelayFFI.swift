import Foundation
import NetworkCoreBridge
import NetworkCoreRuntimeC

/// The delay test, like Loon's and Surge's: an HTTP request to a test URL through the node's own protocol, timed to the
/// first line of the response (crates/engine-embedded `networkcore_ios_measure_delay`). It runs in the app, so it works
/// while the tunnel is down and never goes through the tunnel.
enum NodeDelayFFI {
    /// Same URL as Loon's default delay test.
    static let testURL = "http://www.gstatic.com/generate_204"
    static let timeoutMillis: UInt32 = 5000

    /// Runs `measure` on a dispatch queue, so the blocking call never holds a Swift concurrency thread.
    static func measureOffPool(_ node: NativeImportReport.Node, profileID: String) async -> NodeLatency {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .utility).async {
                continuation.resume(returning: measure(node, profileID: profileID))
            }
        }
    }

    /// Blocks the calling thread for up to `timeoutMillis`.
    static func measure(_ node: NativeImportReport.Node, profileID: String) -> NodeLatency {
        guard let config = envelope(for: node, profileID: profileID) else {
            return .unsupported
        }
        var millis: UInt32 = 0
        let configBytes = Array(config)
        let urlBytes = Array(testURL.utf8)
        let code = configBytes.withUnsafeBufferPointer { configBuffer in
            urlBytes.withUnsafeBufferPointer { urlBuffer in
                networkcore_ios_measure_delay(
                    configBuffer.baseAddress, configBuffer.count,
                    urlBuffer.baseAddress, urlBuffer.count,
                    timeoutMillis, &millis
                )
            }
        }
        switch code {
        case NETWORKCORE_IOS_OK:
            return .millis(Int(millis))
        case NETWORKCORE_IOS_ERR_DELAY_TIMEOUT:
            return .timeout
        case NETWORKCORE_IOS_ERR_CONFIG_REJECTED, NETWORKCORE_IOS_ERR_SCHEMA_MISMATCH:
            // WireGuard and other packet-level outbounds cannot carry a single HTTP request.
            return .unsupported
        default:
            return .failed
        }
    }

    private struct Envelope: Encodable {
        let schemaVersion: Int
        let profileID: String
        let outbound: [String: JSONValue]

        enum CodingKeys: String, CodingKey {
            case schemaVersion = "schema_version"
            case profileID = "profile_id"
            case outbound
        }
    }

    private static func envelope(for node: NativeImportReport.Node, profileID: String) -> Data? {
        try? JSONEncoder().encode(
            Envelope(schemaVersion: EmbeddedRuntimeConfig.schemaVersion, profileID: profileID, outbound: node.outbound)
        )
    }
}
