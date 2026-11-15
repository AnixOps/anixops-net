using System.Globalization;

namespace AnixOps.Net.Presentation;

/// <summary>One line of the request list, with what each column shows already made.</summary>
public sealed class RequestRow
{
    public RequestRow(long seq, string method, int? status, string host, string path, string type, long size, int durationMs)
    {
        Seq = seq;
        Method = method;
        Status = status;
        Host = host;
        Path = path;
        Type = type;
        Size = size;
        DurationMs = durationMs;
        SeqText = seq.ToString(CultureInfo.InvariantCulture);
        StatusText = status?.ToString(CultureInfo.InvariantCulture) ?? "…";
        SizeText = Measures.Size(size);
        DurationText = Measures.Duration(durationMs);
        Search = string.Join(' ', method, StatusText, host, path, type).ToLowerInvariant();
    }

    public long Seq { get; }
    public string Method { get; }

    /// <summary>The response's status, or null while there is no response yet.</summary>
    public int? Status { get; }
    public string Host { get; }
    public string Path { get; }
    public string Type { get; }

    /// <summary>The response body's length in bytes.</summary>
    public long Size { get; }
    public int DurationMs { get; }

    public string SeqText { get; }
    public string StatusText { get; }
    public string SizeText { get; }
    public string DurationText { get; }

    /// <summary>What a filter is matched against, in lower case.</summary>
    public string Search { get; }

    /// <summary>The row as one sentence, for assistive technology.</summary>
    public override string ToString() => $"{Method} {StatusText} {Host}{Path}";
}

/// <summary>Sizes and durations as the lists show them.</summary>
public static class Measures
{
    /// <summary>A length in bytes: "512 B", "1.5 kB", "2.0 MB".</summary>
    public static string Size(long bytes) => bytes switch
    {
        < 1000 => string.Create(CultureInfo.InvariantCulture, $"{bytes} B"),
        < 1_000_000 => string.Create(CultureInfo.InvariantCulture, $"{bytes / 1000.0:0.0} kB"),
        < 1_000_000_000 => string.Create(CultureInfo.InvariantCulture, $"{bytes / 1_000_000.0:0.0} MB"),
        _ => string.Create(CultureInfo.InvariantCulture, $"{bytes / 1_000_000_000.0:0.0} GB"),
    };

    /// <summary>A duration: "85 ms", "1.20 s".</summary>
    public static string Duration(int milliseconds) => milliseconds < 1000
        ? string.Create(CultureInfo.InvariantCulture, $"{milliseconds} ms")
        : string.Create(CultureInfo.InvariantCulture, $"{milliseconds / 1000.0:0.00} s");
}

/// <summary>
/// Made-up requests for tests, screenshots and measuring the list. The same count always gives the same
/// rows. Hosts are under the reserved example domains.
/// </summary>
public static class DemoRequests
{
    private static readonly string[] Hosts =
    [
        "api.example.com", "www.example.org", "cdn.example.net", "static.example.com", "auth.example.net",
        "img.example.org", "telemetry.example.com", "search.example.net",
    ];

    private static readonly string[] Methods = ["GET", "GET", "GET", "GET", "GET", "POST", "POST", "PUT", "DELETE", "HEAD"];
    private static readonly int[] Statuses = [200, 200, 200, 200, 200, 200, 204, 301, 304, 304, 400, 401, 404, 404, 500, 503];
    private static readonly string[] Areas = ["v1", "v2", "assets", "users", "orders", "search", "feed", "media"];
    private static readonly string[] Things = ["items", "profile", "list", "detail", "avatar", "bundle", "events", "token"];

    private static readonly (string Type, string Extension)[] Kinds =
    [
        ("json", ""), ("json", ""), ("json", ".json"), ("html", ".html"), ("image", ".png"), ("image", ".webp"),
        ("js", ".js"), ("css", ".css"), ("other", ""),
    ];

    public static IReadOnlyList<RequestRow> Make(int count)
    {
        var rows = new RequestRow[count];
        var state = 0x9E3779B97F4A7C15UL;
        for (var index = 0; index < count; index++)
        {
            var kind = Kinds[Next(ref state, Kinds.Length)];
            var path = $"/{Areas[Next(ref state, Areas.Length)]}/{Things[Next(ref state, Things.Length)]}/{Next(ref state, 100_000)}{kind.Extension}";
            // One request in forty is still waiting for its response.
            int? status = Next(ref state, 40) == 0 ? null : Statuses[Next(ref state, Statuses.Length)];
            var size = status is null ? 0 : (long)Next(ref state, 2000) * Next(ref state, 2000);
            rows[index] = new RequestRow(
                index + 1,
                Methods[Next(ref state, Methods.Length)],
                status,
                Hosts[Next(ref state, Hosts.Length)],
                path,
                kind.Type,
                size,
                5 + Next(ref state, 60) * Next(ref state, 60));
        }
        return rows;
    }

    // xorshift64*: small, and the same on every machine and runtime version.
    private static int Next(ref ulong state, int below)
    {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        return (int)(((state * 0x2545F4914F6CDD1DUL) >> 33) % (ulong)below);
    }
}

/// <summary>
/// The demo list's filter: every word typed must occur somewhere in the row, in any case. The real list is
/// filtered by the engine (its filter expression, joined by client-view's <c>view.combine_filter</c>); this
/// stands in for it so that the table can be measured before the list is wired to the core.
/// </summary>
public static class RequestFilter
{
    public static IReadOnlyList<RequestRow> Apply(IReadOnlyList<RequestRow> rows, string? typed)
    {
        var words = (typed ?? "").ToLowerInvariant().Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
        if (words.Length == 0)
        {
            return rows;
        }
        var kept = new List<RequestRow>();
        foreach (var row in rows)
        {
            var search = row.Search;
            var all = true;
            foreach (var word in words)
            {
                if (!search.Contains(word, StringComparison.Ordinal))
                {
                    all = false;
                    break;
                }
            }
            if (all)
            {
                kept.Add(row);
            }
        }
        return kept;
    }
}

/// <summary>What the app writes when started with <c>--measure</c>: how the request list behaved.</summary>
/// <param name="Rows">How many requests the list held.</param>
/// <param name="Filters">One sample per filter typed.</param>
/// <param name="Scroll">The scrolling sweep, or null if it did not run.</param>
/// <param name="Error">What went wrong, or null.</param>
public sealed record Measurement(int Rows, IReadOnlyList<FilterSample> Filters, ScrollSample? Scroll, string? Error = null);

/// <param name="Text">The filter typed.</param>
/// <param name="Matches">How many requests it kept.</param>
/// <param name="Milliseconds">From setting the filter box's text until two frames after the list showed the result.</param>
public sealed record FilterSample(string Text, int Matches, double Milliseconds);

/// <param name="Steps">How many times the list was moved.</param>
/// <param name="BlankRows">Rows inside the viewport that had no content on the frame after a move, over all steps.</param>
/// <param name="WorstMilliseconds">The slowest move, until the frame after it.</param>
/// <param name="RowHeight">A row's height in effective pixels.</param>
/// <param name="Viewport">The list's visible height in effective pixels.</param>
public sealed record ScrollSample(int Steps, int BlankRows, double WorstMilliseconds, double RowHeight, double Viewport);
