using System.Globalization;
using System.Text.Json;

namespace AnixOps.Net.Presentation;

/// <summary>One measure of a node checkup as it is drawn.</summary>
/// <param name="Label">What it measures, in the window's language.</param>
/// <param name="Value">The value with its unit; a dash when no sample succeeded.</param>
/// <param name="Warn">Whether it is marked: loss, a failure, or latency the download added.</param>
public sealed record CheckupLine(string Label, string Value, bool Warn);

/// <summary>A group of measures under a heading: latency, then the download when it ran.</summary>
/// <param name="Note">Said under the group, such as the download stopping at its limit.</param>
public sealed record CheckupSection(string Heading, IReadOnlyList<CheckupLine> Lines, string? Note)
{
    public bool Equals(CheckupSection? other) =>
        other is not null && Heading == other.Heading && Note == other.Note && Lines.SequenceEqual(other.Lines);

    public override int GetHashCode() => HashCode.Combine(Heading, Note, Lines.Count);
}

/// <summary>
/// The node checkup (节点体检): what the core is asked and what its answer reads as. The rows, their order and
/// which are marked come from the core (crates/client-view/src/checkup.rs); this words them and writes the
/// numbers. The same as the Linux window's (apps/linux-native/src/checkup.rs).
/// </summary>
public static class Checkup
{
    /// <summary>The core's command; it answers when the checkup is over, which takes seconds.</summary>
    public const string Command = "node_checkup";

    /// <summary>The download stops at this many bytes, so a checkup spends little of a metered node.</summary>
    public const ulong MaxDownloadBytes = 200_000_000;

    private const string None = "—";

    /// <summary>The arguments of <see cref="Command"/>: the kernel's defaults, and the download when asked.</summary>
    public static JsonElement Args(string id, bool throughput) =>
        throughput
            ? JsonSerializer.SerializeToElement(new Dictionary<string, object>
            {
                ["nodeId"] = id,
                ["options"] = new Dictionary<string, object>
                {
                    ["throughput"] = new Dictionary<string, ulong> { ["max_bytes"] = MaxDownloadBytes },
                },
            })
            : JsonSerializer.SerializeToElement(new Dictionary<string, string> { ["nodeId"] = id });

    /// <summary>What the checkup does, said before it is started.</summary>
    public static string Note(Strings strings, bool throughput) =>
        throughput ? $"{strings["checkup.note"]} {strings["checkup.note.throughput"]}" : strings["checkup.note"];

    /// <summary>Reads the core's answer to <see cref="Command"/>. What is missing is left out.</summary>
    public static IReadOnlyList<CheckupSection> Show(Strings strings, JsonElement answer)
    {
        var sections = new List<CheckupSection>();
        if (answer.ValueKind != JsonValueKind.Object
            || !answer.TryGetProperty("sections", out var list)
            || list.ValueKind != JsonValueKind.Array)
        {
            return sections;
        }
        foreach (var section in list.EnumerateArray().Where(section => section.ValueKind == JsonValueKind.Object))
        {
            var id = Text(section, "id");
            if (id is not ("latency" or "download"))
            {
                continue;
            }
            var lines = new List<CheckupLine>();
            if (section.TryGetProperty("rows", out var rows) && rows.ValueKind == JsonValueKind.Array)
            {
                foreach (var row in rows.EnumerateArray().Where(row => row.ValueKind == JsonValueKind.Object))
                {
                    var key = $"checkup.row.{Text(row, "id")}";
                    if (!strings.Keys.Contains(key))
                    {
                        continue;
                    }
                    var value = row.TryGetProperty("value", out var found) ? found : default;
                    lines.Add(new CheckupLine(
                        strings[key],
                        Value(strings, Text(row, "kind") ?? "", value),
                        row.TryGetProperty("warn", out var warn) && warn.ValueKind == JsonValueKind.True));
                }
            }
            var capped = section.TryGetProperty("capped", out var cap) && cap.ValueKind == JsonValueKind.True;
            sections.Add(new CheckupSection(
                strings[$"checkup.section.{id}"],
                lines,
                capped ? strings["checkup.capped"] : null));
        }
        return sections;
    }

    /// <summary>A value of the given kind with its unit: <c>ms</c>, <c>pct</c>, <c>mbps</c>, <c>bytes</c> or
    /// <c>failure</c> (a fixed reason).</summary>
    public static string Value(Strings strings, string kind, JsonElement value)
    {
        if (kind == "failure")
        {
            var code = value.ValueKind == JsonValueKind.String ? value.GetString() : null;
            var key = $"checkup.failure.{code}";
            return code is null ? None : strings.Keys.Contains(key) ? strings[key] : code;
        }
        if (value.ValueKind != JsonValueKind.Number)
        {
            return None;
        }
        var number = value.GetDouble();
        return kind switch
        {
            "ms" => Write(number < 10 ? "{0:F1} ms" : "{0:F0} ms", number),
            "pct" => Write("{0:F0}%", number),
            "mbps" => Write(number < 10 ? "{0:F2} Mbps" : "{0:F1} Mbps", number),
            "bytes" => Overview.Size(value.TryGetUInt64(out var bytes) ? bytes : (ulong)Math.Max(0, number)),
            _ => Write("{0}", number),
        };
    }

    private static string Write(string format, double number) =>
        string.Format(CultureInfo.InvariantCulture, format, number);

    private static string? Text(JsonElement element, string name) =>
        element.ValueKind == JsonValueKind.Object
        && element.TryGetProperty(name, out var value)
        && value.ValueKind == JsonValueKind.String
            ? value.GetString()
            : null;
}
