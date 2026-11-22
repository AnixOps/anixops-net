import Foundation

/// Binary bodies the inspector can still show as something: pictures recognised by their first bytes, and Protobuf
/// (bare, or framed as gRPC) decoded without a schema, the way `protoc --decode_raw` does. Everything here reads bytes
/// from the network, so every length is checked against what is there, work stops at fixed limits, and the answer
/// is nil rather than a crash. The rules, limits and printed text are the same as the desktop's `binary-view.ts` and
/// Android's `CaptureBinary.kt`; docs/body-preview.md describes them.
public enum CaptureBinary {
    /// Pictures larger than this many pixels are not drawn: a few kilobytes of PNG can claim 65535 × 65535.
    public static let maxImagePixels = 67_108_864
    /// The longest side of a picture as the inspector draws it.
    public static let previewPixels = 2048
    /// The most bytes of a body decoded as Protobuf; the rest is left out, as with a body capture cut.
    public static let maxProtoBytes = 2 * 1024 * 1024
    /// How deep messages inside messages are opened; deeper ones are shown as bytes.
    public static let maxDepth = 16
    /// The most fields shown in all; decoding stops there and says so.
    public static let maxFields = 20000
    /// How many bytes, in all, are looked at again to tell text from a message inside a length-delimited field.
    public static let maxScan = 16 * 1024 * 1024
    /// The most gRPC messages read from one body.
    public static let maxFrames = 1000
    static let stringShown = 1000
    static let bytesShown = 32
    static let maxFieldNumber = 536_870_911

    // MARK: Pictures

    /// The picture type of `data` from its signature, or nil.
    public static func sniffImage(_ data: Data) -> String? {
        let bytes = [UInt8](data.prefix(32))
        func at(_ offset: Int, _ text: String) -> Bool {
            let wanted = Array(text.utf8)
            return bytes.count >= offset + wanted.count && Array(bytes[offset..<offset + wanted.count]) == wanted
        }
        if bytes.count >= 8, bytes[0] == 0x89, at(1, "PNG\r\n\u{1a}\n") {
            return "image/png"
        }
        if bytes.count >= 3, bytes[0] == 0xFF, bytes[1] == 0xD8, bytes[2] == 0xFF {
            return "image/jpeg"
        }
        if at(0, "GIF87a") || at(0, "GIF89a") {
            return "image/gif"
        }
        if at(0, "RIFF"), at(8, "WEBP") {
            return "image/webp"
        }
        if at(4, "ftyp"), at(8, "avif") || at(8, "avis") {
            return "image/avif"
        }
        // "BM" alone is too common in other data: the reserved words must be zero and the header size a known one.
        if bytes.count >= 26, at(0, "BM"), bytes[6...9].allSatisfy({ $0 == 0 }) {
            let header = UInt32(bytes[14]) | UInt32(bytes[15]) << 8 | UInt32(bytes[16]) << 16 | UInt32(bytes[17]) << 24
            if [12, 40, 52, 56, 108, 124].contains(header) {
                return "image/bmp"
            }
        }
        return nil
    }

    // MARK: Saving and opening

    /// The file extension for a body of `contentType`, and whether another app may be asked to open such a file.
    /// Types that run code where they open (HTML, SVG, scripts, XML, CSV) are saved but never opened from here;
    /// anything unknown is `bin`. The same list as `body_type` in crates/desktop-mitm/src/capture.rs.
    public static func fileType(_ contentType: String?) -> (fileExtension: String, opens: Bool) {
        let media = CaptureBody.mediaType(contentType)
        switch media {
        case "image/png": return ("png", true)
        case "image/jpeg", "image/jpg": return ("jpg", true)
        case "image/gif": return ("gif", true)
        case "image/webp": return ("webp", true)
        case "image/avif": return ("avif", true)
        case "image/bmp": return ("bmp", true)
        case "image/x-icon", "image/vnd.microsoft.icon": return ("ico", true)
        case "application/pdf": return ("pdf", true)
        case "text/plain": return ("txt", true)
        case "video/mp4": return ("mp4", true)
        case "video/webm": return ("webm", true)
        case "audio/mpeg": return ("mp3", true)
        case "audio/mp4": return ("m4a", true)
        case "audio/ogg": return ("ogg", true)
        case "audio/wav", "audio/x-wav": return ("wav", true)
        case "application/json": return ("json", true)
        case _ where media.hasSuffix("+json"): return ("json", true)
        case "image/svg+xml": return ("svg", false)
        case "text/html": return ("html", false)
        case "text/css": return ("css", false)
        case "text/csv": return ("csv", false)
        case "text/javascript", "application/javascript": return ("js", false)
        case "application/xml", "text/xml": return ("xml", false)
        case _ where media.hasSuffix("+xml"): return ("xml", false)
        case "application/x-www-form-urlencoded": return ("txt", false)
        case "font/woff2": return ("woff2", false)
        case "font/woff": return ("woff", false)
        case "application/zip": return ("zip", false)
        default: return ("bin", false)
        }
    }

