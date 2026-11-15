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
/// the system: <c>--language zh|zh-Hant|en</c>, <c>--theme light|dark</c>, <c>--page &lt;id&gt;</c>,
/// <c>--demo-rows &lt;count&gt;</c>, <c>--measure &lt;file&gt;</c>, <c>--pipe &lt;name&gt;</c>.
/// </summary>
/// <param name="Language">The language asked for, or null to follow Windows.</param>
/// <param name="Appearance">The colours asked for.</param>
/// <param name="Page">The section to open on.</param>
/// <param name="DemoRows">How many made-up requests the request list shows instead of real ones.</param>
/// <param name="Measure">A file to write the request list's timings to; the app measures itself and exits.</param>
/// <param name="Pipe">The name of the core service's pipe (without <c>\\.\pipe\</c>), or null for the installed service's.</param>
public sealed record Launch(string? Language, Appearance Appearance, string Page, int DemoRows = 0, string? Measure = null, string? Pipe = null)
{
    /// <summary>The most demo requests that can be asked for.</summary>
    public const int MostDemoRows = 1_000_000;

    /// <summary>Reads the command line (without the program name). Unknown options and values are ignored:
    /// a shortcut written for a newer version must still open the app.</summary>
    public static Launch Parse(IEnumerable<string> arguments)
    {
        string? language = null;
        var appearance = Appearance.System;
        var page = Sections.Overview;
        var demoRows = 0;
        string? measure = null;
        string? pipe = null;
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
            if (name is not ("--language" or "--theme" or "--page" or "--demo-rows" or "--measure" or "--pipe"))
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
                case "--demo-rows" when int.TryParse(value, System.Globalization.NumberStyles.None, System.Globalization.CultureInfo.InvariantCulture, out var count)
                    && count <= MostDemoRows:
                    demoRows = count;
                    break;
                case "--measure" when value.Length > 0:
                    measure = value;
                    break;
                case "--pipe" when value.Length > 0 && !value.Contains('\\'):
                    pipe = value;
                    break;
            }
        }
        return new Launch(language, appearance, page, demoRows, measure, pipe);
    }
}
