import Foundation

/// The command palette's search: which pages, commands and nodes match what was typed, and in what order
/// (AnixOps Design `guidelines/keyboard.md` K11, K12).
public enum CommandPalette {
    public enum Kind: String, Sendable {
        case page
        case command
        case node
    }

    public struct Item: Identifiable, Equatable, Sendable {
        public let id: String
        public let kind: Kind
        /// The same words as the command's button or menu item.
        public let title: String
        public let detail: String?
        /// The shortcut as shown, for example "⇧⌘C".
        public let shortcut: String?
        /// `title` lowercased, and its letters with every Chinese character replaced by the first letter of its
        /// pinyin, so "xg" finds "香港". Worked out once, not on every key press.
        let folded: String
        let initials: String

        public init(id: String, kind: Kind, title: String, detail: String? = nil, shortcut: String? = nil) {
            self.id = id
            self.kind = kind
            self.title = title
            self.detail = detail
            self.shortcut = shortcut
            self.folded = title.lowercased()
            self.initials = CommandPalette.initials(title)
        }
    }

    /// How many recently run commands are kept and shown first.
    public static let recentLimit = 5

    /// The rows for a query. An empty query lists the recently run items, then the pages and commands as given;
    /// nodes appear only once something is typed, because there can be thousands of them. Otherwise the best match
    /// comes first: the whole name, then its start, then a part of it, then pinyin initials, then the detail line;
    /// among equals a recently run item wins, then the given order.
    public static func search(_ query: String, in items: [Item], recent: [String] = [], limit: Int = 50) -> [Item] {
        let text = query.trimmingCharacters(in: .whitespaces).lowercased()
        if text.isEmpty {
            let recentItems = recent.compactMap { id in items.first { $0.id == id } }
            let rest = items.filter { $0.kind != .node && !recent.contains($0.id) }
            return Array((recentItems + rest).prefix(limit))
        }
        let scored = items.enumerated().compactMap { index, item -> (score: Int, recent: Int, index: Int, item: Item)? in
            guard let score = score(text, item) else {
                return nil
            }
            return (score, recent.firstIndex(of: item.id) ?? Int.max, index, item)
        }
        let sorted = scored.sorted { left, right in
            if left.score != right.score {
                return left.score < right.score
            }
            if left.recent != right.recent {
                return left.recent < right.recent
            }
            return left.index < right.index
        }
        return sorted.prefix(limit).map(\.item)
    }

    /// Lower is better; nil when the item does not match. `text` is trimmed and lowercased.
    static func score(_ text: String, _ item: Item) -> Int? {
        if item.folded == text {
            return 0
        }
        if item.folded.hasPrefix(text) {
            return 1
        }
        if item.folded.contains(text) {
            return 2
        }
        if item.initials.hasPrefix(text) {
            return 3
        }
        if item.initials.contains(text) {
            return 4
        }
        if let detail = item.detail, detail.lowercased().contains(text) {
            return 5
        }
        return nil
    }

    /// The recent list after running `id`: newest first, each id once, at most `recentLimit`.
    public static func remember(_ id: String, in recent: [String]) -> [String] {
        Array(([id] + recent.filter { $0 != id }).prefix(recentLimit))
    }

    /// ASCII letters and digits as they are (lowercased), every other character that has a Latin reading reduced
    /// to its first letter, the rest dropped: "香港 01" gives "xg01".
    static func initials(_ text: String) -> String {
        var result = ""
        for character in text {
            if character.isASCII {
                if character.isLetter || character.isNumber {
                    result += character.lowercased()
                }
                continue
            }
            let latin = String(character)
                .applyingTransform(.toLatin, reverse: false)?
                .applyingTransform(.stripDiacritics, reverse: false)
            if let first = latin?.lowercased().first, first.isASCII, first.isLetter {
                result.append(first)
            }
        }
        return result
    }
}
