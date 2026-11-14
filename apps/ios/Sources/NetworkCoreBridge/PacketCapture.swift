import Compression
import Foundation

/// Capture (抓包) answers of the engine's `capture`, `capture_connection` and `export_capture` commands, and the
/// helpers the request detail uses to show captured bytes: text when it is text (after undoing gzip, deflate or
/// brotli), a hex dump otherwise.
///
/// Capture is off until the app turns it on; the engine then keeps the first 64 KiB of each direction of every new
/// connection (the app's own bytes: plain text for plain protocols, TLS records for HTTPS) and, for connections HTTPS
/// decryption took over, the decrypted HTTP requests and responses. Everything stays in the extension's memory under
/// one budget (2 MiB by default) and is gone when the tunnel stops.
public enum PacketCapture {
    public enum ExportFormat: String, CaseIterable, Identifiable, Sendable {
        /// Wireshark: every connection as a TCP or UDP flow; decrypted connections carry their HTTP text.
        case pcapng
        /// HAR 1.2 of the decrypted HTTP messages, for browser developer tools, Charles or Proxyman.
        case har

        public var id: String {
            rawValue
        }
    }

    /// The `capture`, `set_capture` and `clear_capture` answer.
    public struct Status: Decodable, Equatable {
        public let enabled: Bool
        public let maxTotalBytes: UInt64
        public let usedBytes: UInt64
        public let connections: Int

        enum CodingKeys: String, CodingKey {
            case enabled
            case maxTotalBytes = "max_total_bytes"
            case usedBytes = "used_bytes"
            case connections
        }

        public init(enabled: Bool, maxTotalBytes: UInt64 = 0, usedBytes: UInt64 = 0, connections: Int = 0) {
            self.enabled = enabled
            self.maxTotalBytes = maxTotalBytes
            self.usedBytes = usedBytes
            self.connections = connections
        }
    }

    /// One header line.
    public struct Header: Decodable, Equatable, Hashable {
        public let name: String
        public let value: String

        public init(name: String, value: String) {
            self.name = name
            self.value = value
        }

        /// The engine writes a header as `[name, value]`.
        public init(from decoder: Decoder) throws {
            var container = try decoder.unkeyedContainer()
            name = try container.decode(String.self)
            value = try container.decode(String.self)
        }
    }

    /// The first bytes of a body and how long it was.
    public struct Body: Decodable, Equatable {
        public let size: UInt64
        public let truncated: Bool
        public let data: Data

