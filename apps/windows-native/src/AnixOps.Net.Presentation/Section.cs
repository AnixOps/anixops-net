namespace AnixOps.Net.Presentation;

/// <summary>One entry of the sidebar and the page behind it.</summary>
/// <param name="Id">Stable id: the navigation item's tag and the suffix of its automation id.</param>
/// <param name="Title">Wording key of the name shown in the sidebar and as the page heading.</param>
/// <param name="Summary">Wording key of the line under the heading.</param>
/// <param name="Glyph">Segoe Fluent Icons code point.</param>
/// <param name="Footer">Whether the entry sits at the bottom of the sidebar.</param>
public sealed record Section(string Id, string Title, string Summary, string Glyph, bool Footer = false)
{
    /// <summary>The automation id of the sidebar entry, for UI Automation and tests.</summary>
    public string AutomationId => "Nav." + Id;
}

/// <summary>The sidebar, in order. The same six pages as the other desktop clients.</summary>
public static class Sections
{
    public const string Overview = "overview";
    public const string Proxies = "proxies";
    public const string Rules = "rules";
    public const string Extensions = "extensions";
    public const string Activity = "activity";
    public const string Settings = "settings";

    public static IReadOnlyList<Section> All { get; } =
    [
        new(Overview, "nav.overview", "page.overview.summary", ""),
        new(Proxies, "nav.proxies", "page.proxies.summary", ""),
        new(Rules, "nav.rules", "page.rules.summary", ""),
        new(Extensions, "nav.extensions", "page.extensions.summary", ""),
        new(Activity, "nav.activity", "page.activity.summary", ""),
        new(Settings, "nav.settings", "page.settings.summary", "", Footer: true),
    ];

    /// <summary>The section with this id, or null.</summary>
    public static Section? Find(string? id) => All.FirstOrDefault(section => section.Id == id);
}
