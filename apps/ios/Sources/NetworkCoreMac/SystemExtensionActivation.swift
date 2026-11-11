import Foundation
import SystemExtensions

/// Activates the packet tunnel System Extension of a Developer ID build.
///
/// Outside the Mac App Store a Network Extension ships as a System Extension in
/// `Contents/Library/SystemExtensions`, and macOS loads it only after the app asks for it and the user allows it in
/// System Settings. Asking again for an extension that is already active returns at once; a newer build in the app
/// replaces the running one.
///
/// macOS keeps a request that needs the user's approval open until the user allows the extension in System Settings,
/// which can be never. `run` therefore returns `.needsApproval` as soon as macOS asks for it, and the request stays
/// open in the background: its final outcome goes to `later`.
@MainActor
final class SystemExtensionActivation: NSObject, OSSystemExtensionRequestDelegate {
    enum Outcome: Equatable {
        case activated
        /// The user has not allowed the extension in System Settings yet; the request waits for it.
        case needsApproval
        /// macOS finishes the activation at the next restart.
        case needsRestart
        /// The app is not in /Applications, where macOS requires it to be.
        case notInApplications
        case failed(code: Int)
    }

    private var continuation: CheckedContinuation<Outcome, Never>?
    private let later: (Outcome) -> Void

    /// Requests still open, kept alive here because a request holds its delegate weakly.
    private static var open: Set<SystemExtensionActivation> = []

    private init(later: @escaping (Outcome) -> Void) {
        self.later = later
    }

    /// Submits the activation request and returns its outcome, or `.needsApproval` once macOS asks the user to
    /// allow the extension. In that case the final outcome, after the user acts, goes to `later`.
    static func run(_ identifier: String, later: @escaping (Outcome) -> Void) async -> Outcome {
        let activation = SystemExtensionActivation(later: later)
        open.insert(activation)
        return await withCheckedContinuation { continuation in
            activation.continuation = continuation
            let request = OSSystemExtensionRequest.activationRequest(forExtensionWithIdentifier: identifier, queue: .main)
            request.delegate = activation
            OSSystemExtensionManager.shared.submitRequest(request)
        }
    }

    private func deliver(_ outcome: Outcome) {
        if let continuation {
            continuation.resume(returning: outcome)
            self.continuation = nil
        } else {
            later(outcome)
        }
    }

    private func finish(_ outcome: Outcome) {
        deliver(outcome)
        Self.open.remove(self)
    }

    nonisolated func request(
        _ request: OSSystemExtensionRequest,
        actionForReplacingExtension existing: OSSystemExtensionProperties,
        withExtension ext: OSSystemExtensionProperties
    ) -> OSSystemExtensionRequest.ReplacementAction {
        .replace
    }

    nonisolated func requestNeedsUserApproval(_ request: OSSystemExtensionRequest) {
        MainActor.assumeIsolated {
            deliver(.needsApproval)
        }
    }

    nonisolated func request(_ request: OSSystemExtensionRequest, didFinishWithResult result: OSSystemExtensionRequest.Result) {
        MainActor.assumeIsolated {
            finish(result == .willCompleteAfterReboot ? .needsRestart : .activated)
        }
    }

    nonisolated func request(_ request: OSSystemExtensionRequest, didFailWithError error: Error) {
        let outcome: Outcome
        if let error = error as? OSSystemExtensionError, error.code == .unsupportedParentBundleLocation {
            outcome = .notInApplications
        } else {
            outcome = .failed(code: (error as NSError).code)
        }
        MainActor.assumeIsolated {
            finish(outcome)
        }
    }
}
