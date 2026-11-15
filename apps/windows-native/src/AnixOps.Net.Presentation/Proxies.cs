using System.Text.Json;

namespace AnixOps.Net.Presentation;

/// <summary>What a node's last delay test said.</summary>
/// <param name="Tested">Whether the node was tested while this core ran.</param>
/// <param name="Milliseconds">The delay, or null when the test got no answer.</param>
/// <param name="Fast">Whether the core counts that delay as fast.</param>
public readonly record struct NodeDelay(bool Tested, ulong? Milliseconds, bool Fast)
{
    /// <summary>Never tested while this core ran.</summary>
    public static NodeDelay Untested => default;

    /// <summary>The test got no answer.</summary>
    public static NodeDelay Failed => new(true, null, false);

    /// <summary>The test was answered after this long.</summary>
    public static NodeDelay Answered(ulong milliseconds, bool fast) => new(true, milliseconds, fast);
}

/// <summary>One node of the list.</summary>
/// <param name="Selected">The node in use.</param>
public sealed record ProxyNode(string Id, string Label, string Protocol, bool Selected, NodeDelay Delay);

/// <summary>The nodes of one subscription, or (<paramref name="Group"/> null) those that belong to none.</summary>
/// <param name="Pasted">
/// Share links pasted into the client: the heading is this window's wording, not <paramref name="Group"/>,
/// which is the core's own name for them.
/// </param>
public sealed record ProxySection(string? Group, IReadOnlyList<ProxyNode> Nodes, bool Pasted = false);

/// <summary>What the user asked for on the proxies page.</summary>
public enum ProxyActionKind
{
    /// <summary>Import the subscription or share links at a location.</summary>
    Import,

    /// <summary>Test the delay of the nodes a search leaves.</summary>
    Test,

    /// <summary>Use the node with an id.</summary>
    Switch,
}

/// <summary>Something the proxies page asks of the core.</summary>
/// <param name="Kind">What is asked.</param>
/// <param name="Value">The location, the search or the node's id.</param>
public sealed record ProxyAction(ProxyActionKind Kind, string Value)
{
    /// <summary>The core's command.</summary>
    public string Command => Kind switch
    {
        ProxyActionKind.Import => "import_subscription",
        ProxyActionKind.Test => "test_proxies",
        _ => "switch_node",
    };

    /// <summary>The command's arguments.</summary>
    public JsonElement Args() => Kind switch
    {
        ProxyActionKind.Import => JsonSerializer.SerializeToElement(new Dictionary<string, string> { ["location"] = Value }),
        ProxyActionKind.Test => Proxies.Args(Value),
        _ => JsonSerializer.SerializeToElement(new Dictionary<string, string> { ["nodeId"] = Value }),
    };
}

/// <summary>
/// What the core has not finished yet. The three do not wait for each other: a node can be chosen while a
/// long list is still being tested.
/// </summary>
public readonly record struct ProxiesBusy(bool Importing, bool Testing, bool Switching)
{
    /// <summary>This, with <paramref name="kind"/> sent (true) or answered (false).</summary>
    public ProxiesBusy With(ProxyActionKind kind, bool busy) => kind switch
    {
        ProxyActionKind.Import => this with { Importing = busy },
        ProxyActionKind.Test => this with { Testing = busy },
        _ => this with { Switching = busy },
    };
}

/// <summary>How a delay is coloured.</summary>
public enum DelayTone
{
    /// <summary>Nothing to colour.</summary>
    Plain,
    Fast,
    Slow,
    Failed,
}

/// <summary>One node of the list as it is drawn.</summary>
/// <param name="Section">Which run of nodes the row belongs to, counted from 0.</param>
/// <param name="Heading">The heading over that run: the subscription's name.</param>
/// <param name="Delay">The delay for people; empty for a node that was never tested.</param>
/// <param name="Selected">The node in use.</param>
public sealed record ProxyRow(int Section, string Heading, string Id, string Label, string Protocol, string Delay, DelayTone Tone, bool Selected)
{
    /// <summary>Whether there is a protocol to show under the name.</summary>
    public bool HasProtocol => Protocol.Length > 0;

    /// <summary>How visible the mark for "in use" is. Hidden, not removed: the names stay in one column.</summary>
    public double MarkOpacity => Selected ? 1 : 0;

    // One flag per tone: the list draws the delay once per tone and shows the one that applies, so that
    // each keeps its theme colour.
    public bool IsPlain => Tone == DelayTone.Plain;

    public bool IsFast => Tone == DelayTone.Fast;

    public bool IsSlow => Tone == DelayTone.Slow;

    public bool IsFailed => Tone == DelayTone.Failed;

    /// <summary>The row said in one breath, for a screen reader: the protocol, the delay, and whether it is in use.</summary>
    public string Description(Strings strings)
    {
        var parts = new List<string>();
        if (Protocol.Length > 0)
        {
            parts.Add(Protocol);
        }
        if (Delay.Length > 0)
        {
            parts.Add(Delay);
        }
        if (Selected)
        {
            parts.Add(strings["proxies.selected"]);
        }
        return string.Join(", ", parts);
    }
}

/// <summary>The page as it is drawn.</summary>
/// <param name="Rows">The nodes, run by run.</param>
/// <param name="Count">How many nodes there are, or how many of them the search leaves.</param>
/// <param name="Notice">What stands in place of the list when it has no rows.</param>
/// <param name="Error">Why the last thing the user asked for did not work, in the core's words.</param>
/// <param name="Import">What the import button says.</param>
/// <param name="Test">What the test button says.</param>
/// <param name="CanSwitch">Whether a node can be chosen now.</param>
public sealed record ProxiesShown(
    IReadOnlyList<ProxyRow> Rows,
    string Count,
    string? Notice,
    string? Error,
    string Import,
    bool CanImport,
    string Test,
    bool CanTest,
    bool CanSwitch)
{
    /// <summary>The node in use among the rows, or null.</summary>
    public ProxyRow? Selected => Rows.FirstOrDefault(row => row.Selected);
}

