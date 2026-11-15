import Foundation

extension PacketCapture {
    /// Where an exchange sits in the capture: its connection and its place in that connection's `http` array. It
    /// stays the same while the exchange is under way and after it ends, unlike `seq`.
    public struct ExchangeKey: Hashable, Sendable {
        public let connectionID: UInt64
        public let index: Int

        public init(connectionID: UInt64, index: Int) {
            self.connectionID = connectionID
            self.index = index
        }
    }

    /// One `capture_requests` row: a decrypted HTTP exchange, filed under its connection.
    public struct RequestRow: Decodable, Equatable, Identifiable, Sendable {
        /// The order the exchange ended in; 0 while it is still under way.
        public let seq: UInt64
        public let connectionID: UInt64
        /// Its place in the connection's `http` array (`capture_connection`).
        public let index: Int
        public let method: String
        public let url: String
        public let host: String
        public let status: Int?
        public let note: String?
        /// `HTTP/1.1` or `HTTP/2`.
        public let httpVersion: String?
        public let startedAt: Date
        public let endedAt: Date?
        public let requestSize: UInt64
        public let responseSize: UInt64
        public let responseContentType: String?
        /// Bumped by the engine on every change to the exchange (`since_rev`); 0 from kernels before it.
        public let rev: UInt64
        /// The request or response is still under way: its row shows up as soon as the request head is read and is
        /// updated as the response arrives.
        public let inProgress: Bool
        /// A rewrite rule that would have read or changed the body did not run: the response was relayed as a
        /// stream (an event stream) or was over the size rules can hold.
        public let rulesSkipped: Bool
        /// Server-sent events or WebSocket messages seen so far.
        public let messageCount: UInt64

        public var id: ExchangeKey {
            ExchangeKey(connectionID: connectionID, index: index)
        }

        enum CodingKeys: String, CodingKey {
            case seq
            case connectionID = "connection_id"
            case index, method, url, host, status, note
            case httpVersion = "http_version"
            case startedAtMs = "started_at_ms"
            case endedAtMs = "ended_at_ms"
            case requestSize = "request_size"
            case responseSize = "response_size"
            case responseContentType = "response_content_type"
            case rev, state
            case rulesSkipped = "rules_skipped"
            case messageCount = "message_count"
        }

        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            seq = try container.decode(UInt64.self, forKey: .seq)
            connectionID = try container.decode(UInt64.self, forKey: .connectionID)
            index = try container.decode(Int.self, forKey: .index)
            method = try container.decode(String.self, forKey: .method)
            url = try container.decode(String.self, forKey: .url)
            host = try container.decodeIfPresent(String.self, forKey: .host) ?? ""
            status = try container.decodeIfPresent(Int.self, forKey: .status)
            note = try container.decodeIfPresent(String.self, forKey: .note)
            httpVersion = try container.decodeIfPresent(String.self, forKey: .httpVersion)
            startedAt = PacketCapture.date(try container.decode(UInt64.self, forKey: .startedAtMs))
            endedAt = try container.decodeIfPresent(UInt64.self, forKey: .endedAtMs).map(PacketCapture.date)
            requestSize = try container.decodeIfPresent(UInt64.self, forKey: .requestSize) ?? 0
            responseSize = try container.decodeIfPresent(UInt64.self, forKey: .responseSize) ?? 0
            responseContentType = try container.decodeIfPresent(String.self, forKey: .responseContentType)
            rev = try container.decodeIfPresent(UInt64.self, forKey: .rev) ?? 0
            inProgress = try container.decodeIfPresent(String.self, forKey: .state) == "in_progress"
            rulesSkipped = try container.decodeIfPresent(Bool.self, forKey: .rulesSkipped) ?? false
            messageCount = try container.decodeIfPresent(UInt64.self, forKey: .messageCount) ?? 0
        }

        /// What carries the messages: `SSE` for an event stream, `WS` for a WebSocket; nil for neither.
        public var streamKind: String? {
            if method == "GET", status == 101 {
                return "WS"
            }
            if responseContentType?.lowercased().hasPrefix("text/event-stream") == true {
                return "SSE"
            }
            return messageCount > 0 ? "WS" : nil
        }

        /// The path and query, for the row's second line.
        public var path: String {
            guard let schemeEnd = url.range(of: "://") else {
                return url
            }
            let rest = url[schemeEnd.upperBound...]
            guard let slash = rest.firstIndex(where: { $0 == "/" || $0 == "?" }) else {
                return "/"
            }
            return String(rest[slash...])
        }

