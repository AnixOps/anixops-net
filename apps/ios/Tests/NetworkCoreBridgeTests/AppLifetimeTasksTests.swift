@testable import NetworkCoreBridge
import XCTest

@MainActor
final class AppLifetimeTasksTests: XCTestCase {
    /// The work outlives the window `.task` that started it: cancelling the caller does not cancel the work.
    func testWorkOutlivesTheCancelledCaller() async {
        let tasks = AppLifetimeTasks()
        let ticked = expectation(description: "the loop still runs after its caller was cancelled")
        ticked.assertForOverFulfill = false
        let caller = Task { @MainActor in
            tasks.start("loop") {
                var count = 0
                while !Task.isCancelled {
                    count += 1
                    if count == 3 {
                        ticked.fulfill()
                    }
                    try? await Task.sleep(for: .milliseconds(20))
                }
            }
        }
        _ = await caller.value
        caller.cancel()
        await fulfillment(of: [ticked], timeout: 5)
        XCTAssertTrue(tasks.isStarted("loop"))
        tasks.cancelAll()
        XCTAssertFalse(tasks.isStarted("loop"))
    }

    /// A window opened again does not start a second copy.
    func testEachNameStartsOnce() async {
        let tasks = AppLifetimeTasks()
        var runs = 0
        XCTAssertTrue(tasks.start("work") { runs += 1 })
        XCTAssertFalse(tasks.start("work") { runs += 1 })
        XCTAssertTrue(tasks.start("other") {})
        try? await Task.sleep(for: .milliseconds(100))
        XCTAssertEqual(runs, 1)
        tasks.cancelAll()
    }
}
