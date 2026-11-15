using System.Globalization;
using System.Text.Json;

namespace AnixOps.Net.Presentation;

/// <summary>Whether the client is connected.</summary>
public enum ConnectionState
{
    Connected,
    Disconnected,

    /// <summary>The last try to connect did not work.</summary>
    Failed,

    /// <summary>The engine stopped by itself.</summary>
    CoreError,
}

/// <summary>The engine's counters.</summary>
/// <param name="UploadRate">Bytes per second.</param>
/// <param name="DownloadRate">Bytes per second.</param>
/// <param name="UploadTotal">Bytes since the engine started.</param>
/// <param name="DownloadTotal">Bytes since the engine started.</param>
/// <param name="Connections">Open connections.</param>
public sealed record Traffic(ulong UploadRate, ulong DownloadRate, ulong UploadTotal, ulong DownloadTotal, ulong Connections);

/// <summary>What the user asked for that the core has not finished yet.</summary>
public enum Pending
{
    Nothing,
    Connecting,
    Disconnecting,
}

/// <summary>What the button does when pressed.</summary>
public enum Press
{
    Connect,
    Disconnect,
}

/// <summary>The page as it is drawn: every sentence and number, ready to show.</summary>
/// <param name="Headline">The state in large letters.</param>
/// <param name="Detail">A sentence under it, when there is something to explain.</param>
/// <param name="Error">Why not, in the core's words.</param>
/// <param name="Button">What the button says.</param>
/// <param name="Press">What the button does.</param>
/// <param name="Enabled">Whether pressing can do anything now.</param>
/// <param name="Node">The node in use, or the sentence that says there is none.</param>
/// <param name="DownloadRate">For people, or the unknown mark.</param>
/// <param name="UploadRate">For people, or the unknown mark.</param>
/// <param name="DownloadTotal">"… in total", when known.</param>
/// <param name="UploadTotal">"… in total", when known.</param>
/// <param name="Connections">The number of open connections, or the unknown mark.</param>
public sealed record OverviewShown(
    string Headline,
    string? Detail,
    string? Error,
    string Button,
    Press Press,
    bool Enabled,
    string Node,
    string DownloadRate,
    string UploadRate,
    string? DownloadTotal,
    string? UploadTotal,
    string Connections);

