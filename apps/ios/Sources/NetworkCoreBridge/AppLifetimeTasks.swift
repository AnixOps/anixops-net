import Foundation

/// Long-running work that belongs to the app rather than to a window: the macOS app keeps running in the menu bar
/// after its main window is closed, and SwiftUI cancels a window's `.task` when the window goes. Each piece runs once
/// under its name, in a task of its own, so ending the caller (that `.task`) does not end the work, and a window
/// opened again does not start a second copy.
@MainActor
public final class AppLifetimeTasks {
    private var tasks: [String: Task<Void, Never>] = [:]

    public init() {}

    /// Starts `work` under `name` unless it was started already. Returns whether it started now.
    @discardableResult
    public func start(_ name: String, _ work: @escaping @MainActor () async -> Void) -> Bool {
        guard tasks[name] == nil else {
            return false
        }
        // Unstructured: not a child of the caller's task, so its cancellation does not reach the work.
        tasks[name] = Task { @MainActor in
            await work()
        }
        return true
    }

    public func isStarted(_ name: String) -> Bool {
        tasks[name] != nil
    }

    /// Cancels everything, when the app quits.
    public func cancelAll() {
        for task in tasks.values {
            task.cancel()
        }
        tasks.removeAll()
    }
}
