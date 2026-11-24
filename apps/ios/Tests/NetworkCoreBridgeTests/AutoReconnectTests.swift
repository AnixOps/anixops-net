import Foundation
import NetworkCoreBridge
import XCTest

final class AutoReconnectTests: XCTestCase {
    func testWaitsDoubleAfterTheFirstStart() {
        XCTAssertEqual(AutoReconnect.delay(beforeAttempt: 1, unit: 0.5), 0)
        XCTAssertEqual(AutoReconnect.delay(beforeAttempt: 2, unit: 0.5), 2, accuracy: 1e-9)
        XCTAssertEqual(AutoReconnect.delay(beforeAttempt: 3, unit: 0.5), 4, accuracy: 1e-9)
        XCTAssertEqual(AutoReconnect.delay(beforeAttempt: 4, unit: 0.5), 8, accuracy: 1e-9)
        XCTAssertEqual(AutoReconnect.delay(beforeAttempt: 5, unit: 0.5), 16, accuracy: 1e-9)
        // Never longer than the last step, whatever the count says.
        XCTAssertEqual(AutoReconnect.delay(beforeAttempt: 99, unit: 0.5), 16, accuracy: 1e-9)
    }

    func testJitterStaysWithinAFifth() {
        XCTAssertEqual(AutoReconnect.delay(beforeAttempt: 3, unit: 0), 3.2, accuracy: 1e-9)
        XCTAssertEqual(AutoReconnect.delay(beforeAttempt: 3, unit: 0.999_999), 4.8, accuracy: 1e-4)
        XCTAssertEqual(AutoReconnect.delay(beforeAttempt: 3, unit: -5), 3.2, accuracy: 1e-9)
        XCTAssertEqual(AutoReconnect.delay(beforeAttempt: 3, unit: 7), 4.8, accuracy: 1e-9)
    }

    /// The case from the report: every start fails the same way. The user's start and four by the system, each
    /// later than the one before, then no more.
    func testStopsAfterFiveFailuresInARow() {
        var state = AutoReconnect.State()
        XCTAssertEqual(AutoReconnect.begin(&state, user: true, unit: 0.5), .attempt(1, delay: 0))
        XCTAssertEqual(AutoReconnect.failed(&state, fatal: false), .retry)
        var waits: [TimeInterval] = []
        for attempt in 2...AutoReconnect.maxAttempts {
            guard case let .attempt(number, delay) = AutoReconnect.begin(&state, user: false, unit: 0.5) else {
                return XCTFail("start \(attempt) refused")
            }
            XCTAssertEqual(number, attempt)
            waits.append(delay)
            let outcome = AutoReconnect.failed(&state, fatal: false)
            XCTAssertEqual(outcome, attempt == AutoReconnect.maxAttempts ? .stopped(fatal: false) : .retry)
        }
        XCTAssertEqual(waits, [2, 4, 8, 16])
        XCTAssertTrue(state.stopped)
        XCTAssertEqual(AutoReconnect.begin(&state, user: false, unit: 0.5), .refuse)
        XCTAssertEqual(AutoReconnect.begin(&state, user: false, unit: 0.5), .refuse)
    }

    func testAFatalFailureStopsAtOnce() {
        var state = AutoReconnect.State()
        _ = AutoReconnect.begin(&state, user: false, unit: 0.5)
        XCTAssertEqual(AutoReconnect.failed(&state, fatal: true), .stopped(fatal: true))
        XCTAssertEqual(AutoReconnect.begin(&state, user: false, unit: 0.5), .refuse)
    }

    func testTheUsersStartAlwaysGoesAheadAndCountsAgain() {
        var state = AutoReconnect.State(attempts: 5, stopped: true)
        XCTAssertEqual(AutoReconnect.begin(&state, user: true, unit: 0.5), .attempt(1, delay: 0))
        XCTAssertEqual(state, AutoReconnect.State(attempts: 1))
    }

    /// A start that never reported back (the extension was stopped mid-start) still counts.
    func testStartsThatNeverEndedCountToo() {
        var state = AutoReconnect.State(attempts: AutoReconnect.maxAttempts)
        XCTAssertEqual(AutoReconnect.begin(&state, user: false, unit: 0.5), .refuse)
        XCTAssertTrue(state.stopped)
    }

    func testAStableConnectionStartsTheCountOver() {
        var state = AutoReconnect.State(attempts: 4)
        AutoReconnect.stable(&state)
        XCTAssertEqual(AutoReconnect.begin(&state, user: false, unit: 0.5), .attempt(1, delay: 0))
    }

    func testUserStartMark() {
        XCTAssertFalse(AutoReconnect.isUserStart(nil))
        XCTAssertFalse(AutoReconnect.isUserStart(["is-on-demand": NSNumber(value: true)]))
        let options = AutoReconnect.userStart(["other": "kept" as NSString])
        XCTAssertTrue(AutoReconnect.isUserStart(options))
        XCTAssertEqual(options["other"] as? String, "kept")
    }

    func testTheAppReadsTheStopFromTheReason() {
        let stopped = AutoReconnect.stoppedReason("embedded runtime could not start", fatal: false)
        let fatal = AutoReconnect.stoppedReason("iOS embedded runtime ABI is incompatible", fatal: true)
        XCTAssertEqual(AutoReconnect.stop(in: "隧道已停止：x\n\(stopped) (networkcore.packet-tunnel 1)"), .stopped(fatal: false))
        XCTAssertEqual(AutoReconnect.stop(in: fatal), .stopped(fatal: true))
        XCTAssertNil(AutoReconnect.stop(in: "embedded runtime could not start"))
    }

    func testStateIsKept() throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "AutoReconnectTests"))
        defaults.removePersistentDomain(forName: "AutoReconnectTests")
        XCTAssertEqual(AutoReconnect.load(from: defaults), AutoReconnect.State())
        AutoReconnect.save(AutoReconnect.State(attempts: 3, stopped: true), to: defaults)
        XCTAssertEqual(AutoReconnect.load(from: defaults), AutoReconnect.State(attempts: 3, stopped: true))
        defaults.removePersistentDomain(forName: "AutoReconnectTests")
    }
}