        /// A short kind of the response body: JSON, HTML, image and so on, from its Content-Type.
        public var kind: String? {
            guard let type = responseContentType?.lowercased().split(separator: ";").first?.trimmingCharacters(in: .whitespaces),
                  !type.isEmpty
            else {
                return nil
            }
            if type.contains("json") {
                return "JSON"
            }
            if type.contains("html") {
                return "HTML"
            }
            if type.contains("xml") {
                return "XML"
            }
            if type.contains("javascript") {
                return "JS"
            }
            if type == "text/css" {
                return "CSS"
            }
            if type.hasPrefix("image/") {
                return "IMG"
            }
            if type.hasPrefix("video/") || type.hasPrefix("audio/") {
                return "MEDIA"
            }
            if type.contains("protobuf") || type.contains("grpc") {
                return "PB"
            }
            if type.hasPrefix("text/") {
                return "TEXT"
            }
            return nil
        }

        public var isHTTP2: Bool {
            httpVersion == "HTTP/2"
        }

        public var duration: TimeInterval? {
            endedAt.map { max(0, $0.timeIntervalSince(startedAt)) }
        }
    }

    /// The `capture_requests` answer, or the filter's parse error.
    public struct RequestsAnswer: Decodable, Equatable, Sendable {
        public let requests: [RequestRow]
        public let lastSeq: UInt64
        /// The newest change in the capture (`since_rev`); nil from kernels that list only ended exchanges.
        public let lastRev: UInt64?
        /// Rows up to this `seq` were dropped with their connection (capture budget, clear, a loaded session).
        public let droppedBeforeSeq: UInt64
        /// The 0-based character the filter stopped parsing at.
        public let invalidFilterAt: Int?

        enum CodingKeys: String, CodingKey {
            case requests
            case lastSeq = "last_seq"
            case lastRev = "last_rev"
            case droppedBeforeSeq = "dropped_before_seq"
            case error
        }

        struct Failure: Decodable {
            let code: String
            let at: Int?
        }

        public init(
            requests: [RequestRow],
            lastSeq: UInt64,
            lastRev: UInt64? = nil,
            droppedBeforeSeq: UInt64,
            invalidFilterAt: Int? = nil
        ) {
            self.requests = requests
            self.lastSeq = lastSeq
            self.lastRev = lastRev
            self.droppedBeforeSeq = droppedBeforeSeq
            self.invalidFilterAt = invalidFilterAt
        }

        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            if let failure = try container.decodeIfPresent(Failure.self, forKey: .error) {
                guard failure.code == "invalid_filter" else {
                    throw DecodingError.dataCorruptedError(forKey: .error, in: container, debugDescription: failure.code)
                }
                self.init(requests: [], lastSeq: 0, droppedBeforeSeq: 0, invalidFilterAt: failure.at ?? 0)
                return
            }
            self.init(
                requests: try container.decode([RequestRow].self, forKey: .requests),
                lastSeq: try container.decode(UInt64.self, forKey: .lastSeq),
                lastRev: try container.decodeIfPresent(UInt64.self, forKey: .lastRev),
                droppedBeforeSeq: try container.decodeIfPresent(UInt64.self, forKey: .droppedBeforeSeq) ?? 0
            )
        }
    }
}

/// The quick filters above the HTTP request list. Each is a piece of the kernel's filter expression (see
/// `CaptureFilter`), so a chip and the same text typed into the search box do the same thing.
public enum CaptureQuickFilter: String, CaseIterable, Identifiable, Sendable {
    case errors
    case json
    case images
    case writes
    case http2
    case large
    case mapped

    public var id: String {
        rawValue
    }

    /// The expression the chip stands for.
    public var expression: String {
        switch self {
        case .errors:
            return "status:>=400 OR status:none"
        case .json:
            return "type:json"
        case .images:
            return "type:image"
        case .writes:
            return "-method:GET -method:HEAD -method:OPTIONS"
        case .http2:
            return "version:h2"
        case .large:
            return "size:>100k"
        case .mapped:
            return "note:answered_locally"
        }
    }
}

/// Builds the `capture_requests` filter from the chips that are on and what was typed, all in the one syntax the
/// kernel parses: terms are ANDed, `OR` (upper case) joins groups, `-` negates, quotes keep spaces.
public enum CaptureFilter {
    /// The full expression. AND binds tighter than OR, so the chips' terms go into every OR group of the typed text,
    /// and a chip that is itself an OR (errors) is multiplied out with the rest.
    public static func compose(chips: [CaptureQuickFilter], text: String) -> String {
        var groups: [[String]] = [[]]
        for part in chips.map(\.expression) + [text] {
            let alternatives = orGroups(part)
            guard !alternatives.isEmpty else {
                continue
            }
            groups = groups.flatMap { group in
                alternatives.map { group + $0 }
            }
        }
        return groups
            .map { $0.joined(separator: " ") }
            .filter { !$0.isEmpty }
            .joined(separator: " OR ")
    }

