import Foundation

/// The user agreement (TERMS.md at the root of the anixops-net repository) and whether it still has to be accepted.
///
/// Someone accepts it once, before anything else in the app is usable. Only a new major version asks again: a minor
/// or patch version only clarifies wording. `TermsConsentTests` checks `version` against TERMS.md.
public enum TermsConsent {
    /// The terms version this build asks for, the "基准版本" line of TERMS.md.
    public static let version = "2.1.2"
    /// The `UserDefaults` key that holds the version last accepted.
    public static let storageKey = "terms-accepted-version"
    /// The full text, in the public repository.
    public static let url = URL(string: "https://github.com/AnixOps/anixops-net/blob/main/TERMS.md")!

    /// True when nothing was accepted yet, or what was accepted is an older major version than `current`.
    public static func needsConsent(accepted: String?, current: String = version) -> Bool {
        guard let acceptedMajor = major(accepted) else {
            return true
        }
        return acceptedMajor != major(current)
    }

    /// The terms version from the "基准版本 x.y.z" line of a TERMS.md text.
    public static func version(inTerms markdown: String) -> String? {
        guard let range = markdown.range(of: #"基准版本\s+\d+\.\d+\.\d+"#, options: .regularExpression) else {
            return nil
        }
        return markdown[range].split(whereSeparator: \.isWhitespace).last.map(String.init)
    }

    private static func major(_ version: String?) -> Int? {
        guard let version, let first = version.split(separator: ".").first, version.contains(".") else {
            return nil
        }
        return Int(first)
    }
}

/// The privacy policy (PRIVACY.md at the root of the anixops-net repository), linked from the first-start consent
/// screen and from Settings › About on iOS and macOS. The same address is entered in App Store Connect.
public enum PrivacyPolicy {
    /// The full text, in the public repository.
    public static let url = URL(string: "https://github.com/AnixOps/anixops-net/blob/main/PRIVACY.md")!
}

/// The extra confirmation the first time someone turns HTTPS decryption on (TERMS.md section 4: capture and decrypt
/// only traffic you are entitled to see). Asked once per installation, on iOS and macOS alike.
///
/// Every way of starting decryption goes through it: the plugins' master switch, and every change of the debug
/// mappings that decrypts a domain the saved mappings did not (a line added to the decryption list, a paused line
/// turned on, an exclusion turned into a decrypted line, a Map Local / Map Remote entry saved or turned on, "解密此域名").
/// The app asks before it saves such a change (`isRequired(from:to:accepted:)`), and as a fallback the tunnel profile
/// never carries a mappings plugin that decrypts until it has been confirmed (`profilePlugin(for:accepted:)`).
public enum DecryptionConsent {
    /// The `UserDefaults` key that holds the terms version in force when it was confirmed.
    public static let storageKey = "https-decryption-consent-version"

    /// True until someone has confirmed once.
    public static func needsConsent(accepted: String?) -> Bool {
        accepted?.isEmpty ?? true
    }

    /// Whether saving `next` over `current` has to wait for the confirmation: it was never given, and `next` decrypts
    /// a pattern `current` does not. Removing, pausing or excluding a line, turning a mapping off and adding only
    /// exclusions decrypt nothing new and go ahead.
    public static func isRequired(from current: DebugMappings, to next: DebugMappings, accepted: String?) -> Bool {
        guard needsConsent(accepted: accepted) else {
            return false
        }
        return !Set(next.decryptedPatterns).subtracting(current.decryptedPatterns).isEmpty
    }

    /// The debug mappings' plugin as the tunnel profile may carry it: `mappings.pluginText`, except that a plugin
    /// that decrypts any domain is left out until the confirmation was given. A plugin that only excludes domains
    /// decrypts nothing and stays.
    public static func profilePlugin(for mappings: DebugMappings, accepted: String?) -> String? {
        guard let text = mappings.pluginText else {
            return nil
        }
        if needsConsent(accepted: accepted), !mappings.decryptedPatterns.isEmpty {
            return nil
        }
        return text
    }
}