    // MARK: Protobuf

    public indirect enum ProtoValue: Equatable, Sendable {
        case varint(UInt64)
        case fixed64(UInt64)
        case fixed32(UInt32)
        /// Length-delimited bytes that decode as a message.
        case message([ProtoField], size: Int)
        /// Length-delimited bytes that are text.
        case text(String, size: Int)
        /// Other length-delimited bytes: the first few and how many there are.
        case bytes(head: [UInt8], size: Int)
    }

    public struct ProtoField: Equatable, Sendable {
        public let number: Int
        public let value: ProtoValue
    }

    public struct ProtoMessage: Equatable, Sendable {
        public let fields: [ProtoField]
        /// Not every byte was decoded: the body was cut, too long, or had more fields than are shown.
        public let cut: Bool
    }

    /// Media types that carry one bare Protobuf message.
    public static func isProtobufType(_ media: String) -> Bool {
        if isGrpcType(media) {
            return false
        }
        let named = ["application/x-protobuf", "application/protobuf", "application/x-google-protobuf", "application/vnd.google.protobuf", "application/x-protobuffer"]
        return named.contains(media) || media.hasSuffix("+proto")
    }

    /// gRPC and gRPC-Web in their binary form, where each message is framed. `grpc-web-text` is base64 text.
    public static func isGrpcType(_ media: String) -> Bool {
        ["application/grpc", "application/grpc+proto", "application/grpc-web", "application/grpc-web+proto"].contains(media)
    }

    /// The reader and the limits it shares across one body.
    private final class Reader {
        let bytes: [UInt8]
        var fields = CaptureBinary.maxFields
        var scan = CaptureBinary.maxScan
        /// Set once the field limit stopped decoding.
        var spent = false

        init(_ bytes: [UInt8]) {
            self.bytes = bytes
        }

        /// A tag or a length: at most five bytes.
        func small(_ offset: inout Int, _ end: Int) -> Int? {
            var result = 0
            for index in 0..<5 {
                guard offset < end else {
                    return nil
                }
                let byte = bytes[offset]
                offset += 1
                result |= Int(byte & 0x7F) << (7 * index)
                if byte < 0x80 {
                    return result
                }
            }
            return nil
        }

        /// A value: up to ten bytes, 64 bits.
        func varint(_ offset: inout Int, _ end: Int) -> UInt64? {
            var result: UInt64 = 0
            var shift: UInt64 = 0
            while shift < 70 {
                guard offset < end else {
                    return nil
                }
                let byte = bytes[offset]
                offset += 1
                // The tenth byte may only carry the top bit of a 64-bit value.
                if shift == 63, byte > 1 {
                    return nil
                }
                result |= UInt64(byte & 0x7F) << shift
                if byte < 0x80 {
                    return result
                }
                shift += 7
            }
            return nil
        }

        func little(_ offset: Int, _ count: Int) -> UInt64 {
            var value: UInt64 = 0
            for index in 0..<count {
                value |= UInt64(bytes[offset + index]) << (8 * UInt64(index))
            }
            return value
        }

