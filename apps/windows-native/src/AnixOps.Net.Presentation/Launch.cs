namespace AnixOps.Net.Presentation;

/// <summary>Which colours the window uses.</summary>
public enum Appearance
{
    /// <summary>Follow Windows.</summary>
    System,
    Light,
    Dark,
}

/// <summary>
/// How the app was started. The options exist so that tests and screenshots can fix what otherwise follows
/// the system: <c>--language zh|en</c>, <c>--theme light|dark</c>, <c>--page &lt;id&gt;</c>.
/// </summary>
/// <param name="Language">The language asked for, or null to follow Windows.</param>
/// <param name="Appearance">The colours asked for.</param>
/// <param name="Page">The section to open on.</param>
public sealed record Launch(string? Language, Appearance Appearance, string Page)
{
    /// <summary>Reads the command line (without the program name). Unknown options and values are ignored:
    /// a shortcut written for a newer version must still open the app.</summary>
    public static Launch Parse(IEnumerable<string> arguments)
    {
        string? language = null;
        var appearance = Appearance.System;
        var page = Sections.Overview;
        using var rest = arguments.GetEnumerator();
        while (rest.MoveNext())
        {
            var name = rest.Current;
            string? value = null;
            var equals = name.IndexOf('=');
            if (name.StartsWith("--", StringComparison.Ordinal) && equals > 0)
            {
                value = name[(equals + 1)..];
                name = name[..equals];
            }
            if (name is not ("--language" or "--theme" or "--page"))
            {
                continue;
            }
            if (value is null)
            {
                if (!rest.MoveNext())
                {
                    break;
                }
                value = rest.Current;
            }
            switch (name)
            {
                case "--language" when Strings.Languages.Contains(value):
                    language = value;
                    break;
                case "--theme" when value == "light":
                    appearance = Appearance.Light;
                    break;
                case "--theme" when value == "dark":
                    appearance = Appearance.Dark;
                    break;
                case "--page" when Sections.Find(value) is not null:
                    page = value;
                    break;
            }
        }
        return new Launch(language, appearance, page);
    }
}