        enum CodingKeys: String, CodingKey {
            case size
            case truncated
            case data = "data_base64"
        }

        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            size = try container.decode(UInt64.self, forKey: .size)
            truncated = try container.decodeIfPresent(Bool.self, forKey: .truncated) ?? false
            data = Data(base64Encoded: try container.decode(String.self, forKey: .data)) ?? Data()
        }
    }

    /// One decrypted HTTP request and its response.
    public struct Exchange: Decodable, Equatable {
        public struct Message: Decodable, Equatable {
            public let headers: [Header]
            public let body: Body

            public func header(_ name: String) -> String? {
                headers.first { $0.name.caseInsensitiveCompare(name) == .orderedSame }?.value
            }
        }

        public let startedAt: Date
        public let endedAt: Date?
        public let method: String
        public let url: String
        /// Nil when no response came.
        public let status: Int?
        /// `answered_locally` (a rewrite rule answered), `streaming` (body not kept) or `too_large`.
        public let note: String?
        public let request: Message
        public let response: Message

        enum CodingKeys: String, CodingKey {
            case startedAtMs = "started_at_ms"
            case endedAtMs = "ended_at_ms"
            case method, url, status, note, request, response
        }

        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            startedAt = PacketCapture.date(try container.decode(UInt64.self, forKey: .startedAtMs))
            endedAt = try container.decodeIfPresent(UInt64.self, forKey: .endedAtMs).map(PacketCapture.date)
            method = try container.decode(String.self, forKey: .method)
            url = try container.decode(String.self, forKey: .url)
            status = try container.decodeIfPresent(Int.self, forKey: .status)
            note = try container.decodeIfPresent(String.self, forKey: .note)
            request = try container.decode(Message.self, forKey: .request)
            response = try container.decode(Message.self, forKey: .response)
        }
    }

    /// Bytes one way: `up` from the app, `down` to it.
    public struct Chunk: Decodable, Equatable {
        public let isUpload: Bool
        public let at: Date
        public let data: Data

        enum CodingKeys: String, CodingKey {
            case dir
            case atMs = "at_ms"
            case data = "data_base64"
        }

        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            isUpload = try container.decode(String.self, forKey: .dir) == "up"
            at = PacketCapture.date(try container.decode(UInt64.self, forKey: .atMs))
            data = Data(base64Encoded: try container.decode(String.self, forKey: .data)) ?? Data()
        }
    }

    /// What was kept of one direction.
    public struct DirectionTotals: Decodable, Equatable {
        /// Bytes that went this way while capturing.
        public let seen: UInt64
        /// Bytes kept (at most 64 KiB).
        public let captured: UInt64
        public let truncated: Bool
    }

    /// The `capture_connection` answer for one connection.
    public struct Connection: Decodable, Equatable {
        public let id: UInt64
        public let network: String
        public let decrypted: Bool
        public let upload: DirectionTotals
        public let download: DirectionTotals
        public let chunks: [Chunk]
        public let http: [Exchange]

        /// The first bytes look like TLS records: the content is encrypted end to end.
        public var looksLikeTLS: Bool {
            guard let first = chunks.first?.data, first.count >= 3 else {
                return false
            }
            let bytes = [UInt8](first.prefix(3))
            return (0x14...0x17).contains(bytes[0]) && bytes[1] == 0x03
        }
    }

    /// `{"capture": … | null}`.
    public struct ConnectionAnswer: Decodable, Equatable {
        public let capture: Connection?
    }

    /// The `export_capture` answer: the file, already base64-decoded.
    public struct ExportAnswer: Decodable, Equatable {
        public let format: String
        public let fileName: String
        public let contentType: String
        public let data: Data

        enum CodingKeys: String, CodingKey {
            case format
            case fileName = "file_name"
            case contentType = "content_type"
            case data = "data_base64"
        }

        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            format = try container.decode(String.self, forKey: .format)
            // Only a bare file name is used, whatever the engine sent.
            let name = try container.decode(String.self, forKey: .fileName)
            fileName = (name as NSString).lastPathComponent.isEmpty ? "capture" : (name as NSString).lastPathComponent
            contentType = try container.decode(String.self, forKey: .contentType)
            data = Data(base64Encoded: try container.decode(String.self, forKey: .data)) ?? Data()
        }
    }

    /// Writes an exported file into the temporary folder for the share sheet; older exports are removed first.
    public static func write(
        _ answer: ExportAnswer,
        in directory: URL = FileManager.default.temporaryDirectory
    ) throws -> URL {
        let folder = directory.appendingPathComponent("AnixOps-Capture", isDirectory: true)
        let manager = FileManager.default
        try? manager.removeItem(at: folder)
        try manager.createDirectory(at: folder, withIntermediateDirectories: true)
        let url = folder.appendingPathComponent(answer.fileName)
        try answer.data.write(to: url, options: .atomic)
        return url
    }

    // MARK: - Showing bytes

    /// `data` as text: decompressed by `contentEncoding` (`gzip`, `deflate`, `br`) when given, then UTF-8. Nil when
    /// it is not text.
    public static func text(_ data: Data, contentEncoding: String? = nil) -> String? {
        let plain = decode(data, contentEncoding: contentEncoding) ?? data
        guard !plain.isEmpty else {
            return ""
        }
        if let text = String(data: plain, encoding: .utf8), isPrintable(text) {
            return text
        }
        // A body cut at 64 KiB may end inside a multi-byte character.
        for cut in 1...3 where plain.count > cut {
            if let text = String(data: plain.dropLast(cut), encoding: .utf8), isPrintable(text) {
                return text
            }
        }
        return nil
    }

    /// Undoes `Content-Encoding`; nil for an encoding it does not know or data it cannot decode.
    public static func decode(_ data: Data, contentEncoding: String?) -> Data? {
        guard let encoding = contentEncoding?.lowercased().trimmingCharacters(in: .whitespaces), !encoding.isEmpty else {
            return nil
        }
        switch encoding {
        case "gzip", "x-gzip":
            guard let body = gzipPayload(data) else {
                return nil
            }
            return inflate(body, algorithm: COMPRESSION_ZLIB)
        case "deflate":
            // Usually a zlib stream (2-byte header); raw deflate otherwise.
            if data.count > 2, let inflated = inflate(data.dropFirst(2), algorithm: COMPRESSION_ZLIB) {
                return inflated
            }
            return inflate(data, algorithm: COMPRESSION_ZLIB)
        case "br":
            return inflate(data, algorithm: COMPRESSION_BROTLI)
        default:
            return nil
        }
    }

    /// A hex dump, 16 bytes a line with offsets and printable ASCII, of at most `limit` bytes.
    public static func hexDump(_ data: Data, limit: Int = 16 * 1024) -> String {
        let bytes = [UInt8](data.prefix(limit))
        var lines: [String] = []
        lines.reserveCapacity(bytes.count / 16 + 2)
        var offset = 0
        while offset < bytes.count {
            let row = bytes[offset..<min(offset + 16, bytes.count)]
            var hex = row.map { String(format: "%02x", $0) }.joined(separator: " ")
            hex += String(repeating: " ", count: max(0, 47 - hex.count))
            let ascii = String(row.map { (0x20...0x7e).contains($0) ? Character(UnicodeScalar($0)) : "." })
            lines.append(String(format: "%08x  ", offset) + hex + "  " + ascii)
            offset += 16
        }
        if data.count > limit {
            lines.append("… \(data.count - limit) more bytes")
        }
        return lines.joined(separator: "\n")
    }

    static func date(_ millis: UInt64) -> Date {
        Date(timeIntervalSince1970: Double(millis) / 1000)
    }

    private static func isPrintable(_ text: String) -> Bool {
        var control = 0
        for scalar in text.unicodeScalars {
            if scalar.value < 0x20, scalar != "\n", scalar != "\r", scalar != "\t" {
                control += 1
            }
        }
        return control * 100 <= text.unicodeScalars.count
    }

    /// The deflate stream inside a gzip member (RFC 1952), without its header and trailer.
    private static func gzipPayload(_ data: Data) -> Data? {
        let bytes = [UInt8](data)
        guard bytes.count > 18, bytes[0] == 0x1f, bytes[1] == 0x8b, bytes[2] == 8 else {
            return nil
        }
        let flags = bytes[3]
        var index = 10
        if flags & 0x04 != 0 {
            guard index + 2 <= bytes.count else { return nil }
            index += 2 + Int(bytes[index]) + Int(bytes[index + 1]) << 8
        }
        for flag in [UInt8(0x08), 0x10] where flags & flag != 0 {
            while index < bytes.count, bytes[index] != 0 {
                index += 1
            }
            index += 1
        }
        if flags & 0x02 != 0 {
            index += 2
        }
        guard index < bytes.count else {
            return nil
        }
        return Data(bytes[index...])
    }

    /// Decompresses with the Compression framework, stopping at 8 MiB. A stream cut short (a body kept only in
    /// part) yields what decoded before the cut.
    private static func inflate(_ data: Data, algorithm: compression_algorithm) -> Data? {
        guard !data.isEmpty else {
            return nil
        }
        let limit = 8 * 1024 * 1024
        let source = [UInt8](data)
        var output = Data()
        let pointer = UnsafeMutablePointer<compression_stream>.allocate(capacity: 1)
        defer { pointer.deallocate() }
        guard compression_stream_init(pointer, COMPRESSION_STREAM_DECODE, algorithm) == COMPRESSION_STATUS_OK else {
            return nil
        }
        defer { compression_stream_destroy(pointer) }
        let bufferSize = 64 * 1024
        let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: bufferSize)
        defer { buffer.deallocate() }
        return source.withUnsafeBufferPointer { input -> Data? in
            guard let base = input.baseAddress else {
                return nil
            }
            pointer.pointee.src_ptr = base
            pointer.pointee.src_size = input.count
            while true {
                pointer.pointee.dst_ptr = buffer
                pointer.pointee.dst_size = bufferSize
                let status = compression_stream_process(pointer, Int32(COMPRESSION_STREAM_FINALIZE.rawValue))
                let produced = bufferSize - pointer.pointee.dst_size
                output.append(buffer, count: produced)
                switch status {
                case COMPRESSION_STATUS_OK:
                    if produced == 0 {
                        // No progress: the input ended early.
                        return output.isEmpty ? nil : output
                    }
                    if output.count >= limit {
                        return output
                    }
                case COMPRESSION_STATUS_END:
                    return output
                default:
                    return output.isEmpty ? nil : output
                }
            }
        }
    }
}
