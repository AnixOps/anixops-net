import Foundation

/// The user agreement (TERMS.md at the root of the anixops-net repository) and whether it still has to be accepted.
///
/// Someone accepts it once, before anything else in the app is usable. Only a new major version asks again: a minor
/// or patch version only clarifies wording. `TermsConsentTests` checks `version` against TERMS.md.
public enum TermsConsent {
    /// The terms version this build asks for, the "基准版本" line of TERMS.md.
    public static let version = "2.0.0"
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
