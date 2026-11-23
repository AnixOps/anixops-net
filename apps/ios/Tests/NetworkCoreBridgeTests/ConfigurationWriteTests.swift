import NetworkCoreBridge
import XCTest

/// A saved configuration and the copies read from it, the way the system keeps the VPN configuration: a copy
/// remembers the revision it was read at, and a save from a copy read before another save is refused as stale.
@MainActor
private final class FakePreferences {
    struct Fields: Equatable {
        var onDemand = false
        var mode = "rule"
        var dns = "old"
    }

    final class Copy {
        var fields: Fields
        let revision: Int

        init(fields: Fields, revision: Int) {
            self.fields = fields
            self.revision = revision
        }
    }

    struct Stale: Error {}

    private(set) var saved: Fields?
    /// What other processes save right after a copy is read and before the write saves it, one per read: the widget,
    /// Control Center, Shortcuts or Settings changing the configuration meanwhile.
    var otherSaves: [(inout Fields) -> Void] = []
    /// Refuses every save with this error.
    var refusal: Error?
    private(set) var revision = 0
    private(set) var reads = 0
    private(set) var saves = 0

    init(saved: Fields?) {
        self.saved = saved
    }

    func write(
        creating: Bool = false,
        _ change: (Copy) throws -> Bool
    ) async throws -> ConfigurationWrite.Outcome<Copy> {
        let create: (() -> Copy)? = creating ? { Copy(fields: Fields(), revision: self.revision) } : nil
        return try await ConfigurationWrite.run(
            load: { self.read() },
            create: create,
            save: { try self.save($0) },
            isStale: { $0 is Stale },
            change: change
        )
    }

    private func read() -> Copy? {
        reads += 1
        guard let fields = saved else {
            return nil
        }
        let copy = Copy(fields: fields, revision: revision)
        if !otherSaves.isEmpty {
            var changed = fields
            otherSaves.removeFirst()(&changed)
            saved = changed
            revision += 1
        }
        return copy
    }

    private func save(_ copy: Copy) throws {
        if let refusal {
            throw refusal
        }
        guard copy.revision == revision else {
            throw Stale()
        }
        saved = copy.fields
        revision += 1
        saves += 1
    }
}

private typealias Fields = FakePreferences.Fields

private struct Refused: Error {}

@MainActor
final class ConfigurationWriteTests: XCTestCase {
    func testChangeReplaysOnTheConfigurationAnotherProcessSaved() async throws {
        // The app read the configuration; then Control Center turned On Demand on and Shortcuts picked global.
        let preferences = FakePreferences(saved: Fields())
        preferences.otherSaves = [{ $0.onDemand = true; $0.mode = "global" }]
        var changes = 0
        let outcome = try await preferences.write { copy in
            changes += 1
            copy.fields.dns = "new"
            return true
        }
        XCTAssertTrue(outcome.isSaved)
        XCTAssertEqual(changes, 2)
        // Only what the write owns changed; nothing of the refused copy came back.
        XCTAssertEqual(preferences.saved, Fields(onDemand: true, mode: "global", dns: "new"))
        XCTAssertEqual(outcome.object?.fields, preferences.saved)
        XCTAssertEqual(preferences.saves, 1)
    }

    func testGivesUpWhenTheReplayIsStaleToo() async {
        let preferences = FakePreferences(saved: Fields())
        preferences.otherSaves = [{ $0.mode = "global" }, { $0.mode = "direct" }]
        do {
            _ = try await preferences.write { copy in
                copy.fields.dns = "new"
                return true
            }
            XCTFail("a second stale save should fail")
        } catch {
            XCTAssertTrue(error is FakePreferences.Stale)
        }
        XCTAssertEqual(preferences.reads, 2)
        XCTAssertEqual(preferences.saved, Fields(onDemand: false, mode: "direct", dns: "old"))
    }

    func testOtherFailuresAreNotReplayed() async {
        let preferences = FakePreferences(saved: Fields())
        preferences.refusal = Refused()
        do {
            _ = try await preferences.write { copy in
                copy.fields.dns = "new"
                return true
            }
            XCTFail("the refusal should pass on")
        } catch {
            XCTAssertTrue(error is Refused)
        }
        XCTAssertEqual(preferences.reads, 1)
        XCTAssertEqual(preferences.saved, Fields())
    }

    func testMissingConfigurationIsAddedOnlyByAWriteThatMay() async throws {
        // Changing a setting never adds a VPN configuration; only an import or a connect does.
        let preferences = FakePreferences(saved: nil)
        var changed = false
        let missing = try await preferences.write { _ in
            changed = true
            return true
        }
        XCTAssertNil(missing.object)
        XCTAssertFalse(missing.isSaved)
        XCTAssertFalse(changed)
        XCTAssertNil(preferences.saved)

        let created = try await preferences.write(creating: true) { copy in
            copy.fields.dns = "new"
            return true
        }
        XCTAssertTrue(created.isSaved)
        XCTAssertEqual(preferences.saved?.dns, "new")
    }

    func testConfigurationThatHasTheChangeIsNotSaved() async throws {
        let preferences = FakePreferences(saved: Fields(dns: "new"))
        let outcome = try await preferences.write { copy in
            guard copy.fields.dns != "new" else {
                return false
            }
            copy.fields.dns = "new"
            return true
        }
        XCTAssertFalse(outcome.isSaved)
        XCTAssertEqual(outcome.object?.fields.dns, "new")
        XCTAssertEqual(preferences.saves, 0)
    }
}
