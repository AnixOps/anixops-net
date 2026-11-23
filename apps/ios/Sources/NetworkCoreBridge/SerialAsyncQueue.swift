import Foundation

/// Runs async operations one at a time, in the order they were submitted.
///
/// Main-actor code is entered again at every `await`. Two taps that each saved the VPN configuration used to
/// interleave there: both wrote a node key, both waited for the system to save, and each then removed the other's
/// key, so the profile that stayed in the system settings named a key that was gone. An operation run here keeps
/// every later one waiting until it returns, its suspensions included.
///
/// An operation must not submit to its own queue and wait for that: it would wait for itself.
@MainActor
public final class SerialAsyncQueue {
    /// Set while an operation runs; handed on to the next waiting one without becoming free in between.
    private var busy = false
    private var waiting: [CheckedContinuation<Void, Never>] = []
    /// The keyed operation submitted last, until it finishes (`run(unlessRepeating:_:)`).
    private var latest: (key: String, id: Int)?
    /// Calls that repeated a keyed operation and wait for it, by that operation's id.
    private var repeats: [Int: [CheckedContinuation<Void, Never>]] = [:]
    private var lastID = 0

    /// Calls that have not returned yet: the operation running, those waiting for their turn and the repeats waiting
    /// for the operation they repeat.
    public private(set) var pending = 0

    nonisolated public init() {}

    /// Runs `operation` once every operation submitted before it has finished, and returns its result.
    public func run<T>(_ operation: () async throws -> T) async rethrows -> T {
        pending += 1
        if busy {
            await withCheckedContinuation { waiting.append($0) }
        } else {
            busy = true
        }
        defer {
            pending -= 1
            if waiting.isEmpty {
                busy = false
            } else {
                waiting.removeFirst().resume()
            }
        }
        return try await operation()
    }

    /// Runs `operation` as `run(_:)` does, unless the keyed operation submitted last has the same `key` and has not
    /// finished: this call then waits for that one and returns false instead of running `operation` again. A second
    /// tap on the node being saved waits for that save; a tap on another node in between counts, so the last tap
    /// always wins.
    @discardableResult
    public func run(unlessRepeating key: String, _ operation: () async -> Void) async -> Bool {
        if let latest, latest.key == key {
            pending += 1
            await withCheckedContinuation { repeats[latest.id, default: []].append($0) }
            pending -= 1
            return false
        }
        lastID += 1
        let id = lastID
        latest = (key, id)
        defer {
            if latest?.id == id {
                latest = nil
            }
            for waiter in repeats.removeValue(forKey: id) ?? [] {
                waiter.resume()
            }
        }
        await run(operation)
        return true
    }
}