        /// The fields of one message in `bytes[start..<end]`; nil when the bytes are not a whole message. With
        /// `partial`, the fields before the first one that does not decode are kept (the body was cut there).
        func message(_ start: Int, _ end: Int, depth: Int, partial: Bool) -> ProtoMessage? {
            var offset = start
            var fields: [ProtoField] = []
            func stop() -> ProtoMessage? {
                partial && !fields.isEmpty ? ProtoMessage(fields: fields, cut: true) : nil
            }
            while offset < end {
                if self.fields <= 0 {
                    spent = true
                    return ProtoMessage(fields: fields, cut: true)
                }
                guard let tag = small(&offset, end) else {
                    return stop()
                }
                let number = tag >> 3
                guard number >= 1, number <= CaptureBinary.maxFieldNumber else {
                    return stop()
                }
                let value: ProtoValue
                switch tag & 7 {
                case 0:
                    guard let read = varint(&offset, end) else {
                        return stop()
                    }
                    value = .varint(read)
                case 1:
                    guard end - offset >= 8 else {
                        return stop()
                    }
                    value = .fixed64(little(offset, 8))
                    offset += 8
                case 5:
                    guard end - offset >= 4 else {
                        return stop()
                    }
                    value = .fixed32(UInt32(little(offset, 4)))
                    offset += 4
                case 2:
                    guard let length = small(&offset, end), length <= end - offset else {
                        return stop()
                    }
                    value = lengthDelimited(offset, offset + length, depth: depth)
                    offset += length
                default:
                    // Groups (3, 4) are long deprecated and 6, 7 do not exist: not a message this reader shows.
                    return stop()
                }
                self.fields -= 1
                fields.append(ProtoField(number: number, value: value))
            }
            return ProtoMessage(fields: fields, cut: false)
        }

        private func lengthDelimited(_ from: Int, _ to: Int, depth: Int) -> ProtoValue {
            let size = to - from
            let head = Array(bytes[from..<min(to, from + CaptureBinary.bytesShown)])
            guard size <= scan else {
                return .bytes(head: head, size: size)
            }
            scan -= size
            if let text = CaptureBinary.printable(bytes[from..<to]) {
                return .text(text, size: size)
            }
            if depth + 1 < CaptureBinary.maxDepth {
                // A failed try gives back the fields it counted; one stopped by the field limit is kept.
                let before = fields
                if let inner = message(from, to, depth: depth + 1, partial: false), !inner.fields.isEmpty || inner.cut {
                    return .message(inner.fields, size: size)
                }
                fields = before
            }
            return .bytes(head: head, size: size)
        }
    }

    /// The bytes as UTF-8 text when they are text: no control characters besides tab, line feed and carriage return.
    static func printable(_ bytes: ArraySlice<UInt8>) -> String? {
        for byte in bytes where (byte < 0x20 && byte != 0x09 && byte != 0x0A && byte != 0x0D) || byte == 0x7F {
            return nil
        }
        return String(bytes: bytes, encoding: .utf8)
    }

    /// `data` as one Protobuf message without a schema; nil when it is not one. A body that was cut (`cut`) or is
    /// longer than `maxProtoBytes` keeps the fields before the cut. An empty body is an empty message.
    public static func decodeProtobuf(_ data: Data, cut: Bool = false) -> ProtoMessage? {
        let shortened = data.count > maxProtoBytes
        let reader = Reader([UInt8](data.prefix(maxProtoBytes)))
        guard let message = reader.message(0, reader.bytes.count, depth: 0, partial: cut || shortened) else {
            return nil
        }
        return ProtoMessage(fields: message.fields, cut: message.cut || shortened || reader.spent)
    }

    /// A binary body of unknown type that also reads as a Protobuf message, worth offering as a second view.
    public static func looksLikeProtobuf(_ data: Data) -> ProtoMessage? {
        guard !data.isEmpty, let message = decodeProtobuf(data), !message.cut, !message.fields.isEmpty else {
            return nil
        }
        return message
    }

    // MARK: gRPC

    public struct GrpcFrame: Equatable, Sendable {
        /// Compressed with the stream's `grpc-encoding`; the message is not decoded.
        public let compressed: Bool
        /// A gRPC-Web trailer frame: `name: value` lines.
        public let trailers: Bool
        /// The frame's length as its header gives it.
        public let size: Int
        /// The bytes of the frame that are there (fewer than `size` when the body was cut).
        public let data: Data
    }

    public struct GrpcBody: Equatable, Sendable {
        public let frames: [GrpcFrame]
        public let cut: Bool
    }

    /// The length-prefixed frames of a gRPC body; nil when the body does not start with one.
    public static func grpcFrames(_ data: Data) -> GrpcBody? {
        let bytes = [UInt8](data)
        var frames: [GrpcFrame] = []
        var offset = 0
        while offset < bytes.count {
            if frames.count >= maxFrames {
                return GrpcBody(frames: frames, cut: true)
            }
            let flag = bytes[offset]
            guard bytes.count - offset >= 5, flag & 0x7E == 0 else {
                return frames.isEmpty ? nil : GrpcBody(frames: frames, cut: true)
            }
            let size = Int(bytes[offset + 1]) << 24 | Int(bytes[offset + 2]) << 16 | Int(bytes[offset + 3]) << 8 | Int(bytes[offset + 4])
            let start = offset + 5
            let end = min(bytes.count, start + size)
            frames.append(GrpcFrame(compressed: flag & 0x01 != 0, trailers: flag & 0x80 != 0, size: size, data: Data(bytes[start..<end])))
            if start + size > bytes.count {
                return GrpcBody(frames: frames, cut: true)
            }
            offset = end
        }
        return frames.isEmpty ? nil : GrpcBody(frames: frames, cut: false)
    }

