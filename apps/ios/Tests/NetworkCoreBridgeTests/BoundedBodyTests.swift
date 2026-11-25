@testable import NetworkCoreBridge
import XCTest

final class BoundedBodyTests: XCTestCase {
    /// Bytes from an array, counting how many were taken.
    private final class Counter: @unchecked Sendable {
        var taken = 0
    }

    private struct Bytes: AsyncSequence {
        typealias Element = UInt8
        let count: Int?
        let counter: Counter

        struct AsyncIterator: AsyncIteratorProtocol {
            var left: Int?
            let counter: Counter
            mutating func next() async -> UInt8? {
                if let remaining = left {
                    guard remaining > 0 else { return nil }
                    left = remaining - 1
                }
                counter.taken += 1
                return 0x61
            }
        }

        func makeAsyncIterator() -> AsyncIterator {
            AsyncIterator(left: count, counter: counter)
        }
    }

    func testReadsUpToTheLimitExactly() async throws {
        let counter = Counter()
        let data = try await BoundedBody.collect(Bytes(count: 1000, counter: counter), expectedLength: -1, limit: 1000)
        XCTAssertEqual(data.count, 1000)
        let declared = try await BoundedBody.collect(Bytes(count: 1000, counter: counter), expectedLength: 1000, limit: 1000)
        XCTAssertEqual(declared.count, 1000)
    }

    func testRefusesOneByteOverWithoutALength() async {
        do {
            _ = try await BoundedBody.collect(Bytes(count: 1001, counter: Counter()), expectedLength: -1, limit: 1000)
            XCTFail("an answer over the limit was read")
        } catch {
            XCTAssertEqual(error as? BoundedBody.TooLarge, BoundedBody.TooLarge(limit: 1000))
        }
    }

    func testRefusesADeclaredLengthOverTheLimitBeforeReading() async {
        let counter = Counter()
        do {
            _ = try await BoundedBody.collect(Bytes(count: nil, counter: counter), expectedLength: 1001, limit: 1000)
            XCTFail("an answer declared over the limit was read")
        } catch {
            XCTAssertTrue(error is BoundedBody.TooLarge)
        }
        XCTAssertEqual(counter.taken, 0)
    }

    func testStopsAnEndlessAnswerAtTheLimit() async {
        // No length, or one that lies: reading stops right after the limit.
        for declared: Int64 in [-1, 10] {
            let counter = Counter()
            do {
                _ = try await BoundedBody.collect(Bytes(count: nil, counter: counter), expectedLength: declared, limit: 1000)
                XCTFail("an endless answer was read")
            } catch {
                XCTAssertTrue(error is BoundedBody.TooLarge)
            }
            XCTAssertLessThanOrEqual(counter.taken, 1001)
        }
    }
}