    /// Where a parse error the kernel reported at `at` of `compose(chips:text:)` falls in the typed text, when the
    /// chips' terms all come before it unchanged (no `OR` anywhere); nil otherwise.
    public static func textPosition(of at: Int, chips: [CaptureQuickFilter], text: String) -> Int? {
        let prefix = chips.map(\.expression).joined(separator: " ")
        let trimmed = text.trimmingCharacters(in: .whitespaces)
        guard !prefix.contains(" OR "), orGroups(trimmed).count <= 1 else {
            return nil
        }
        let offset = prefix.isEmpty ? 0 : prefix.count + 1
        let leading = text.prefix { $0 == " " }.count
        guard at >= offset else {
            return nil
        }
        return min(at - offset + leading, text.count)
    }

    /// The terms of each `OR` group, quotes kept as written.
    static func orGroups(_ text: String) -> [[String]] {
        var groups: [[String]] = []
        var current: [String] = []
        for token in tokens(text) {
            if token == "OR" {
                if !current.isEmpty {
                    groups.append(current)
                }
                current = []
            } else {
                current.append(token)
            }
        }
        if !current.isEmpty {
            groups.append(current)
        }
        return groups
    }

    /// Splits on spaces outside double quotes.
    static func tokens(_ text: String) -> [String] {
        var tokens: [String] = []
        var current = ""
        var quoted = false
        var escaped = false
        for character in text {
            if escaped {
                current.append(character)
                escaped = false
                continue
            }
            if character == "\\" && quoted {
                current.append(character)
                escaped = true
                continue
            }
            if character == "\"" {
                quoted.toggle()
                current.append(character)
                continue
            }
            if character.isWhitespace && !quoted {
                if !current.isEmpty {
                    tokens.append(current)
                    current = ""
                }
                continue
            }
            current.append(character)
        }
        if !current.isEmpty {
            tokens.append(current)
        }
        return tokens
    }
}

/// The rows the request list shows, kept in step with the engine. With a kernel that numbers changes (`since_rev`),
/// a request shows up as soon as its head is read and its row is updated in place as the response arrives; rows are
/// in the order they first appeared. Older kernels list only ended exchanges, after `nextSeq`. Rows the engine
/// dropped go away; `maxRows` keeps the newest.
public struct CaptureRequestLog: Equatable, Sendable {
    public static let pageSize = 500
    public static let maxRows = 5000

    public private(set) var rows: [PacketCapture.RequestRow] = []
    /// What to ask for next (`since_seq`), for kernels without `since_rev`.
    public private(set) var nextSeq: UInt64 = 0
    /// What to ask for next (`since_rev`).
    public private(set) var nextRev: UInt64 = 0
    /// Where each row is in `rows`.
    private var positions: [PacketCapture.ExchangeKey: Int] = [:]

    public init() {}

    /// Takes in one answer to `capture_requests(sinceSeq: nextSeq, sinceRev: nextRev, …)`. Returns whether a full
    /// page came, so there may be more to read right away.
    @discardableResult
    public mutating func apply(_ answer: PacketCapture.RequestsAnswer, limit: Int = pageSize) -> Bool {
        if answer.droppedBeforeSeq > 0 {
            // A row still under way has no `seq` yet and is kept; if its connection went, it is simply not updated
            // again, and opening it says it is gone.
            removeRows { $0.seq != 0 && $0.seq <= answer.droppedBeforeSeq }
        }
        let full = answer.requests.count >= limit
        if let lastRev = answer.lastRev {
            for row in answer.requests {
                if let position = positions[row.id] {
                    rows[position] = row
                } else {
                    positions[row.id] = rows.count
                    rows.append(row)
                }
            }
            if full, let last = answer.requests.last?.rev {
                nextRev = max(nextRev, last)
            } else {
                nextRev = max(nextRev, lastRev)
            }
        } else {
            for row in answer.requests where row.seq > nextSeq && positions[row.id] == nil {
                positions[row.id] = rows.count
                rows.append(row)
            }
        }
        if full, let last = answer.requests.last?.seq, answer.lastRev == nil {
            // A filtered page can stop short of `last_seq`: carry on from the last row read.
            nextSeq = max(nextSeq, last)
        } else {
            nextSeq = max(nextSeq, answer.lastSeq)
        }
        if rows.count > Self.maxRows {
            rows.removeFirst(rows.count - Self.maxRows)
            reindex()
        }
        // A cleared capture or a loaded session starts numbering above everything shown, which the drop above has
        // already removed; the next read then starts from there.
        return full
    }

    /// Forgets everything, for a new filter.
    public mutating func reset() {
        rows = []
        positions = [:]
        nextSeq = 0
        nextRev = 0
    }

    private mutating func removeRows(where drop: (PacketCapture.RequestRow) -> Bool) {
        let before = rows.count
        rows.removeAll(where: drop)
        if rows.count != before {
            reindex()
        }
    }

    private mutating func reindex() {
        positions = Dictionary(uniqueKeysWithValues: rows.enumerated().map { ($0.element.id, $0.offset) })
    }
}