    // MARK: Text

    /// The words around decoded Protobuf, in the app's language.
    public struct Labels {
        /// The heading of the `index`th (from 1) gRPC message of `size` bytes.
        public let message: (Int, Int) -> String
        /// A compressed message, which is not decoded.
        public let compressed: String
        /// The heading of gRPC-Web trailers.
        public let trailers: String
        /// A message that does not decode.
        public let undecoded: String
        /// The end of what is shown when not everything was.
        public let more: String

        public init(message: @escaping (Int, Int) -> String, compressed: String, trailers: String, undecoded: String, more: String) {
            self.message = message
            self.compressed = compressed
            self.trailers = trailers
            self.undecoded = undecoded
            self.more = more
        }
    }

    static func hex(_ bytes: some Sequence<UInt8>) -> String {
        bytes.map { String(format: "%02x", $0) }.joined(separator: " ")
    }

    static func quoted(_ text: String) -> String {
        let shown = text.count > stringShown ? String(text.prefix(stringShown)) + "…" : text
        var out = "\""
        for character in shown {
            switch character {
            case "\\": out += "\\\\"
            case "\"": out += "\\\""
            case "\n": out += "\\n"
            case "\r": out += "\\r"
            case "\t": out += "\\t"
            case "\r\n": out += "\\r\\n"
            default: out.append(character)
            }
        }
        return out + "\""
    }

    private static func lines(_ fields: [ProtoField], indent: String, into lines: inout [String]) {
        for field in fields {
            let name = "\(indent)\(field.number)"
            switch field.value {
            case let .varint(value):
                lines.append("\(name): \(value)" + (value >= 1 << 63 ? " (\(Int64(bitPattern: value)))" : ""))
            case let .fixed64(value):
                lines.append("\(name): 0x" + String(format: "%016llx", value) + " (fixed64)")
            case let .fixed32(value):
                lines.append("\(name): 0x" + String(format: "%08x", value) + " (fixed32)")
            case let .message(inner, _):
                lines.append("\(name) {")
                Self.lines(inner, indent: indent + "  ", into: &lines)
                lines.append("\(indent)}")
            case let .text(text, _):
                lines.append("\(name): \(quoted(text))")
            case let .bytes(head, size):
                lines.append("\(name): <\(hex(head))\(size > head.count ? " …" : "")> (\(size) B)")
            }
        }
    }

    /// A decoded message as text, one field per line, in `protoc --decode_raw` style.
    public static func protoText(_ message: ProtoMessage, more: String) -> String {
        var out: [String] = []
        lines(message.fields, indent: "", into: &out)
        if message.cut {
            out.append(more)
        }
        return out.joined(separator: "\n")
    }

    /// A gRPC body as text: each message under a heading, decoded where it can be.
    public static func grpcText(_ body: GrpcBody, labels: Labels) -> String {
        var parts: [String] = []
        var index = 0
        for frame in body.frames {
            if frame.trailers {
                let bytes = [UInt8](frame.data)
                let shown = printable(bytes[...]) ?? hex(bytes.prefix(bytesShown))
                parts.append(trimmedEnd("# \(labels.trailers)\n\(shown)"))
                continue
            }
            index += 1
            let heading = "# \(labels.message(index, frame.size))"
            if frame.compressed {
                parts.append("\(heading)\n\(labels.compressed)")
                continue
            }
            if let message = decodeProtobuf(frame.data, cut: frame.data.count < frame.size) {
                parts.append(trimmedEnd("\(heading)\n\(protoText(message, more: labels.more))"))
            } else {
                parts.append("\(heading)\n\(labels.undecoded)")
            }
        }
        if body.cut {
            parts.append(labels.more)
        }
        return parts.joined(separator: "\n\n")
    }

    private static func trimmedEnd(_ text: String) -> String {
        var text = text
        while let last = text.unicodeScalars.last, CharacterSet.whitespacesAndNewlines.contains(last) {
            text.unicodeScalars.removeLast()
        }
        return text
    }
}
