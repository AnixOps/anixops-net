import Foundation

extension PacketCapture {
    /// The messages kept of one exchange: server-sent events of an event stream, or the frames of a WebSocket. The
    /// engine keeps the last 512 (64 KiB of data in all, 4 KiB of each), so `dropped` older ones may be missing.
    public struct StreamMessages: Decodable, Equatable, Sendable {
        /// Messages seen, kept or not.
        public let seen: UInt64
        /// The oldest ones dropped to stay within the limits.
        public let dropped: UInt64
        public let kept: [StreamMessage]

        public init(seen: UInt64, dropped: UInt64, kept: [StreamMessage]) {
            self.seen = seen
            self.dropped = dropped
            self.kept = kept
        }
    }

    /// One server-sent event or WebSocket message, as it went by unchanged.
    public struct StreamMessage: Decodable, Equatable, Identifiable, Sendable {
        public enum Kind: String, Decodable, Sendable {
            case sse, text, binary, ping, pong, close
        }

        /// Its place among the exchange's messages, from 0.
        public let index: UInt64
        public let at: Date
        /// From the app (a WebSocket message it sent); events and replies come down.
        public let isUpload: Bool
        public let kind: Kind
        /// The event's `event:` name, or `permessage-deflate` for a WebSocket message that could not be inflated.
        public let label: String?
        /// The event's `id:`.
        public let eventID: String?
        /// A WebSocket close code.
        public let code: Int?
        /// Its whole length; `data` may hold only the start.
        public let size: UInt64
        public let truncated: Bool
        /// The data as text when it is UTF-8.
        public let text: String?
        public let data: Data

        public var id: UInt64 {
            index
        }

        enum CodingKeys: String, CodingKey {
            case index, kind, label, code, size, truncated, text
            case atMs = "at_ms"
            case dir
            case eventID = "id"
            case data = "data_base64"
        }

        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            index = try container.decode(UInt64.self, forKey: .index)
            at = PacketCapture.date(try container.decode(UInt64.self, forKey: .atMs))
            isUpload = try container.decode(String.self, forKey: .dir) == "up"
            kind = try container.decode(Kind.self, forKey: .kind)
            label = try container.decodeIfPresent(String.self, forKey: .label)
            eventID = try container.decodeIfPresent(String.self, forKey: .eventID)
            code = try container.decodeIfPresent(Int.self, forKey: .code)
            size = try container.decode(UInt64.self, forKey: .size)
            truncated = try container.decodeIfPresent(Bool.self, forKey: .truncated) ?? false
            let text = try container.decodeIfPresent(String.self, forKey: .text)
            self.text = text
            if let text {
                data = Data(text.utf8)
            } else {
                data = Data(base64Encoded: try container.decodeIfPresent(String.self, forKey: .data) ?? "") ?? Data()
            }
        }

        /// A short name for the list: the event name, or the WebSocket frame kind.
        public var title: String {
            switch kind {
            case .sse:
                return label ?? "message"
            case .text:
                return "Text"
            case .binary:
                return "Binary"
            case .ping:
                return "Ping"
            case .pong:
                return "Pong"
            case .close:
                return code.map { "Close \($0)" } ?? "Close"
            }
        }

