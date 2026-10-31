import Foundation

/// The cards of the home screen dashboard. Raw values are what the saved layout stores, so they never change.
public enum DashboardCard: String, CaseIterable, Codable, Identifiable, Sendable {
    case speed
    case usage
    case network
    case groups
    case topHosts = "top_hosts"
    case dns
    case exitIP = "exit_ip"
    case events

    public var id: String {
        rawValue
    }

    /// Wide cards take a whole row; the others sit two to a row.
    public var isWide: Bool {
        switch self {
        case .speed, .groups, .topHosts, .events:
            return true
        case .usage, .network, .dns, .exitIP:
            return false
        }
    }
}

/// Which dashboard cards show and in what order, saved as one string such as `speed,usage,-dns`: a leading `-`
/// hides a card. Cards added in later versions that a saved layout does not name show at the end.
public struct DashboardLayout: Equatable, Sendable {
    /// The `UserDefaults` key the home screen saves the layout under.
    public static let storageKey = "home-dashboard-layout"

    public private(set) var order: [DashboardCard]
    public private(set) var hidden: Set<DashboardCard>

    public init(order: [DashboardCard] = DashboardCard.allCases, hidden: Set<DashboardCard> = []) {
        var seen = Set<DashboardCard>()
        var ordered = order.filter { seen.insert($0).inserted }
        ordered += DashboardCard.allCases.filter { !seen.contains($0) }
        self.order = ordered
        self.hidden = hidden
    }

    public init(storage: String) {
        var order: [DashboardCard] = []
        var hidden = Set<DashboardCard>()
        for item in storage.split(separator: ",") {
            let token = item.trimmingCharacters(in: .whitespaces)
            let isHidden = token.hasPrefix("-")
            guard let card = DashboardCard(rawValue: isHidden ? String(token.dropFirst()) : token) else {
                continue
            }
            order.append(card)
            if isHidden {
                hidden.insert(card)
            }
        }
        self.init(order: order, hidden: hidden)
    }

    public var storage: String {
        order.map { hidden.contains($0) ? "-\($0.rawValue)" : $0.rawValue }.joined(separator: ",")
    }

    public var visible: [DashboardCard] {
        order.filter { !hidden.contains($0) }
    }

    public func isShown(_ card: DashboardCard) -> Bool {
        !hidden.contains(card)
    }

    public mutating func setShown(_ card: DashboardCard, _ shown: Bool) {
        if shown {
            hidden.remove(card)
        } else {
            hidden.insert(card)
        }
    }

    /// The same move as a SwiftUI list's `onMove`: the cards at `offsets` go before the card now at `destination`.
    public mutating func move(fromOffsets offsets: IndexSet, toOffset destination: Int) {
        let moving = offsets.filter { order.indices.contains($0) }.map { order[$0] }
        guard !moving.isEmpty else {
            return
        }
        let before = offsets.filter { $0 < destination }.count
        var rest = order.enumerated().filter { !offsets.contains($0.offset) }.map(\.element)
        let index = min(max(0, destination - before), rest.count)
        rest.insert(contentsOf: moving, at: index)
        order = rest
    }

    /// The visible cards laid out in rows: a wide card alone, narrow cards two at a time in order. A narrow card
    /// followed by a wide one, or last, has a row of its own.
    public var rows: [[DashboardCard]] {
        var rows: [[DashboardCard]] = []
        var pending: DashboardCard?
        for card in visible {
            if card.isWide {
                if let narrow = pending {
                    rows.append([narrow])
                    pending = nil
                }
                rows.append([card])
            } else if let narrow = pending {
                rows.append([narrow, card])
                pending = nil
            } else {
                pending = card
            }
        }
        if let narrow = pending {
            rows.append([narrow])
        }
        return rows
    }
}

/// The last minute of tunnel speed, one sample a second, for the speed card's curve.
public struct TrafficHistory: Equatable, Sendable {
    public struct Sample: Equatable, Sendable {
        public let upload: UInt64
        public let download: UInt64

        public init(upload: UInt64, download: UInt64) {
            self.upload = upload
            self.download = download
        }
    }

    public let capacity: Int
    public private(set) var samples: [Sample] = []

    public init(capacity: Int = 60) {
        self.capacity = max(2, capacity)
    }

    public mutating func append(upload: UInt64, download: UInt64) {
        samples.append(Sample(upload: upload, download: download))
        if samples.count > capacity {
            samples.removeFirst(samples.count - capacity)
        }
    }

    public mutating func reset() {
        samples.removeAll()
    }

    /// The highest rate in the window, at least 1 so the curve can divide by it.
    public var peak: UInt64 {
        max(1, samples.map { max($0.upload, $0.download) }.max() ?? 1)
    }
}