/// <summary>
/// The core's state as the overview page needs it. What the state means (connected or not, which node,
/// whether connecting can work) is decided by the core, in its <c>overview</c> command: every window gets
/// the same answer. This reads that answer (<see cref="Read"/>) and turns it into the page's sentences and
/// numbers (<see cref="Show"/>), the same way the Linux window does (apps/linux-native/src/overview.rs).
/// </summary>
/// <param name="State">Whether the client is connected.</param>
/// <param name="Direct">Traffic leaves directly, not through a node.</param>
/// <param name="NodeLabel">The name of the node in use, or null when there is none.</param>
/// <param name="NodeProtocol">Its protocol; empty when the core did not say.</param>
/// <param name="CanConnect">Connecting can work: a node is chosen, or traffic leaves directly.</param>
/// <param name="Error">Why the client is not connected, in the core's words.</param>
/// <param name="Traffic">The counters, while connected.</param>
public sealed record Overview(
    ConnectionState State,
    bool Direct,
    string? NodeLabel,
    string NodeProtocol,
    bool CanConnect,
    string? Error,
    Traffic? Traffic)
{
    /// <summary>The core's command that answers with the overview. It takes no arguments.</summary>
    public const string Command = "overview";

    /// <summary>What stands where a number is not known.</summary>
    public const string Unknown = "—";

    private static readonly string[] Units = ["KB", "MB", "GB", "TB", "PB"];

    /// <summary>The core's command for a press of the button.</summary>
    public static string CommandFor(Press press) => press == Press.Connect ? "connect" : "disconnect";

    /// <summary>What is pending once the button was pressed.</summary>
    public static Pending PendingFor(Press press) => press == Press.Connect ? Pending.Connecting : Pending.Disconnecting;

    /// <summary>Reads the core's answer. What is missing reads as "not connected, nothing known".</summary>
    public static Overview Read(JsonElement answer)
    {
        var node = Member(answer, "node");
        var hasNode = node is { ValueKind: JsonValueKind.Object };
        return new Overview(
            Text(answer, "state") switch
            {
                "connected" => ConnectionState.Connected,
                "failed" => ConnectionState.Failed,
                "core_error" => ConnectionState.CoreError,
                _ => ConnectionState.Disconnected,
            },
            Direct: Text(answer, "egress") == "direct",
            NodeLabel: hasNode ? Text(node!.Value, "label") ?? "" : null,
            NodeProtocol: hasNode ? Text(node!.Value, "protocol") ?? "" : "",
            CanConnect: Member(answer, "canConnect") is { ValueKind: JsonValueKind.True },
            Error: Text(answer, "error"),
            Traffic: ReadTraffic(Member(answer, "traffic")));
    }

    /// <summary>
    /// The page for what is known now. <paramref name="overview"/> is null while the core does not answer
    /// (it is not running, or the first answer has not come); <paramref name="failure"/> is why the user's
    /// last press did not work.
    /// </summary>
    public static OverviewShown Show(Strings strings, bool coreAnswers, Overview? overview, Pending pending, string? failure)
    {
        var connected = overview?.State == ConnectionState.Connected;
        var headline = pending switch
        {
            Pending.Connecting => strings["overview.state.connecting"],
            Pending.Disconnecting => strings["overview.state.disconnecting"],
            _ => overview?.State switch
            {
                ConnectionState.Connected => strings["overview.state.connected"],
                ConnectionState.Failed => strings["overview.state.failed"],
                ConnectionState.CoreError => strings["overview.state.core_error"],
                _ => strings["overview.state.disconnected"],
            },
        };
        var press = connected ? Press.Disconnect : Press.Connect;
        var node = overview switch
        {
            null => Unknown,
            { Direct: true } => strings["overview.node.direct"],
            { NodeLabel: { } label, NodeProtocol: "" } => label,
            { NodeLabel: { } label } => $"{label} · {overview.NodeProtocol}",
            _ => strings["overview.node.none"],
        };
        var traffic = overview?.Traffic;
        return new OverviewShown(
            headline,
            Detail: coreAnswers ? null : strings["overview.no_core"],
            Error: failure ?? overview?.Error,
            Button: strings[press == Press.Connect ? "overview.connect" : "overview.disconnect"],
            press,
            Enabled: coreAnswers && pending == Pending.Nothing && overview is not null && (connected || overview.CanConnect),
            node,
            DownloadRate: traffic is null ? Unknown : Rate(traffic.DownloadRate),
            UploadRate: traffic is null ? Unknown : Rate(traffic.UploadRate),
            DownloadTotal: traffic is null ? null : strings.Format("overview.total", Size(traffic.DownloadTotal)),
            UploadTotal: traffic is null ? null : strings.Format("overview.total", Size(traffic.UploadTotal)),
            Connections: traffic is null ? Unknown : traffic.Connections.ToString(CultureInfo.InvariantCulture));
    }

    /// <summary>A number of bytes for people: <c>980 B</c>, <c>1.5 KB</c>, <c>700.0 MB</c>. Units step by 1024.</summary>
    public static string Size(ulong bytes)
    {
        if (bytes < 1024)
        {
            return string.Create(CultureInfo.InvariantCulture, $"{bytes} B");
        }
        var value = bytes / 1024.0;
        var unit = 0;
        // 1023.95 and up would print as "1024.0" of the smaller unit.
        while (value >= 1023.95 && unit + 1 < Units.Length)
        {
            value /= 1024.0;
            unit++;
        }
        // A value exactly half way between two tenths goes to the even one, as the other windows write it.
        var tenths = Math.Round(value * 10.0, MidpointRounding.ToEven) / 10.0;
        return string.Create(CultureInfo.InvariantCulture, $"{tenths:0.0} {Units[unit]}");
    }

    /// <summary>Bytes per second for people: <c>1.5 MB/s</c>.</summary>
    public static string Rate(ulong bytesPerSecond) => Size(bytesPerSecond) + "/s";

    private static JsonElement? Member(JsonElement value, string name) =>
        value.ValueKind == JsonValueKind.Object && value.TryGetProperty(name, out var member) ? member : null;

    private static string? Text(JsonElement value, string name) =>
        Member(value, name) is { ValueKind: JsonValueKind.String } text ? text.GetString() : null;

    private static Traffic? ReadTraffic(JsonElement? traffic)
    {
        if (traffic is not { ValueKind: JsonValueKind.Object } counters)
        {
            return null;
        }
        return Number(counters, "uploadRate") is { } uploadRate
            && Number(counters, "downloadRate") is { } downloadRate
            && Number(counters, "uploadTotal") is { } uploadTotal
            && Number(counters, "downloadTotal") is { } downloadTotal
            && Number(counters, "connections") is { } connections
            ? new Traffic(uploadRate, downloadRate, uploadTotal, downloadTotal, connections)
            : null;
    }

    private static ulong? Number(JsonElement value, string name) =>
        Member(value, name) is { ValueKind: JsonValueKind.Number } number && number.TryGetUInt64(out var whole) ? whole : null;
}
