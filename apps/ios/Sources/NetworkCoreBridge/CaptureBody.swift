import Foundation

/// How the request inspector shows a request or response body: what it is, and the ways it can be looked at.
public enum CaptureBody {
    public enum Kind: Equatable, Sendable {
        case json(JsonNode)
        case image
        case form([Pair])
        /// Protobuf decoded without a schema. `labelled`: the Content-Type says Protobuf or gRPC, so it opens decoded;
        /// otherwise the bytes only read as a message, which random bytes can too, so it opens as hex.
        case protobuf(Proto, labelled: Bool)
        case text
        case binary
    }

    /// A body decoded as Protobuf: one message, or gRPC's framed messages.
    public enum Proto: Equatable, Sendable {
        case message(CaptureBinary.ProtoMessage)
        case grpc(CaptureBinary.GrpcBody)
    }

    /// One way of looking at a body. `hex` is always offered, so nothing is ever hidden behind a nicer view.
    public enum Mode: String, CaseIterable, Identifiable, Sendable {
        case tree
        case pretty
        case preview
        case table
        case protobuf
        case raw
        case hex

        public var id: String {
            rawValue
        }
    }

    /// A name and its value: a header, a query parameter or a form field.
    public struct Pair: Equatable, Sendable {
        public let name: String
        public let value: String

        public init(name: String, value: String) {
            self.name = name
            self.value = value
        }
    }

    /// The media type of a `Content-Type` value, lowercased and without parameters: `application/json`.
    public static func mediaType(_ contentType: String?) -> String {
        let first = contentType?.split(separator: ";", maxSplits: 1, omittingEmptySubsequences: false).first ?? ""
        return first.trimmingCharacters(in: .whitespaces).lowercased()
    }

    /// A picture the system can draw. SVG is text, and is shown as such.
    public static func isImage(_ contentType: String?) -> Bool {
        let media = mediaType(contentType)
        return media.hasPrefix("image/") && media != "image/svg+xml"
    }

    /// What a body is. `text` is the body as text (already decompressed), nil when it is not text; `data` is its
    /// bytes, and `cut` whether capture kept only their start. A picture or Protobuf sent with a generic type is
    /// recognised by its bytes.
    public static func kind(contentType: String?, text: String?, data: Data? = nil, cut: Bool = false) -> Kind {
        let media = mediaType(contentType)
        if isImage(contentType) {
            return .image
        }
        if let data, CaptureBinary.isGrpcType(media), let body = CaptureBinary.grpcFrames(data) {
            return .protobuf(.grpc(body), labelled: true)
        }
        if let data, CaptureBinary.isProtobufType(media), let message = CaptureBinary.decodeProtobuf(data, cut: cut) {
            return .protobuf(.message(message), labelled: true)
        }
        guard let text else {
            if let data, !media.hasPrefix("text/"), CaptureBinary.sniffImage(data) != nil {
                return .image
            }
            if let data, let message = CaptureBinary.looksLikeProtobuf(data) {
                return .protobuf(.message(message), labelled: false)
            }
            return .binary
        }
        if media == "application/x-www-form-urlencoded" {
            let pairs = formPairs(text)
            return pairs.isEmpty ? .text : .form(pairs)
        }
        // Plenty of servers send JSON as text/plain, so the text decides as well as the header.
        let start = text.unicodeScalars.first { !CharacterSet.whitespacesAndNewlines.contains($0) && $0 != "\u{FEFF}" }
        let openers: [Unicode.Scalar] = ["{", "["]
        // A body cut short is not whole JSON, so it is not parsed: building most of a large tree only to fail at its
        // end costs the inspector a second for nothing.
        if !cut, media.contains("json") || start.map(openers.contains) == true {
            if let node = JsonDocument.parse(text) {
                return .json(node)
            }
        }
        return .text
    }

    /// The views a body offers, the first being the one it opens in.
    public static func modes(_ kind: Kind) -> [Mode] {
        switch kind {
        case .json:
            return [.tree, .pretty, .raw, .hex]
        case .image:
            return [.preview, .hex]
        case .form:
            return [.table, .raw, .hex]
        case let .protobuf(_, labelled):
            return labelled ? [.protobuf, .hex] : [.hex, .protobuf]
        case .text:
            return [.raw, .hex]
        case .binary:
            return [.hex]
        }
    }

