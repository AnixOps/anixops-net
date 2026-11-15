import Foundation
import NetworkCoreBridge
import NetworkCoreRuntimeC

/// Undoes a captured body's `Content-Encoding` with the kernel (crates/engine-embedded `networkcore_ios_decode_body`),
/// in the app's own process: gzip, deflate, br and zstd, several in a row, the same on every platform. At most
/// 16 MiB comes out, so a compression bomb stops there.
enum CaptureBodyDecoder {
    struct Decoded {
        let data: Data
        /// Only the start came out: the body was kept only in part, or decoding reached the limit.
        let truncated: Bool
    }

    /// Nil when there is no encoding to undo, or the kernel cannot undo it (an unknown coding, a damaged body, a
    /// window over the limit); the bytes are then shown as captured.
    static func decode(_ body: PacketCapture.Body, contentEncoding: String?) -> Decoded? {
        guard let encoding = contentEncoding?.trimmingCharacters(in: .whitespaces), !encoding.isEmpty,
              encoding.lowercased() != "identity", !body.data.isEmpty
        else {
            return nil
        }
        let complete: UInt8 = !body.truncated && UInt64(body.data.count) == body.size ? 1 : 0
        let coding = Array(encoding.utf8)
        var truncated: UInt8 = 0
        var buffer = NetworkCoreBuffer(ptr: nil, len: 0, cap: 0)
        let code = body.data.withUnsafeBytes { (input: UnsafeRawBufferPointer) -> Int32 in
            coding.withUnsafeBufferPointer { name in
                networkcore_ios_decode_body(
                    input.bindMemory(to: UInt8.self).baseAddress,
                    input.count,
                    name.baseAddress,
                    name.count,
                    complete,
                    &truncated,
                    &buffer
                )
            }
        }
        defer { networkcore_ios_runtime_free_buffer(buffer) }
        guard code == NETWORKCORE_IOS_OK else {
            return nil
        }
        let data = buffer.ptr.map { Data(bytes: $0, count: buffer.len) } ?? Data()
        return Decoded(data: data, truncated: truncated != 0)
    }
}
