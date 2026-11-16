using System.Globalization;
using System.Text.Json;

namespace AnixOps.Net.Presentation;

/// <summary>One open connection as the core describes it.</summary>
/// <param name="Id">What the core is told when the connection is closed.</param>
/// <param name="Target">Where it goes: a name and port, or an address and port.</param>
/// <param name="Network"><c>tcp</c> or <c>udp</c>.</param>
/// <param name="Rule">The rule that matched.</param>
/// <param name="Route">The policy groups passed through and the exit, by name.</param>
public sealed record Connection(ulong Id, string Target, string Network, string Rule, IReadOnlyList<string> Route, ulong Upload, ulong Download)
{
    public bool Equals(Connection? other) =>
        other is not null
        && Id == other.Id
        && Target == other.Target
        && Network == other.Network
        && Rule == other.Rule
        && Upload == other.Upload
        && Download == other.Download
        && Route.SequenceEqual(other.Route);

    public override int GetHashCode() => HashCode.Combine(Id, Target, Network, Rule, Upload, Download, Route.Count);
}

/// <summary>The core's answer about the open connections.</summary>
/// <param name="Running">Whether there is an engine whose connections could be listed.</param>
/// <param name="Total">How many connections are open, whatever the search leaves.</param>
/// <param name="Open">The ones the search leaves, the newest first.</param>
public sealed record ConnectionList(bool Running, ulong Total, IReadOnlyList<Connection> Open)
{
    /// <summary>What is known without an answer: no engine, no connections.</summary>
    public static ConnectionList Nothing { get; } = new(false, 0, []);

    public bool Equals(ConnectionList? other) =>
        other is not null && Running == other.Running && Total == other.Total && Open.SequenceEqual(other.Open);

    public override int GetHashCode() => HashCode.Combine(Running, Total, Open.Count);
}

/// <summary>One connection of the list as it is drawn.</summary>
/// <param name="Title">Where the connection goes, over the row.</param>
/// <param name="Details">The rule, the route and what was moved, in one line.</param>
/// <param name="Enabled">Whether the connection can be closed now.</param>
public sealed record ConnectionRow(ulong Id, string Title, string Details, bool Enabled);

/// <summary>The list as it is drawn.</summary>
/// <param name="Count">How many connections are open, or how many of them the search leaves.</param>
/// <param name="Notice">What stands in place of the rows when there are none.</param>
/// <param name="CanSearch">Whether there is something to search.</param>
/// <param name="CanCloseAll">Whether there is something for "close all" to close.</param>
public sealed record ConnectionsShown(string Count, IReadOnlyList<ConnectionRow> Rows, string? Notice, bool CanSearch, bool CanCloseAll)
{
    public bool Equals(ConnectionsShown? other) =>
        other is not null
        && Count == other.Count
        && Notice == other.Notice
        && CanSearch == other.CanSearch
        && CanCloseAll == other.CanCloseAll
        && Rows.SequenceEqual(other.Rows);

    public override int GetHashCode() => HashCode.Combine(Count, Notice, CanSearch, CanCloseAll, Rows.Count);
}

/// <summary>
/// The open connections of the overview page. Which connections are open, where each goes, by which rule and
/// through which groups and exit, and which ones a search leaves, is decided by the core, in its
/// <c>connections</c> command: every window gets the same answer. This reads that answer
/// (<see cref="Read"/>) and turns it into the list's rows (<see cref="Show"/>), the same way the Linux window
/// does (apps/linux-native/src/connections.rs).
/// </summary>
public static class Connections
{
    /// <summary>The core's command that answers with the open connections.</summary>
    public const string Command = "connections";

    /// <summary>The core's command that closes every open connection. It takes no arguments.</summary>
    public const string CloseAllCommand = "close_connections";

    /// <summary>The core's command that closes one connection.</summary>
    public const string CloseCommand = "close_connection";

    /// <summary>The arguments of <see cref="Command"/>: what the user typed into the search.</summary>
    public static JsonElement Args(string query) =>
        JsonSerializer.SerializeToElement(new Dictionary<string, string> { ["query"] = query });