        /// The data as text, or a hex dump when it is not text.
        public var preview: String {
            text ?? PacketCapture.hexDump(data, limit: 512)
        }
    }

    /// Keeps the messages of one exchange in step with the engine while it is under way: each read asks for the
    /// messages from `nextIndex` on and adds the new ones; ones the engine has since dropped stay shown.
    public struct StreamMessageLog: Equatable, Sendable {
        /// At most this many are kept here, the newest.
        public static let maxMessages = 2000

        public private(set) var messages: [StreamMessage] = []
        public private(set) var seen: UInt64 = 0
        /// Seen by the engine but never shown here (dropped before they were read).
        public private(set) var missed: UInt64 = 0

        public init() {}

        /// What to ask for next (`since_message`).
        public var nextIndex: UInt64 {
            messages.last.map { $0.index + 1 } ?? 0
        }

        public mutating func apply(_ answer: StreamMessages) {
            let fresh = answer.kept.filter { $0.index >= nextIndex }
            if let first = fresh.first, first.index > nextIndex {
                missed += first.index - nextIndex
            }
            messages.append(contentsOf: fresh)
            if messages.count > Self.maxMessages {
                missed += UInt64(messages.count - Self.maxMessages)
                messages.removeFirst(messages.count - Self.maxMessages)
            }
            seen = max(seen, answer.seen)
        }
    }

    /// The `capture_exchange` answer: one exchange (nil when it is no longer kept) with its messages from the
    /// `since_message` asked for.
    public struct ExchangeAnswer: Decodable, Equatable {
        public let exchange: Exchange?
    }

    /// One HTTP/2 frame header seen on a decrypted connection. Only the 9-byte header and a short value are kept,
    /// never the payload.
    public struct H2Frame: Decodable, Equatable, Identifiable, Sendable {
        public enum Link: String, Decodable, Sendable {
            /// Between the app and AnixOps.
            case app
            /// Between AnixOps and the server.
            case origin
        }

        public let index: UInt64
        public let at: Date
        public let link: Link
        /// Sent by the app's side of that link: from the app on the app link, from AnixOps on the origin link.
        public let isUpload: Bool
        /// `DATA`, `HEADERS`, `RST_STREAM`, `SETTINGS`, `GOAWAY`, `WINDOW_UPDATE` and so on.
        public let type: String
        public let flags: [String]
        public let stream: UInt32
        public let length: UInt32
        /// The error code of `RST_STREAM` and `GOAWAY`, such as `CANCEL` or `PROTOCOL_ERROR`.
        public let error: String?
        /// `WINDOW_UPDATE`: the increment; `GOAWAY`: the last stream id; `SETTINGS`: how many settings it carries.
        public let value: UInt32?

        public var id: UInt64 {
            index
        }

        enum CodingKeys: String, CodingKey {
            case index, link, dir, type, stream, length, error, value
            case atMs = "at_ms"
            case flags = "flag_names"
        }

        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            index = try container.decode(UInt64.self, forKey: .index)
            at = PacketCapture.date(try container.decode(UInt64.self, forKey: .atMs))
            link = try container.decode(Link.self, forKey: .link)
            isUpload = try container.decode(String.self, forKey: .dir) == "up"
            type = try container.decode(String.self, forKey: .type)
            flags = try container.decodeIfPresent([String].self, forKey: .flags) ?? []
            stream = try container.decode(UInt32.self, forKey: .stream)
            length = try container.decode(UInt32.self, forKey: .length)
            error = try container.decodeIfPresent(String.self, forKey: .error)
            value = try container.decodeIfPresent(UInt32.self, forKey: .value)
        }

        /// The error, increment or count in a few words, for the frame's row.
        public var detail: String? {
            if let error {
                return error
            }
            guard let value else {
                return nil
            }
            switch type {
            case "WINDOW_UPDATE":
                return "+\(value)"
            case "GOAWAY":
                return "last \(value)"
            case "SETTINGS":
                return "\(value) settings"
            default:
                return String(value)
            }
        }

        /// Frames worth a look when tracking down a stall or a reset.
        public var isNotable: Bool {
            error != nil || type == "GOAWAY" || type == "RST_STREAM"
        }
    }

    /// The `capture_h2_frames` answer (`h2_frames` is null when the connection is no longer kept).
    public struct H2FramesAnswer: Decodable, Equatable {
        public struct Frames: Decodable, Equatable {
            public let seen: UInt64
            public let dropped: UInt64
            public let frames: [H2Frame]
        }

        public let frames: Frames?

        enum CodingKeys: String, CodingKey {
            case frames = "h2_frames"
        }
    }
}
