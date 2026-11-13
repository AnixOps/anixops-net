import Foundation

/// The user's local rules as one editable text, one Surge / Loon rule a line, for the rule editor: the text of a
/// `LocalRules` (`text(of:)`), the colored pieces of each line (`tokens(in:)`), what is wrong and where (`issues(in:)`),
/// and the rules the text makes (`apply(_:to:)`).
///
/// A line starting with `#`, `;` or `//` is a comment. A comment that is exactly a rule (`# DOMAIN,example.com,DIRECT`)
/// is that rule switched off, so the list's on/off switches survive a trip through the text; other comments are not
/// kept. `FINAL` is not a user rule: the preset or the subscription decides it.
///
/// Positions are UTF-16 offsets, as `NSRange`, `UITextView` and `NSTextView` count them.
public enum RuleText {
    /// What a piece of a line is, for coloring.
    public enum TokenKind: String, Equatable, Sendable {
        case comment
        /// The rule type, such as `DOMAIN-SUFFIX`.
        case kind
        case value
        case policy
        /// `no-resolve` and the other trailing options.
        case option
        /// A comma between fields.
        case separator
    }

    public struct Token: Equatable, Sendable {
        public let kind: TokenKind
        /// Offset from the start of the whole text.
        public let location: Int
        public let length: Int

        public var range: NSRange {
            NSRange(location: location, length: length)
        }
    }

    /// Why a line cannot be kept.
    public enum Problem: Equatable, Sendable {
        /// A type the editor does not know, with the type as written.
        case unsupportedKind(String)
        /// The line has a type but no value.
        case missingValue
        /// The value does not fit the type; the engine would refuse the whole routing for it.
        case invalidValue
        /// `FINAL` / `MATCH`: decided by the preset or the subscription.
        case final
        /// The line would be rule number `LocalRules.maxRules + 1` or later.
        case tooMany
    }

    /// One problem on one line. `line` counts from 0; `location` and `length` mark the bad field in the whole text.
    public struct Issue: Equatable, Identifiable, Sendable {
        public let line: Int
        public let location: Int
        public let length: Int
        public let problem: Problem

        public var id: Int {
            line
        }

        public var range: NSRange {
            NSRange(location: location, length: length)
        }
    }

    /// The text cannot be applied; `issues` is never empty.
    public struct Rejected: Error, Equatable {
        public let issues: [Issue]
    }

    // MARK: - Lines

    /// One line of the text: its characters without the line break, and where it starts.
    struct Line {
        let index: Int
        let text: String
        /// UTF-16 offset of the first character in the whole text.
        let start: Int
    }

    static func lines(of text: String) -> [Line] {
        var lines: [Line] = []
        var start = 0
        var index = 0
        // `\r\n`, `\n` and `\r` all end a line, as text views treat them.
        var current = ""
        var pendingCR = false
        for scalar in text.unicodeScalars {
            if pendingCR {
                pendingCR = false
                if scalar == "\n" {
                    start += 1
                    continue
                }
            }
            if scalar == "\n" || scalar == "\r" {
                lines.append(Line(index: index, text: current, start: start))
                start += current.utf16.count + 1
                index += 1
                current = ""
                pendingCR = scalar == "\r"
                continue
            }
            current.unicodeScalars.append(scalar)
        }
        lines.append(Line(index: index, text: current, start: start))
        return lines
    }

    /// One comma-separated field of a line: its text and its UTF-16 offset in the line.
    struct Field {
        let text: String
        let offset: Int
    }

    static func fields(of line: String) -> [Field] {
        var fields: [Field] = []
        var offset = 0
        for part in line.split(separator: ",", omittingEmptySubsequences: false) {
            fields.append(Field(text: String(part), offset: offset))
            offset += part.utf16.count + 1
        }
        return fields
    }

    static let commentPrefixes = ["#", ";", "//"]

    /// The rule text after a comment marker, or nil when the line is not a comment.
    static func commentBody(_ trimmed: String) -> String? {
        for prefix in commentPrefixes where trimmed.hasPrefix(prefix) {
            return String(trimmed.dropFirst(prefix.count)).trimmingCharacters(in: .whitespaces)
        }
        return nil
    }

    static func isFinal(_ trimmed: String) -> Bool {
        let head = trimmed.split(separator: ",", maxSplits: 1).first.map { $0.trimmingCharacters(in: .whitespaces).uppercased() }
        return head == "FINAL" || head == "MATCH"
    }

    // MARK: - Text

    /// Every rule a line, in matching order; switched-off rules as `# ` comments.
    public static func text(of rules: LocalRules) -> String {
        rules.rules.map { rule in
            rule.enabled ? rule.text : "# " + rule.text
        }
        .joined(separator: "\n")
    }

    // MARK: - Coloring

