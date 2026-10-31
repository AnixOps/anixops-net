import Foundation

/// How a measured delay reads at a glance: the color of a health strip block or a latency label.
public enum LatencyGrade: Equatable, Sendable {
    /// Under 120 ms.
    case good
    /// 120 ms up to 250 ms.
    case fair
    /// 250 ms or more.
    case poor
    /// Not measured, timed out or failed.
    case unknown

    public init(milliseconds: UInt32?) {
        guard let milliseconds else {
            self = .unknown
            return
        }
        if milliseconds < 120 {
            self = .good
        } else if milliseconds < 250 {
            self = .fair
        } else {
            self = .poor
        }
    }

    /// The members of a policy group with their last measured delay, in the group's order, for its health strip.
    /// Members the engine has no delay for (`DIRECT`, nested groups, untested nodes) read as unknown.
    public static func strip(
        members: [String],
        latencies: [RuntimeControl.Groups.Group.Member]
    ) -> [LatencyGrade] {
        let byName = Dictionary(latencies.map { ($0.name, $0.latencyMs) }, uniquingKeysWith: { first, _ in first })
        return members.map { LatencyGrade(milliseconds: byName[$0] ?? nil) }
    }

    /// How many blocks of a strip have a measured delay.
    public static func reachable(_ strip: [LatencyGrade]) -> Int {
        strip.filter { $0 != .unknown }.count
    }
}