/// One host's share of this session's traffic, for the top hosts card.
public struct HostTraffic: Equatable, Identifiable, Sendable {
    public let host: String
    public let bytes: UInt64
    public let connections: Int
    /// The outbound that carried the most bytes for this host.
    public let outbound: String

    public var id: String {
        host
    }

    public init(host: String, bytes: UInt64, connections: Int, outbound: String) {
        self.host = host
        self.bytes = bytes
        self.connections = connections
        self.outbound = outbound
    }

    /// The hosts that moved the most bytes over the active and recently closed connections, largest first.
    /// Connections without a host name count under their destination address without the port.
    public static func top(_ list: RuntimeControl.Connections, limit: Int = 5) -> [HostTraffic] {
        struct Sum {
            var bytes: UInt64 = 0
            var connections = 0
            var byOutbound: [String: UInt64] = [:]
        }
        var sums: [String: Sum] = [:]
        for connection in list.active + list.closed {
            let host = hostName(of: connection)
            let bytes = connection.upload &+ connection.download
            var sum = sums[host, default: Sum()]
            sum.bytes &+= bytes
            sum.connections += 1
            sum.byOutbound[connection.outbound, default: 0] &+= bytes
            sums[host] = sum
        }
        let hosts = sums.map { host, sum in
            let outbound = sum.byOutbound.max { lhs, rhs in
                lhs.value == rhs.value ? lhs.key > rhs.key : lhs.value < rhs.value
            }?.key ?? ""
            return HostTraffic(host: host, bytes: sum.bytes, connections: sum.connections, outbound: outbound)
        }
        return Array(hosts.filter { $0.bytes > 0 }
            .sorted { $0.bytes == $1.bytes ? $0.host < $1.host : $0.bytes > $1.bytes }
            .prefix(max(0, limit)))
    }

    static func hostName(of connection: RuntimeControl.Connection) -> String {
        if let host = connection.host, !host.isEmpty {
            return host
        }
        let destination = connection.destination
        if destination.hasPrefix("["), let close = destination.firstIndex(of: "]") {
            return String(destination[destination.index(after: destination.startIndex)..<close])
        }
        if destination.filter({ $0 == ":" }).count == 1, let colon = destination.lastIndex(of: ":") {
            return String(destination[..<colon])
        }
        return destination
    }
}

/// The public address the exit IP card shows, read from a `cdn-cgi/trace` answer (`ip=…` and `loc=…` lines).
public struct ExitAddress: Equatable, Sendable {
    public let ip: String
    /// Two-letter country or region code, when the answer has one.
    public let region: String?

    public init(ip: String, region: String?) {
        self.ip = ip
        self.region = region
    }

    public static func parseTrace(_ text: String) -> ExitAddress? {
        var fields: [String: String] = [:]
        for line in text.split(whereSeparator: \.isNewline) {
            guard let equals = line.firstIndex(of: "=") else {
                continue
            }
            fields[String(line[..<equals])] = String(line[line.index(after: equals)...])
                .trimmingCharacters(in: .whitespaces)
        }
        guard let ip = fields["ip"], !ip.isEmpty else {
            return nil
        }
        let region = fields["loc"].flatMap { $0.isEmpty || $0 == "XX" ? nil : $0.uppercased() }
        return ExitAddress(ip: ip, region: region)
    }
}

/// What happened recently, newest first, for the events card: connects, drops, network switches and errors.
/// Kept on the device only, at most `capacity` entries.
public struct DashboardEventLog: Codable, Equatable, Sendable {
    public enum Kind: String, Codable, Sendable {
        case connected
        case disconnected
        case network
        case error
    }

    public struct Event: Codable, Equatable, Identifiable, Sendable {
        public let id: UUID
        public let date: Date
        public let kind: Kind
        public let text: String

        public init(id: UUID = UUID(), date: Date, kind: Kind, text: String) {
            self.id = id
            self.date = date
            self.kind = kind
            self.text = text
        }
    }

    public static let capacity = 20

    public private(set) var events: [Event]

    public init(events: [Event] = []) {
        self.events = Array(events.prefix(Self.capacity))
    }

    /// Adds an event at the top. The same kind and text as the newest event within a minute is dropped, so a
    /// flapping network or a repeated error shows once.
    public mutating func record(_ kind: Kind, _ text: String, at date: Date = Date()) {
        if let newest = events.first, newest.kind == kind, newest.text == text,
           abs(date.timeIntervalSince(newest.date)) < 60 {
            return
        }
        events.insert(Event(date: date, kind: kind, text: text), at: 0)
        if events.count > Self.capacity {
            events.removeLast(events.count - Self.capacity)
        }
    }

    public mutating func clear() {
        events.removeAll()
    }
}