    /// The colored pieces of every line. Unknown types and bad values are still colored as what they stand in for;
    /// `issues(in:)` marks them.
    public static func tokens(in text: String) -> [Token] {
        var tokens: [Token] = []
        for line in lines(of: text) {
            let leading = line.text.prefix { $0 == " " || $0 == "\t" }.utf16.count
            let trimmed = line.text.trimmingCharacters(in: .whitespaces)
            guard !trimmed.isEmpty else {
                continue
            }
            if commentBody(trimmed) != nil {
                tokens.append(Token(kind: .comment, location: line.start + leading, length: trimmed.utf16.count))
                continue
            }
            let options: Set<String> = ["no-resolve", "pre-matching", "extended-matching"]
            for (position, field) in fields(of: line.text).enumerated() {
                let location = line.start + field.offset
                if position > 0 {
                    tokens.append(Token(kind: .separator, location: location - 1, length: 1))
                }
                let fieldLeading = field.text.prefix { $0 == " " || $0 == "\t" }.utf16.count
                let word = field.text.trimmingCharacters(in: .whitespaces)
                guard !word.isEmpty else {
                    continue
                }
                let kind: TokenKind
                switch position {
                case 0:
                    kind = .kind
                case 1:
                    kind = .value
                case 2 where !options.contains(word.lowercased()):
                    kind = .policy
                default:
                    kind = .option
                }
                tokens.append(Token(kind: kind, location: location + fieldLeading, length: word.utf16.count))
            }
        }
        return tokens
    }

    // MARK: - Checking

    /// Every line that cannot be kept, top to bottom, at most one problem a line.
    public static func issues(in text: String) -> [Issue] {
        check(text).issues
    }

    /// The rules of `text`, keeping the id of each unchanged rule of `rules` so the list keeps its identity; the
    /// preset, the private-network switch and the rule sets stay as they are. Throws `Rejected` with every problem
    /// when one line cannot be kept, and then nothing changes.
    public static func apply(_ text: String, to rules: LocalRules) throws -> LocalRules {
        let checked = check(text)
        guard checked.issues.isEmpty else {
            throw Rejected(issues: checked.issues)
        }
        var unused = rules.rules
        var next = rules
        next.rules = checked.rules.map { parsed in
            var rule = parsed
            if let index = unused.firstIndex(where: { $0.text == rule.text && $0.enabled == rule.enabled }) {
                rule.id = unused[index].id
                unused.remove(at: index)
            }
            return rule
        }
        return next
    }

    static func check(_ text: String) -> (rules: [LocalRules.Rule], issues: [Issue]) {
        var rules: [LocalRules.Rule] = []
        var issues: [Issue] = []
        for line in lines(of: text) {
            let leading = line.text.prefix { $0 == " " || $0 == "\t" }.utf16.count
            let trimmed = line.text.trimmingCharacters(in: .whitespaces)
            guard !trimmed.isEmpty else {
                continue
            }
            var body = trimmed
            var enabled = true
            if let commented = commentBody(trimmed) {
                // A comment that reads as a rule is a switched-off rule; any other comment is just a note.
                guard !commented.isEmpty, !isFinal(commented), let rule = try? LocalRules.rule(from: commented) else {
                    continue
                }
                rules.append(rule)
                rules[rules.count - 1].enabled = false
                body = commented
                enabled = false
            }
            let wholeLine = Issue(line: line.index, location: line.start + leading, length: trimmed.utf16.count, problem: .final)
            if enabled {
                if isFinal(trimmed) {
                    issues.append(wholeLine)
                    continue
                }
                do {
                    rules.append(try LocalRules.rule(from: body))
                } catch {
                    issues.append(issue(for: error, line: line, fallback: wholeLine))
                    continue
                }
            }
            if rules.count > LocalRules.maxRules {
                issues.append(Issue(line: line.index, location: wholeLine.location, length: wholeLine.length, problem: .tooMany))
            }
        }
        return (rules, issues)
    }

    /// Marks the field `LocalRules.rule(from:)` refused: the type for an unknown type, else the value.
    static func issue(for error: Error, line: Line, fallback: Issue) -> Issue {
        let fields = fields(of: line.text)
        func mark(_ position: Int, _ problem: Problem) -> Issue {
            guard position < fields.count else {
                return Issue(line: fallback.line, location: fallback.location, length: fallback.length, problem: problem)
            }
            let field = fields[position]
            let leading = field.text.prefix { $0 == " " || $0 == "\t" }.utf16.count
            let length = field.text.trimmingCharacters(in: .whitespaces).utf16.count
            guard length > 0 else {
                return Issue(line: fallback.line, location: fallback.location, length: fallback.length, problem: problem)
            }
            return Issue(line: line.index, location: line.start + field.offset + leading, length: length, problem: problem)
        }
        switch error as? LocalRules.Failure {
        case let .unsupportedKind(kind):
            return mark(0, .unsupportedKind(kind))
        case .invalidValue:
            return mark(1, .invalidValue)
        case .empty, .tooMany, .none:
            return mark(0, .missingValue)
        }
    }

    /// The range of line `index` (from 0) without its line break, for selecting it.
    public static func range(ofLine index: Int, in text: String) -> NSRange? {
        guard let line = lines(of: text).first(where: { $0.index == index }) else {
            return nil
        }
        return NSRange(location: line.start, length: line.text.utf16.count)
    }
}
