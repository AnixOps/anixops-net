import Foundation

/// How the tunnel starts again by itself after a failed start, and when it stops trying.
///
/// With reconnecting on (`OnDemandSettings`), the system starts the tunnel again the moment a start fails, so a start
/// that cannot work (an incompatible engine, a node that refuses the client) used to loop several times a second.
/// The extension now counts the starts since the last one the user asked for, or since the tunnel last stayed up for
/// `stableAfter`: the first goes ahead at once, each later one waits twice as long as the one before (2, 4, 8, 16 s,
/// give or take a fifth so many devices do not retry in step), and after `maxAttempts` failed starts in a row it stops
/// reconnecting and says so. A failure no retry can fix (`fatal`) stops it at once. The desktop and Android clients
/// follow the same limit (`on_demand::retry`, `MAX_RESTARTS`), see docs/auto-reconnect.md.
public enum AutoReconnect {
    /// Failed starts in a row, counting the user's own, before the tunnel stops reconnecting by itself.
    public static let maxAttempts = 5
    /// The wait before the second start; it doubles for each start after that.
    public static let baseDelay: TimeInterval = 2
    /// How far a wait may stray from its nominal value, as a fraction of it.
    public static let jitter = 0.2
    /// A tunnel up this long counts as working again: the next failure counts from one.
    public static let stableAfter: TimeInterval = 60
    /// How long a start the extension refuses is held before it fails, when the extension could not turn reconnecting
    /// off itself: the system would otherwise start it again at once, so this is how often such a loop can turn.
    public static let refusalHold: TimeInterval = 30

    /// The `startVPNTunnel(options:)` key of a start the user asked for (the app, its widgets, shortcuts and
    /// Control Center switch); the system's own starts (On Demand, after a crash or the memory limit) carry none.
    public static let userStartKey = "net.anixops.user-start"

    /// `options` with the mark of a start the user asked for.
    public static func userStart(_ options: [String: NSObject]? = nil) -> [String: NSObject] {
        var marked = options ?? [:]
        marked[userStartKey] = NSNumber(value: true)
        return marked
    }

    /// Whether a start's options carry the user's mark.
    public static func isUserStart(_ options: [String: NSObject]?) -> Bool {
        (options?[userStartKey] as? NSNumber)?.boolValue == true
    }

    /// The wait before start number `attempt` (from 1): none for the first, then `baseDelay` doubling. `unit` is a
    /// random number in 0..<1 that spreads the wait by `jitter` either way.
    public static func delay(beforeAttempt attempt: Int, unit: Double) -> TimeInterval {
        guard attempt > 1 else {
            return 0
        }
        let nominal = baseDelay * pow(2, Double(min(attempt, maxAttempts) - 2))
        let spread = 1 + jitter * (2 * min(max(unit, 0), 1) - 1)
        return nominal * spread
    }

    /// What the extension keeps between starts.
    public struct State: Codable, Equatable, Sendable {
        /// Starts since the user's last start or the last stable connection.
        public var attempts: Int
        /// Set once it stopped reconnecting; only a start the user asks for clears it.
        public var stopped: Bool

        public init(attempts: Int = 0, stopped: Bool = false) {
            self.attempts = attempts
            self.stopped = stopped
        }
    }

    /// What a start does.
    public enum Decision: Equatable, Sendable {
        /// Go ahead as start number `attempt`, after waiting `delay` seconds.
        case attempt(Int, delay: TimeInterval)
        /// Do not start: reconnecting stopped. Only a start the user asks for goes ahead.
        case refuse
    }

    /// Decides a start and counts it. The user's start always goes ahead at once and counts from one again.
    public static func begin(_ state: inout State, user: Bool, unit: Double) -> Decision {
        if user {
            state = State(attempts: 1)
            return .attempt(1, delay: 0)
        }
        if state.stopped {
            return .refuse
        }
        let attempt = state.attempts + 1
        guard attempt <= maxAttempts else {
            // The earlier start ended without saying how (the extension was stopped mid-start): it counts as a
            // failure all the same.
            state.stopped = true
            return .refuse
        }
        state.attempts = attempt
        return .attempt(attempt, delay: delay(beforeAttempt: attempt, unit: unit))
    }

    /// What follows a failed start.
    public enum Outcome: Equatable, Sendable {
        /// The system may start it again; the next start waits longer.
        case retry
        /// Reconnecting stopped: `maxAttempts` failures in a row, or a `fatal` one.
        case stopped(fatal: Bool)
    }

    /// Counts a failed start.
    public static func failed(_ state: inout State, fatal: Bool) -> Outcome {
        if fatal || state.attempts >= maxAttempts {
            state.stopped = true
            return .stopped(fatal: fatal)
        }
        return .retry
    }

    /// The tunnel stayed up `stableAfter`: the count starts over.
    public static func stable(_ state: inout State) {
        state = State()
    }

    /// The end the extension adds to the failure reason when it stops reconnecting, so the app can tell from the
    /// system's record of the stop (`fetchLastDisconnectError`) that nothing will start the tunnel again.
    public static let stoppedMark = "[auto-reconnect stopped]"
    public static let fatalMark = "[auto-reconnect stopped: retrying cannot fix this]"

    public static func stoppedReason(_ reason: String, fatal: Bool) -> String {
        "\(reason) \(fatal ? fatalMark : stoppedMark)"
    }

    /// How the stop recorded in `text` ended reconnecting: nil when it did not.
    public static func stop(in text: String) -> Outcome? {
        if text.contains(fatalMark) {
            return .stopped(fatal: true)
        }
        if text.contains(stoppedMark) {
            return .stopped(fatal: false)
        }
        return nil
    }

    /// Where the extension keeps `State`: its own defaults, so the System Extension on macOS (a root process with no
    /// App Group) keeps it too.
    public static let storageKey = "net.anixops.auto-reconnect"

    public static func load(from defaults: UserDefaults = .standard) -> State {
        guard let data = defaults.data(forKey: storageKey),
              let state = try? JSONDecoder().decode(State.self, from: data)
        else {
            return State()
        }
        return state
    }

    public static func save(_ state: State, to defaults: UserDefaults = .standard) {
        if let data = try? JSONEncoder().encode(state) {
            defaults.set(data, forKey: storageKey)
        }
    }
}