/// <summary>
/// The node list as the proxies page needs it. Which nodes a search leaves, how they are split by
/// subscription and what each node's last delay test said is decided by the core, in its <c>proxies</c>
/// command: every window gets the same answer. This reads that answer (<see cref="Read"/>) and turns it into
/// the page's rows and sentences (<see cref="Show"/>), the same way the Linux window does
/// (apps/linux-native/src/proxies.rs).
/// </summary>
/// <param name="Total">How many nodes there are.</param>
/// <param name="Shown">How many of them the search leaves.</param>
public sealed record Proxies(ulong Total, ulong Shown, IReadOnlyList<ProxySection> Sections)
{
    /// <summary>The core's command that answers with the node list.</summary>
    public const string Command = "proxies";

    /// <summary>The arguments of <see cref="Command"/> and of a delay test for a search.</summary>
    public static JsonElement Args(string query) =>
        JsonSerializer.SerializeToElement(new Dictionary<string, string> { ["query"] = query });

    /// <summary>"12 nodes", and "1 node" where the language tells one from many.</summary>
    public static string Count(Strings strings, ulong nodes) =>
        nodes == 1 ? strings["proxies.count.one"] : strings.Format("proxies.count", nodes);

    /// <summary>Reads the core's answer. What is missing reads as "no nodes".</summary>
    public static Proxies Read(JsonElement answer)
    {
        var sections = new List<ProxySection>();
        if (Member(answer, "sections") is { ValueKind: JsonValueKind.Array } listed)
        {
            foreach (var section in listed.EnumerateArray())
            {
                var nodes = new List<ProxyNode>();
                if (Member(section, "nodes") is { ValueKind: JsonValueKind.Array } members)
                {
                    foreach (var node in members.EnumerateArray())
                    {
                        nodes.Add(new ProxyNode(
                            Text(node, "id") ?? "",
                            Text(node, "label") ?? "",
                            Text(node, "protocol") ?? "",
                            Member(node, "selected") is { ValueKind: JsonValueKind.True },
                            ReadDelay(Member(node, "delay"))));
                    }
                }
                sections.Add(new ProxySection(
                    Text(section, "group"),
                    nodes,
                    Member(section, "pasted") is { ValueKind: JsonValueKind.True }));
            }
        }
        return new Proxies(Number(answer, "total") ?? 0, Number(answer, "shown") ?? 0, sections);
    }

    /// <summary>The page for what is known now. <paramref name="proxies"/> is null while the core does not answer.</summary>
    public static ProxiesShown Show(Strings strings, bool coreAnswers, Proxies? proxies, ProxiesBusy busy, string? failure)
    {
        var rows = new List<ProxyRow>();
        var number = 0;
        // A subscription without nodes has no heading either.
        foreach (var section in (proxies?.Sections ?? Array.Empty<ProxySection>()).Where(section => section.Nodes.Count > 0))
        {
            var heading = section.Pasted ? strings["proxies.group.pasted"] : section.Group ?? strings["proxies.group.none"];
            foreach (var node in section.Nodes)
            {
                var (delay, tone) = node.Delay switch
                {
                    { Tested: false } => ("", DelayTone.Plain),
                    { Milliseconds: null } => (strings["proxies.delay.failed"], DelayTone.Failed),
                    { Milliseconds: { } ms, Fast: var fast } => (strings.Format("proxies.delay.ms", ms), fast ? DelayTone.Fast : DelayTone.Slow),
                };
                rows.Add(new ProxyRow(number, heading, node.Id, node.Label, node.Protocol, delay, tone, node.Selected));
            }
            number++;
        }
        var count = "";
        if (proxies is not null)
        {
            count = proxies.Shown == proxies.Total
                ? Count(strings, proxies.Total)
                : strings.Format("proxies.count.shown", proxies.Shown, proxies.Total);
        }
        string? notice = null;
        if (!coreAnswers)
        {
            notice = strings["overview.no_core"];
        }
        else if (proxies is { Total: 0 })
        {
            notice = strings["proxies.empty"];
        }
        else if (proxies is not null && rows.Count == 0)
        {
            notice = strings["proxies.no_match"];
        }
        return new ProxiesShown(
            rows,
            count,
            notice,
            Error: failure,
            Import: strings[busy.Importing ? "proxies.importing" : "proxies.import"],
            CanImport: coreAnswers && !busy.Importing,
            Test: strings[busy.Testing ? "proxies.testing" : "proxies.test"],
            CanTest: coreAnswers && !busy.Testing && rows.Count > 0,
            CanSwitch: coreAnswers && !busy.Switching);
    }

    private static NodeDelay ReadDelay(JsonElement? delay)
    {
        if (delay is not { ValueKind: JsonValueKind.Object } said)
        {
            return NodeDelay.Untested;
        }
        return Number(said, "ms") is { } ms
            ? NodeDelay.Answered(ms, Text(said, "tone") == "fast")
            : NodeDelay.Failed;
    }

    private static JsonElement? Member(JsonElement value, string name) =>
        value.ValueKind == JsonValueKind.Object && value.TryGetProperty(name, out var member) ? member : null;

    private static string? Text(JsonElement value, string name) =>
        Member(value, name) is { ValueKind: JsonValueKind.String } text ? text.GetString() : null;

    private static ulong? Number(JsonElement value, string name) =>
        Member(value, name) is { ValueKind: JsonValueKind.Number } number && number.TryGetUInt64(out var whole) ? whole : null;
}
