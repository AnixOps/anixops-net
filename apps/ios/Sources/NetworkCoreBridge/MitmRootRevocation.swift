#if os(macOS)
import Foundation
import Security

/// The system calls `MitmRootRevocation` makes, so the order and the handling of each answer can be tested.
public protocol MitmRootTrustStore {
    /// `SecTrustSettingsRemoveTrustSettings` in the user domain.
    func removeUserTrust(_ certificateDER: Data) -> OSStatus
    /// `SecItemDelete` of the certificate in the login keychain.
    func removeFromKeychain(_ certificateDER: Data) -> OSStatus
    /// Whether the system would accept a leaf issued by the root (`MitmCertificateAuthority.evaluateTrust`).
    func isTrusted(_ certificateDER: Data) -> Bool
}

/// Takes back a Mac's trust in the MITM root that a new one replaced (generated, or the user's own imported).
///
/// The app trusts its root through the user trust settings (`SecTrustSettingsSetTrustSettings`), which macOS keeps
/// apart from the login keychain item and finds by the certificate's hash alone. Deleting the item leaves the trust
/// in force: anyone holding the old private key could still present a chain ending at the old root and Safari would
/// accept it. So the trust setting goes first, then the item, and the result is checked the way an attacker's chain
/// would be (`isTrusted`). When the trust cannot be removed (the person cancelled the password prompt, or it lives
/// in the admin domain the app cannot change), the item stays in the login keychain so Keychain Access still shows
/// the certificate to remove by hand.
public enum MitmRootRevocation {
    /// What happened to the old root.
    public enum Outcome: Equatable, Sendable {
        /// No longer trusted and gone from the login keychain.
        case revoked
        /// The person cancelled the password prompt: still trusted, item kept.
        case cancelled
        /// The user trust setting could not be removed (the OSStatus): still trusted, item kept.
        case failed(OSStatus)
        /// The user trust is gone but the system still trusts the root, through a setting this user cannot change
        /// (the admin domain, or the System keychain).
        case stillTrusted
        /// No longer trusted, but the login keychain item could not be deleted (the OSStatus).
        case itemKept(OSStatus)
    }

    /// Answers of `removeUserTrust` that mean there was no user trust to remove.
    static let noTrust: Set<OSStatus> = [errSecSuccess, errSecItemNotFound, errSecNoTrustSettings]
    /// Answers that mean the person cancelled the authorization prompt.
    static let cancelledAnswers: Set<OSStatus> = [errAuthorizationCanceled, errSecAuthFailed, errSecUserCanceled]

    /// Revokes `previous` once `current` replaced it; nil when it is the same certificate (nothing was replaced).
    public static func revoke(previous: Data, current: Data?, store: MitmRootTrustStore) -> Outcome? {
        guard previous != current else {
            return nil
        }
        let removed = store.removeUserTrust(previous)
        if cancelledAnswers.contains(removed) {
            return .cancelled
        }
        guard noTrust.contains(removed) else {
            return .failed(removed)
        }
        let deleted = store.removeFromKeychain(previous)
        if store.isTrusted(previous) {
            return .stillTrusted
        }
        guard deleted == errSecSuccess || deleted == errSecItemNotFound else {
            return .itemKept(deleted)
        }
        return .revoked
    }
}

extension MitmRootRevocation {
    /// The real trust settings and login keychain of the signed-in user.
    public struct SystemTrustStore: MitmRootTrustStore {
        /// Whether the system accepts a leaf the old root signed, presented with the root in the chain: what an
        /// attacker holding the old key would send (the caller still has that key in memory).
        let evaluate: (Data) -> Bool

        public init(evaluate: @escaping (Data) -> Bool) {
            self.evaluate = evaluate
        }

        public func removeUserTrust(_ certificateDER: Data) -> OSStatus {
            guard let certificate = SecCertificateCreateWithData(nil, certificateDER as CFData) else {
                return errSecParam
            }
            return SecTrustSettingsRemoveTrustSettings(certificate, .user)
        }

        public func removeFromKeychain(_ certificateDER: Data) -> OSStatus {
            guard let certificate = SecCertificateCreateWithData(nil, certificateDER as CFData) else {
                return errSecParam
            }
            return SecItemDelete([
                kSecClass as String: kSecClassCertificate,
                kSecValueRef as String: certificate,
            ] as CFDictionary)
        }

        public func isTrusted(_ certificateDER: Data) -> Bool {
            evaluate(certificateDER)
        }
    }
}
#endif
