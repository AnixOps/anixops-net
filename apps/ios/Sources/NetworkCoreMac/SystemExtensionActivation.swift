import Foundation
import SystemExtensions

/// Activates the packet tunnel System Extension of a Developer ID build.
///
/// Outside the Mac App Store a Network Extension ships as a System Extension in
/// `Contents/Library/SystemExtensions`, and macOS loads it only after the app asks for it and the user allows it in
/// System Settings. Asking again for an extension that is already active returns at once; a newer build in the app
/// replaces the running one.
final class SystemExtensionActivation: NSObject, OSSystemExtensionRequestDelegate {
    enum Outcome: Equatable {
        case activated
        /// macOS finishes the activation at the next restart.
        case needsRestart
        /// The app is not in /Applications, where macOS requires it to be.
        case notInApplications
        case failed(code: Int)
    }

    private var continuation: CheckedContinuation<Outcome, Never>?
    private let needsApproval: () -> Void

    private init(needsApproval: @escaping () -> Void) {
        self.needsApproval = needsApproval
    }

    /// Submits the activation request and waits for its result, which waits for the user's approval when macOS asks
    /// for it; `needsApproval` runs when it does.
    static func run(_ identifier: String, needsApproval: @escaping () -> Void) async -> Outcome {
        let activation = SystemExtensionActivation(needsApproval: needsApproval)
        let outcome = await withCheckedContinuation { continuation in
            DispatchQueue.main.async {
                activation.continuation = continuation
                let request = OSSystemExtensionRequest.activationRequest(forExtensionWithIdentifier: identifier, queue: .main)
                request.delegate = activation
                OSSystemExtensionManager.shared.submitRequest(request)
            }
        }
        // The request holds its delegate weakly.
        withExtendedLifetime(activation) {}
        return outcome
    }

    private func finish(_ outcome: Outcome) {
        continuation?.resume(returning: outcome)
        continuation = nil
    }

    func request(
        _ request: OSSystemExtensionRequest,
        actionForReplacingExtension existing: OSSystemExtensionProperties,
        withExtension ext: OSSystemExtensionProperties
    ) -> OSSystemExtensionRequest.ReplacementAction {
        .replace
    }

    func requestNeedsUserApproval(_ request: OSSystemExtensionRequest) {
        needsApproval()
    }

    func request(_ request: OSSystemExtensionRequest, didFinishWithResult result: OSSystemExtensionRequest.Result) {
        finish(result == .willCompleteAfterReboot ? .needsRestart : .activated)
    }

    func request(_ request: OSSystemExtensionRequest, didFailWithError error: Error) {
        if let error = error as? OSSystemExtensionError, error.code == .unsupportedParentBundleLocation {
            finish(.notInApplications)
        } else {
            finish(.failed(code: (error as NSError).code))
        }
    }
}
