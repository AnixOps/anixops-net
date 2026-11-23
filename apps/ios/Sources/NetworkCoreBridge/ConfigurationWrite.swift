import Foundation

/// One read-modify-write of a saved configuration that other processes write too: the VPN configuration, which the
/// widget, Control Center, the Live Activity, Shortcuts and Settings change besides the app.
///
/// The configuration is read right before the change, so the change applies to what is saved now rather than to a
/// copy read earlier, and `change` sets only the fields its write owns: an earlier copy would put back the On Demand
/// switch or the mode another process changed since. When the save is refused because another process saved in
/// between (`isStale`), the configuration is read once more and `change` replays on that copy; nothing of the
/// refused copy carries over.
public enum ConfigurationWrite {
    public enum Outcome<Object> {
        /// There is no saved configuration, and the write may not add one.
        case missing
        /// The configuration already had what the write sets; nothing was saved.
        case unchanged(Object)
        /// The changed configuration was saved.
        case saved(Object)

        /// The configuration as it is saved after the write; nil when there is none.
        public var object: Object? {
            switch self {
            case .missing:
                return nil
            case let .unchanged(object), let .saved(object):
                return object
            }
        }

        public var isSaved: Bool {
            if case .saved = self {
                return true
            }
            return false
        }
    }

    /// Runs the write.
    ///
    /// - Parameters:
    ///   - load: Reads the saved configuration; nil when there is none.
    ///   - create: Makes a new configuration when there is none. Nil leaves a missing configuration missing: only an
    ///     import or a connect adds one, never a change of a setting.
    ///   - save: Saves a changed copy.
    ///   - isStale: Whether a failed save was refused because another process saved after the copy was read.
    ///   - change: Sets the fields this write owns on a fresh copy; false when the copy already has them.
    @MainActor
    public static func run<Object>(
        load: () async throws -> Object?,
        create: (() -> Object)? = nil,
        save: (Object) async throws -> Void,
        isStale: (Error) -> Bool,
        change: (Object) throws -> Bool
    ) async throws -> Outcome<Object> {
        var replayed = false
        while true {
            guard let object = try await load() ?? create?() else {
                return .missing
            }
            guard try change(object) else {
                return .unchanged(object)
            }
            do {
                try await save(object)
                return .saved(object)
            } catch let error where !replayed && isStale(error) {
                replayed = true
            }
        }
    }
}