    /// The view to show: the one last chosen when this body offers it, otherwise the body's first.
    public static func chosenMode(_ kind: Kind, preferred: Mode?) -> Mode {
        let offered = modes(kind)
        if let preferred, offered.contains(preferred) {
            return preferred
        }
        return offered[0]
    }

    /// `a=1&b=two+words` as names and values, decoded. A part without `=` is a name with an empty value.
    public static func formPairs(_ text: String) -> [Pair] {
        text.split(separator: "&", omittingEmptySubsequences: true).map { part in
            let pieces = part.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)
            let name = pieces.first.map(String.init) ?? ""
            let value = pieces.count > 1 ? String(pieces[1]) : ""
            return Pair(name: decoded(name), value: decoded(value))
        }
    }

    /// The query parameters of a URL. A `?` after `#` belongs to the fragment and is not a query.
    public static func queryPairs(_ url: String) -> [Pair] {
        let beforeFragment = url.split(separator: "#", maxSplits: 1, omittingEmptySubsequences: false).first ?? ""
        guard let mark = beforeFragment.firstIndex(of: "?") else {
            return []
        }
        return formPairs(String(beforeFragment[beforeFragment.index(after: mark)...]))
    }

    private static func decoded(_ text: String) -> String {
        let spaced = text.replacingOccurrences(of: "+", with: " ")
        return spaced.removingPercentEncoding ?? spaced
    }

    /// Where one request sits on its connection's time axis.
    public struct Bar: Equatable, Sendable {
        /// Where it starts, 0…1 of the axis.
        public let left: Double
        /// How long it took, 0…1 of the axis.
        public let width: Double
        /// Milliseconds from start to end; nil while it is still running.
        public let millis: Int?
    }

    /// The requests of one connection on a shared time axis, from the first start to the last end. A request with
    /// no end runs to `now`. The engine records only each request's start and end, so there are no DNS, connect
    /// or TLS segments to draw.
    public static func waterfall(_ spans: [(start: Date, end: Date?)], now: Date) -> [Bar] {
        guard let first = spans.map(\.start).min() else {
            return []
        }
        let last = spans.map { $0.end ?? max(now, $0.start) }.max() ?? first
        let total = max(last.timeIntervalSince(first), 0.001)
        return spans.map { span in
            let end = span.end ?? max(now, span.start)
            let length = max(end.timeIntervalSince(span.start), 0)
            return Bar(
                left: min(max(span.start.timeIntervalSince(first) / total, 0), 1),
                width: min(length / total, 1),
                millis: span.end == nil ? nil : Int((length * 1000).rounded())
            )
        }
    }
}

/// Long text cut into pieces a lazy list lays out one at a time, for the inspector's code views: one SwiftUI Text
/// holding a whole formatted body is laid out at once, on the main thread. Lines are kept together up to
/// `width` characters a piece; a longer line (minified JSON, a token) is split every `width` characters. Only the
/// first `limit` characters are kept.
public struct CodeTextChunks: Equatable, Sendable {
    public static let maxCharacters = 200_000
    public static let chunkCharacters = 2_000

    public let chunks: [String]
    /// The text was longer than the limit and only its start is in `chunks`.
    public let cut: Bool

    public init(_ text: String, limit: Int = CodeTextChunks.maxCharacters, width: Int = CodeTextChunks.chunkCharacters) {
        let width = max(width, 1)
        let end = text.index(text.startIndex, offsetBy: max(limit, 0), limitedBy: text.endIndex)
        let shown = end.map { text[..<$0] } ?? text[...]
        cut = end.map { $0 < text.endIndex } ?? false
        var chunks: [String] = []
        var current = ""
        var count = 0
        var started = false
        for line in shown.split(omittingEmptySubsequences: false, whereSeparator: \.isNewline) {
            var rest = line
            repeat {
                let stop = rest.index(rest.startIndex, offsetBy: width, limitedBy: rest.endIndex) ?? rest.endIndex
                let piece = rest[..<stop]
                let length = piece.count
                if started, count + 1 + length > width {
                    chunks.append(current)
                    current = ""
                    count = 0
                    started = false
                }
                if started {
                    current += "\n"
                    count += 1
                }
                current += piece
                count += length
                started = true
                rest = rest[stop...]
            } while !rest.isEmpty
        }
        if started || chunks.isEmpty {
            chunks.append(current)
        }
        self.chunks = chunks
    }
}
