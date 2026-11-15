namespace AnixOps.Net.Presentation;

/// <summary>
/// The app's wording in one language. This table covers the shell only and goes away when the clients share
/// one wording source (docs/architecture/desktop-native.md, "文案单一来源").
/// </summary>
public sealed class Strings
{
    public const string Chinese = "zh";
    public const string English = "en";

    /// <summary>The languages there is wording for.</summary>
    public static IReadOnlyList<string> Languages { get; } = [Chinese, English];

    private static readonly Dictionary<string, string> Zh = new()
    {
        ["app.name"] = "AnixOps Net",
        ["app.preview"] = "预览",
        ["nav.label"] = "页面",
        ["nav.overview"] = "概览",
        ["nav.proxies"] = "代理",
        ["nav.rules"] = "规则",
        ["nav.extensions"] = "扩展",
        ["nav.activity"] = "活动",
        ["nav.settings"] = "设置",
        ["page.unfinished"] = "原生版预览：这一页还没有完成。",
        ["page.overview.summary"] = "连接开关、当前节点、速率和流量。",
        ["page.proxies.summary"] = "订阅、节点、测速和策略组。",
        ["page.rules.summary"] = "分流模式、我的规则、规则集和 DNS。",
        ["page.extensions.summary"] = "插件、HTTPS 解密和改写。",
        ["page.activity.summary"] = "经过客户端的请求和连接。",
        ["page.settings.summary"] = "启动、通知、语言和诊断。",
    };

    private static readonly Dictionary<string, string> En = new()
    {
        ["app.name"] = "AnixOps Net",
        ["app.preview"] = "Preview",
        ["nav.label"] = "Pages",
        ["nav.overview"] = "Overview",
        ["nav.proxies"] = "Proxies",
        ["nav.rules"] = "Rules",
        ["nav.extensions"] = "Extensions",
        ["nav.activity"] = "Activity",
        ["nav.settings"] = "Settings",
        ["page.unfinished"] = "Native preview: this page is not finished yet.",
        ["page.overview.summary"] = "The connection switch, the current node, speed and traffic.",
        ["page.proxies.summary"] = "Subscriptions, nodes, latency tests and policy groups.",
        ["page.rules.summary"] = "Routing mode, your rules, rule sets and DNS.",
        ["page.extensions.summary"] = "Plugins, HTTPS decryption and rewriting.",
        ["page.activity.summary"] = "The requests and connections passing through the client.",
        ["page.settings.summary"] = "Startup, notifications, language and diagnostics.",
    };

    private readonly Dictionary<string, string> table;

    private Strings(string language, Dictionary<string, string> table)
    {
        Language = language;
        this.table = table;
    }

    /// <summary>The language of this wording: <see cref="Chinese"/> or <see cref="English"/>.</summary>
    public string Language { get; }

    /// <summary>Every key this language has wording for.</summary>
    public IReadOnlyCollection<string> Keys => table.Keys;

    /// <summary>The wording for a key. A key without wording is a programming error.</summary>
    public string this[string key] =>
        table.TryGetValue(key, out var value) ? value : throw new KeyNotFoundException($"no wording for '{key}' in '{Language}'");

    /// <summary>The wording in a language; anything other than Chinese is English.</summary>
    public static Strings For(string language) =>
        language == Chinese ? new Strings(Chinese, Zh) : new Strings(English, En);

    /// <summary>The language to use for a system culture name such as "zh-CN" or "en-US".</summary>
    public static string Preferred(string? culture) =>
        culture is not null && (culture.Equals("zh", StringComparison.OrdinalIgnoreCase) || culture.StartsWith("zh-", StringComparison.OrdinalIgnoreCase))
            ? Chinese
            : English;
}
