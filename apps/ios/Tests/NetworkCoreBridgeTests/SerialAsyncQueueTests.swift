import NetworkCoreBridge
import XCTest

/// Holds operations at one point until the test lets them go on.
@MainActor
private final class Gate {
    private var isOpen = false
    private var waiters: [CheckedContinuation<Void, Never>] = []

    func wait() async {
        guard !isOpen else {
            return
        }
        await withCheckedContinuation { waiters.append($0) }
    }

    func open() {
        isOpen = true
        let resumed = waiters
        waiters.removeAll()
        for waiter in resumed {
            waiter.resume()
        }
    }
}

/// What the operations did, in order.
@MainActor
private final class Journal {
    var entries: [String] = []
}

@MainActor
final class SerialAsyncQueueTests: XCTestCase {
    /// Lets the tasks started so far run until `condition` holds, for a bounded number of turns.
    private func settle(until condition: () -> Bool) async {
        var turns = 0
        while !condition(), turns < 1_000 {
            turns += 1
            await Task.yield()
        }
    }

    func testOperationsRunOneAtATimeInTheOrderSubmitted() async {
        // Two node switches used to interleave at their awaits; the second must not start before the first ends.
        let queue = SerialAsyncQueue()
        let journal = Journal()
        let gate = Gate()
        var tasks: [Task<Void, Never>] = []
        for index in 1...3 {
            tasks.append(Task {
                await queue.run {
                    journal.entries.append("start \(index)")
                    await gate.wait()
                    journal.entries.append("end \(index)")
                }
            })
            await settle { queue.pending == index }
        }
        XCTAssertEqual(queue.pending, 3)
        XCTAssertEqual(journal.entries, ["start 1"])
        gate.open()
        for task in tasks {
            _ = await task.value
        }
        XCTAssertEqual(journal.entries, ["start 1", "end 1", "start 2", "end 2", "start 3", "end 3"])
        XCTAssertEqual(queue.pending, 0)
    }

    func testPassesResultsAndErrorsOnAndGoesOnAfterAFailure() async {
        let queue = SerialAsyncQueue()
        let value = await queue.run { 42 }
        XCTAssertEqual(value, 42)
        do {
            try await queue.run { () async throws -> Void in
                throw CancellationError()
            }
            XCTFail("the operation's error should pass on")
        } catch {
            XCTAssertTrue(error is CancellationError)
        }
        let next = await queue.run { "next" }
        XCTAssertEqual(next, "next")
        XCTAssertEqual(queue.pending, 0)
    }

    func testRepeatedTapWaitsForTheSaveItRepeats() async {
        let queue = SerialAsyncQueue()
        let journal = Journal()
        let gate = Gate()
        let first = Task {
            await queue.run(unlessRepeating: "node-a") {
                journal.entries.append("save a")
                await gate.wait()
                journal.entries.append("saved a")
            }
        }
        await settle { queue.pending == 1 }
        let repeated = Task { () async -> Bool in
            let ran = await queue.run(unlessRepeating: "node-a") {
                journal.entries.append("save a again")
            }
            journal.entries.append("repeat returned")
            return ran
        }
        await settle { queue.pending == 2 }
        XCTAssertEqual(journal.entries, ["save a"])
        gate.open()
        let firstRan = await first.value
        let repeatRan = await repeated.value
        XCTAssertTrue(firstRan)
        XCTAssertFalse(repeatRan)
        // It returned only once the save it repeats was done, and did not save again.
        XCTAssertEqual(journal.entries, ["save a", "saved a", "repeat returned"])
        XCTAssertEqual(queue.pending, 0)

        // Once that save is done, the same key runs again.
        let later = await queue.run(unlessRepeating: "node-a") {
            journal.entries.append("save a later")
        }
        XCTAssertTrue(later)
        XCTAssertEqual(journal.entries.last, "save a later")
    }

    func testLastTapWins() async {
        // A tap on another node in between counts, so tapping a again switches back to a.
        let outcome = await tap(["node-a", "node-b", "node-a"])
        XCTAssertEqual(outcome.ran, [true, true, true])
        XCTAssertEqual(outcome.order, ["node-a", "node-b", "node-a"])
    }

    func testRepeatedTapOnAWaitingSaveSavesOnce() async {
        let outcome = await tap(["node-a", "node-b", "node-b"])
        XCTAssertEqual(outcome.ran, [true, true, false])
        XCTAssertEqual(outcome.order, ["node-a", "node-b"])
    }

    /// Submits one keyed operation per key while the first holds the queue, then lets them all finish: whether each
    /// ran, and the keys of the operations that did, in the order they ran.
    private func tap(_ keys: [String]) async -> (ran: [Bool], order: [String]) {
        let queue = SerialAsyncQueue()
        let journal = Journal()
        let gate = Gate()
        var tasks: [Task<Bool, Never>] = []
        for (index, key) in keys.enumerated() {
            tasks.append(Task {
                await queue.run(unlessRepeating: key) {
                    journal.entries.append(key)
                    await gate.wait()
                }
            })
            await settle { queue.pending == index + 1 }
        }
        gate.open()
        var ran: [Bool] = []
        for task in tasks {
            let value = await task.value
            ran.append(value)
        }
        return (ran, journal.entries)
    }
}
