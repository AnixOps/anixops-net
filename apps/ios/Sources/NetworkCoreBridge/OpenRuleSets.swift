import Foundation

/// Open-source rule lists on GitHub the rules page offers in one tap. Every entry is a Surge-format list the engine
/// reads itself (`TYPE,value` lines or bare domains). The engine keeps a list's domains in one packed table, about
/// 25 bytes a domain (networkcore #187), so even the 200 000-domain Loyalsoldier `reject` list costs some 5 MB in the
/// tunnel. Users can still add any other list by URL.
public enum OpenRuleSets {
    public enum Category: String, CaseIterable, Identifiable {
        case block
        case ai
        case media
        case services
        case direct

        public var id: String {
            rawValue
        }
    }

    public struct Entry: Identifiable, Equatable {
        public let id: String
        public let name: String
        public let category: Category
        public let url: String
        public let policy: LocalRules.Policy
        /// The project the list comes from, for the source line.
        public let source: String

        /// A subscription for this list.
        public func subscription(policy: LocalRules.Policy? = nil) -> LocalRules.Subscription {
            LocalRules.Subscription(name: name, url: url, policy: policy ?? self.policy, catalogID: id)
        }
    }

    static let blackmatrix7 = "blackmatrix7/ios_rule_script"
    static let loyalsoldier = "Loyalsoldier/surge-rules"

    private static func bm7(_ list: String) -> String {
        "https://raw.githubusercontent.com/blackmatrix7/ios_rule_script/master/rule/Surge/\(list)/\(list).list"
    }

    private static func ls(_ list: String) -> String {
        "https://raw.githubusercontent.com/Loyalsoldier/surge-rules/release/\(list).txt"
    }

    public static let entries: [Entry] = [
        Entry(id: "bm7-advertising", name: "广告拦截", category: .block, url: bm7("Advertising"), policy: .reject, source: blackmatrix7),
        Entry(id: "ls-reject", name: "广告拦截（完整，约 20 万条）", category: .block, url: ls("reject"), policy: .reject, source: loyalsoldier),
        Entry(id: "bm7-openai", name: "OpenAI", category: .ai, url: bm7("OpenAI"), policy: .proxy, source: blackmatrix7),
        Entry(id: "bm7-claude", name: "Claude", category: .ai, url: bm7("Claude"), policy: .proxy, source: blackmatrix7),
        Entry(id: "bm7-gemini", name: "Gemini", category: .ai, url: bm7("Gemini"), policy: .proxy, source: blackmatrix7),
        Entry(id: "bm7-youtube", name: "YouTube", category: .media, url: bm7("YouTube"), policy: .proxy, source: blackmatrix7),
        Entry(id: "bm7-netflix", name: "Netflix", category: .media, url: bm7("Netflix"), policy: .proxy, source: blackmatrix7),
        Entry(id: "bm7-disney", name: "Disney+", category: .media, url: bm7("Disney"), policy: .proxy, source: blackmatrix7),
        Entry(id: "bm7-spotify", name: "Spotify", category: .media, url: bm7("Spotify"), policy: .proxy, source: blackmatrix7),
        Entry(id: "bm7-tiktok", name: "TikTok", category: .media, url: bm7("TikTok"), policy: .proxy, source: blackmatrix7),
        Entry(id: "bm7-telegram", name: "Telegram", category: .services, url: bm7("Telegram"), policy: .proxy, source: blackmatrix7),
        Entry(id: "ls-telegramcidr", name: "Telegram IP", category: .services, url: ls("telegramcidr"), policy: .proxy, source: loyalsoldier),
        Entry(id: "bm7-twitter", name: "X (Twitter)", category: .services, url: bm7("Twitter"), policy: .proxy, source: blackmatrix7),
        Entry(id: "bm7-google", name: "Google", category: .services, url: bm7("Google"), policy: .proxy, source: blackmatrix7),
        Entry(id: "bm7-github", name: "GitHub", category: .services, url: bm7("GitHub"), policy: .proxy, source: blackmatrix7),
        Entry(id: "ls-gfw", name: "GFW 列表", category: .services, url: ls("gfw"), policy: .proxy, source: loyalsoldier),
        Entry(id: "ls-proxy", name: "常见海外域名（约 2.7 万条）", category: .services, url: ls("proxy"), policy: .proxy, source: loyalsoldier),
        Entry(id: "bm7-apple", name: "Apple", category: .direct, url: bm7("Apple"), policy: .direct, source: blackmatrix7),
        Entry(id: "ls-icloud", name: "iCloud", category: .direct, url: ls("icloud"), policy: .direct, source: loyalsoldier),
        Entry(id: "bm7-microsoft", name: "Microsoft", category: .direct, url: bm7("Microsoft"), policy: .direct, source: blackmatrix7),
        Entry(id: "bm7-bilibili", name: "哔哩哔哩", category: .direct, url: bm7("BiliBili"), policy: .direct, source: blackmatrix7),
        Entry(id: "ls-private", name: "局域网与保留域名", category: .direct, url: ls("private"), policy: .direct, source: loyalsoldier),
    ]

    public static func entry(id: String) -> Entry? {
        entries.first { $0.id == id }
    }
}
