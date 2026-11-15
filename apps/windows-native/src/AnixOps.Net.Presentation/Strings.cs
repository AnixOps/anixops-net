using System.Text.Json;

namespace AnixOps.Net.Presentation;

/// <summary>
/// The app's wording in one language. The sentences are not written here: they are read from the catalogue
/// every native window shares (crates/client-strings/catalogue.json), which is built into this library as it
/// is in the repository, so the Windows window cannot word anything differently from the others.
/// </summary>
public sealed class Strings
{
    public const string Chinese = "zh";
    public const string TraditionalChinese = "zh-Hant";
    public const string English = "en";

    /// <summary>The languages there is wording for, in the order they are offered.</summary>
    public static IReadOnlyList<string> Languages { get; } = [Chinese, TraditionalChinese, English];

    private static readonly Lazy<Dictionary<string, Dictionary<string, string>>> Catalogue = new(Load);

    private readonly Dictionary<string, string> table;

    private Strings(string language, Dictionary<string, string> table)
    {
        Language = language;
        this.table = table;
    }

    /// <summary>The language of this wording: one of <see cref="Languages"/>.</summary>
    public string Language { get; }

    /// <summary>Every key this language has wording for.</summary>
    public IReadOnlyCollection<string> Keys => table.Keys;

    /// <summary>The wording for a key. A key without wording is a programming error.</summary>
    public string this[string key] =>
        table.TryGetValue(key, out var value) ? value : throw new KeyNotFoundException($"no wording for '{key}' in '{Language}'");

    /// <summary>The wording for a key with its placeholders filled in. Numbers are written plainly, without
    /// grouping, so that the same values read the same everywhere.</summary>
    public string Format(string key, params object[] values) =>
        string.Format(System.Globalization.CultureInfo.InvariantCulture, this[key], values);

    /// <summary>The wording in a language; a language there is no wording for is English.</summary>
    public static Strings For(string language)
    {
        var known = Languages.Contains(language) ? language : English;
        return new Strings(known, Catalogue.Value[known]);
    }

    /// <summary>The language to use for a system culture name such as "zh-CN", "zh-Hant-TW" or "en-US".
    /// Taiwan, Hong Kong, Macau and anything written in the Traditional script read Traditional Chinese, other
    /// Chinese cultures read Simplified Chinese, everything else reads English.</summary>
    public static string Preferred(string? culture)
    {
        var parts = (culture ?? "").ToLowerInvariant().Split('-', '_');
        if (parts[0] != "zh")
        {
            return English;
        }
        return parts.Skip(1).Any(part => part is "hant" or "cht" or "tw" or "hk" or "mo") ? TraditionalChinese : Chinese;
    }

    /// <summary>Reads a catalogue: every id carries one sentence in every language and nothing else.</summary>
    public static Dictionary<string, Dictionary<string, string>> Read(Stream catalogue)
    {
        using var document = JsonDocument.Parse(catalogue);
        if (document.RootElement.ValueKind != JsonValueKind.Object)
        {
            throw new InvalidDataException("the wording catalogue is not an object");
        }
        var tables = Languages.ToDictionary(language => language, _ => new Dictionary<string, string>(StringComparer.Ordinal));
        foreach (var entry in document.RootElement.EnumerateObject())
        {
            if (entry.Value.ValueKind != JsonValueKind.Object || entry.Value.EnumerateObject().Count() != Languages.Count)
            {
                throw new InvalidDataException($"'{entry.Name}' does not have exactly one sentence per language");
            }
            foreach (var language in Languages)
            {
                if (!entry.Value.TryGetProperty(language, out var sentence) || sentence.ValueKind != JsonValueKind.String)
                {
                    throw new InvalidDataException($"'{entry.Name}' has no sentence in '{language}'");
                }
                if (!tables[language].TryAdd(entry.Name, sentence.GetString()!))
                {
                    throw new InvalidDataException($"'{entry.Name}' is in the catalogue twice");
                }
            }
        }
        return tables;
    }

    private static Dictionary<string, Dictionary<string, string>> Load()
    {
        using var stream = typeof(Strings).Assembly.GetManifestResourceStream("catalogue.json")
            ?? throw new InvalidDataException("the wording catalogue is not built into this library");
        return Read(stream);
    }
}
