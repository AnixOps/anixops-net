import Foundation

/// Traffic over the last seven days in hourly buckets, by host and by policy, for the statistics page. The tunnel
/// extension builds it from the engine's connection list and keeps it in its own container; the app reads it with
/// `TunnelAppMessage.trafficStats`. It holds host names and byte counts only, never addresses or paths.
public struct TrafficStats: Codable, Equatable, Sendable {
    public struct Hour: Codable, Equatable, Sendable {
        /// The start of the hour.
        public let start: Date
        public var upload: UInt64 = 0
        public var download: UInt64 = 0
        public var hosts: [String: UInt64] = [:]
        public var policies: [String: UInt64] = [:]

        public init(start: Date) {
            self.start = start
        }

        public var total: UInt64 {
            upload &+ download
        }
    }

    /// One row of the by-host or by-policy list.
    public struct Share: Equatable, Identifiable, Sendable {
        public let name: String
        public let bytes: UInt64

        public var id: String {
            name
        }

        public init(name: String, bytes: UInt64) {
            self.name = name
            self.bytes = bytes
        }
    }

    public static let keptHours = 7 * 24
    /// Hosts kept per hour; the rest add up under `otherHosts`.
    public static let hostsPerHour = 64
    public static let otherHosts = "*"

    /// Oldest first.
    public private(set) var hours: [Hour] = []

    public init(hours: [Hour] = []) {
        self.hours = hours
    }

    public static func hourStart(of date: Date, calendar: Calendar = .current) -> Date {
        calendar.dateInterval(of: .hour, for: date)?.start ?? date
    }

    /// Adds bytes one connection moved since it was last counted.
    public mutating func add(host: String, policy: String, upload: UInt64, download: UInt64, at date: Date) {
        let bytes = upload &+ download
        guard bytes > 0 else {
            return
        }
        let start = Self.hourStart(of: date)
        if !hours.contains(where: { $0.start == start }) {
            hours.append(Hour(start: start))
            hours.sort { $0.start < $1.start }
            prune(now: date)
        }
        guard let index = hours.lastIndex(where: { $0.start == start }) else {
            return
        }
        var hour = hours[index]
        hour.upload &+= upload
        hour.download &+= download
        let key = hour.hosts[host] != nil || hour.hosts.count < Self.hostsPerHour ? host : Self.otherHosts
        hour.hosts[key, default: 0] &+= bytes
        hour.policies[policy, default: 0] &+= bytes
        hours[index] = hour
    }

    /// Drops hours older than seven days before `now`.
    public mutating func prune(now: Date) {
        let oldest = Self.hourStart(of: now).addingTimeInterval(-Double(Self.keptHours - 1) * 3600)
        hours.removeAll { $0.start < oldest }
    }

    public mutating func clear() {
        hours.removeAll()
    }

    /// The hours that start at or after `since`.
    public func hours(since: Date) -> [Hour] {
        hours.filter { $0.start >= since }
    }

    public static func total(_ hours: [Hour]) -> UInt64 {
        hours.reduce(0) { $0 &+ $1.total }
    }

    public static func hosts(_ hours: [Hour], limit: Int = 50) -> [Share] {
        shares(hours.map(\.hosts), limit: limit)
    }

    public static func policies(_ hours: [Hour], limit: Int = 50) -> [Share] {
        shares(hours.map(\.policies), limit: limit)
    }

    private static func shares(_ maps: [[String: UInt64]], limit: Int) -> [Share] {
        var sums: [String: UInt64] = [:]
        for map in maps {
            for (name, bytes) in map {
                sums[name, default: 0] &+= bytes
            }
        }
        let rows: [Share] = sums.map { name, bytes in
            Share(name: name, bytes: bytes)
        }
        let sorted = rows.sorted(by: Self.isLarger)
        return Array(sorted.prefix(max(0, limit)))
    }

    /// More bytes first; equal counts by name.
    private static func isLarger(_ lhs: Share, _ rhs: Share) -> Bool {
        if lhs.bytes != rhs.bytes {
            return lhs.bytes > rhs.bytes
        }
        return lhs.name < rhs.name
    }

    public func encoded() throws -> Data {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .secondsSince1970
        return try encoder.encode(self)
    }

    public static func decoded(from data: Data) throws -> TrafficStats {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .secondsSince1970
        return try decoder.decode(TrafficStats.self, from: data)
    }
}

/// Turns successive connection lists into byte deltas: each connection counts only what it moved since the last
/// list, so polling never counts a byte twice. A connection that both opened and closed between two polls and has
/// left the closed list by then is missed; polling every few seconds keeps that rare.
public struct TrafficStatsCounter: Sendable {
    private struct Seen: Sendable {
        var upload: UInt64
        var download: UInt64
    }

    private var seen: [UInt64: Seen] = [:]

    public init() {}

    public mutating func ingest(_ list: RuntimeControl.Connections, into stats: inout TrafficStats, at date: Date = Date()) {
        var next: [UInt64: Seen] = [:]
        for connection in list.active + list.closed {
            let before = seen[connection.id]
            // A smaller count than before means the engine restarted and reused the id: count it from zero.
            let upload = Self.delta(connection.upload, since: before?.upload)
            let download = Self.delta(connection.download, since: before?.download)
            stats.add(
                host: connection.hostName,
                policy: connection.chain.first ?? connection.outbound,
                upload: upload,
                download: download,
                at: date
            )
            next[connection.id] = Seen(upload: connection.upload, download: connection.download)
        }
        seen = next
    }

    private static func delta(_ now: UInt64, since before: UInt64?) -> UInt64 {
        guard let before, now >= before else {
            return now
        }
        return now - before
    }

    /// Forgets the connections seen so far, for a new engine.
    public mutating func reset() {
        seen.removeAll()
    }
}
