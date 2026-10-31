import Foundation

extension RuntimeControl.Connection {
    /// Where a connection went, as the request list colors it.
    public enum Route: Equatable, Sendable {
        case proxy
        case direct
        case reject
    }

    /// The state a row's status bar shows.
    public enum Status: Equatable, Sendable {
        case active
        case done
        case failed
        case rejected
    }

    public var route: Route {
        if outbound == "REJECT" || error == "rejected" {
            return .reject
        }
        return outbound == "DIRECT" ? .direct : .proxy
    }

    public var status: Status {
        if route == .reject {
            return .rejected
        }
        if error != nil {
            return .failed
        }
        return isActive ? .active : .done
    }

    /// The host name, or the destination address without its port when the engine saw no name.
    public var hostName: String {
        HostTraffic.hostName(of: self)
    }

    /// A short protocol badge. The engine reports only TCP or UDP, so well-known ports name the usual protocol
    /// on top: HTTPS and HTTP on TCP 443 and 80, QUIC on UDP 443, DNS on 53.
    public var protocolLabel: String {
        let port = destination.lastIndex(of: ":").flatMap { Int(destination[destination.index(after: $0)...]) }
        switch (network.lowercased(), port) {
        case ("tcp", 443):
            return "HTTPS"
        case ("tcp", 80):
            return "HTTP"
        case ("udp", 443):
            return "QUIC"
        case (_, 53):
            return "DNS"
        default:
            return network.uppercased()
        }
    }

    /// The first policy group and the node, as `Proxy · hk-1`; the node alone without groups.
    public var policyLabel: String {
        guard let group = chain.first, group != outbound else {
            return outbound
        }
        return "\(group) · \(outbound)"
    }

    /// How long the connection has been open, or was open.
    public func duration(now: Date = Date()) -> TimeInterval {
        max(0, (endedAt ?? now).timeIntervalSince(startedAt))
    }
}

/// The filter chips above the request list.
public enum RequestFilter: String, CaseIterable, Identifiable, Sendable {
    case all
    case proxy
    case direct
    case reject
    case failed
    case udp

    public var id: String {
        rawValue
    }

    public func matches(_ connection: RuntimeControl.Connection) -> Bool {
        switch self {
        case .all:
            return true
        case .proxy:
            return connection.route == .proxy
        case .direct:
            return connection.route == .direct
        case .reject:
            return connection.route == .reject
        case .failed:
            return connection.status == .failed
        case .udp:
            return connection.network.lowercased() == "udp"
        }
    }
}

extension HostTraffic {
    /// Every host in `connections` with its bytes and connection count, largest first; hosts that moved nothing
    /// are kept, after the others.
    public static func summarize(_ connections: [RuntimeControl.Connection]) -> [HostTraffic] {
        struct Sum {
            var bytes: UInt64 = 0
            var connections = 0
            var byOutbound: [String: UInt64] = [:]
        }
        var sums: [String: Sum] = [:]
        for connection in connections {
            let bytes = connection.upload &+ connection.download
            var sum = sums[connection.hostName, default: Sum()]
            sum.bytes &+= bytes
            sum.connections += 1
            sum.byOutbound[connection.outbound, default: 0] &+= bytes
            sums[connection.hostName] = sum
        }
        return sums.map { host, sum in
            let outbound = sum.byOutbound.max { lhs, rhs in
                lhs.value == rhs.value ? lhs.key > rhs.key : lhs.value < rhs.value
            }?.key ?? ""
            return HostTraffic(host: host, bytes: sum.bytes, connections: sum.connections, outbound: outbound)
        }
        .sorted { $0.bytes == $1.bytes ? $0.host < $1.host : $0.bytes > $1.bytes }
    }
}

/// The badge that stands in for a site's icon: a letter and a color picked from the host name on the device,
/// so no icon is fetched from the network.
public enum HostMonogram {
    private static let skippedLabels: Set<String> = ["www", "m", "api", "cdn", "img", "static", "edge"]

    public static func letter(for host: String) -> String {
        let labels = host.split(separator: ".").map(String.init)
        let isAddress = !labels.isEmpty && labels.allSatisfy { Int($0) != nil } || host.contains(":")
        if isAddress {
            return "#"
        }
        let name = labels.first { !skippedLabels.contains($0.lowercased()) && labels.count > 1 } ?? labels.first ?? ""
        return name.first.map { String($0).uppercased() } ?? "#"
    }

    /// A stable index into a palette of `count` colors.
    public static func colorIndex(for host: String, count: Int) -> Int {
        guard count > 0 else {
            return 0
        }
        var hash: UInt32 = 2_166_136_261
        for byte in host.lowercased().utf8 {
            hash = (hash ^ UInt32(byte)) &* 16_777_619
        }
        return Int(hash % UInt32(count))
    }
}