    /// <summary>The arguments of <see cref="CloseCommand"/>.</summary>
    public static JsonElement CloseArgs(ulong id) =>
        JsonSerializer.SerializeToElement(new Dictionary<string, ulong> { ["id"] = id });

    /// <summary>Reads the core's answer. What is missing reads as "no engine, no connections".</summary>
    public static ConnectionList Read(JsonElement answer)
    {
        var open = new List<Connection>();
        if (Member(answer, "connections") is { ValueKind: JsonValueKind.Array } rows)
        {
            foreach (var row in rows.EnumerateArray())
            {
                var route = new List<string>();
                if (Member(row, "route") is { ValueKind: JsonValueKind.Array } names)
                {
                    foreach (var name in names.EnumerateArray())
                    {
                        if (name.ValueKind == JsonValueKind.String)
                        {
                            route.Add(name.GetString() ?? "");
                        }
                    }
                }
                open.Add(new Connection(
                    Number(row, "id"),
                    Text(row, "target"),
                    Text(row, "network"),
                    Text(row, "rule"),
                    route,
                    Number(row, "upload"),
                    Number(row, "download")));
            }
        }
        return new ConnectionList(Member(answer, "running") is { ValueKind: JsonValueKind.True }, Number(answer, "total"), open);
    }

    /// <summary>
    /// The list for what is known now. <paramref name="query"/> is what the search holds;
    /// <paramref name="busy"/> holds the connections the core is still closing, and
    /// <paramref name="closingAll"/> says that it is closing all of them.
    /// </summary>
    public static ConnectionsShown Show(Strings strings, ConnectionList list, string query, IReadOnlyCollection<ulong> busy, bool closingAll)
    {
        string count;
        if (query.Trim().Length > 0)
        {
            count = strings.Format("connections.count.shown", Whole((ulong)list.Open.Count), Whole(list.Total));
        }
        else if (list.Total == 1)
        {
            count = strings["connections.count.one"];
        }
        else
        {
            count = strings.Format("connections.count", Whole(list.Total));
        }
        var rows = new List<ConnectionRow>();
        foreach (var connection in list.Open)
        {
            var details = new List<string>();
            // Most connections are TCP; only the other kind is named.
            if (string.Equals(connection.Network, "udp", StringComparison.OrdinalIgnoreCase))
            {
                details.Add("UDP");
            }
            if (connection.Rule.Length > 0)
            {
                details.Add(connection.Rule);
            }
            if (connection.Route.Count > 0)
            {
                details.Add(string.Join(" → ", connection.Route));
            }
            details.Add(strings.Format("connections.traffic", Overview.Size(connection.Upload), Overview.Size(connection.Download)));
            rows.Add(new ConnectionRow(
                connection.Id,
                connection.Target,
                string.Join(" · ", details),
                !closingAll && !busy.Contains(connection.Id)));
        }
        string? notice;
        if (!list.Running)
        {
            notice = strings["connections.stopped"];
        }
        else if (rows.Count > 0)
        {
            notice = null;
        }
        else if (list.Total > 0)
        {
            notice = strings["connections.no_match"];
        }
        else
        {
            notice = strings["connections.empty"];
        }
        var some = list.Running && list.Total > 0;
        return new ConnectionsShown(count, rows, notice, some, some && !closingAll);
    }

    private static string Whole(ulong number) => number.ToString(CultureInfo.InvariantCulture);

    private static JsonElement? Member(JsonElement value, string name) =>
        value.ValueKind == JsonValueKind.Object && value.TryGetProperty(name, out var member) ? member : null;

    private static string Text(JsonElement value, string name) =>
        Member(value, name) is { ValueKind: JsonValueKind.String } text ? text.GetString() ?? "" : "";

    private static ulong Number(JsonElement value, string name) =>
        Member(value, name) is { ValueKind: JsonValueKind.Number } number && number.TryGetUInt64(out var whole) ? whole : 0;
}
