using System.Globalization;
using System.Text.Json;

namespace AnixOps.Net.Presentation;

/// <summary>Why a node cannot be removed.</summary>
public enum NodeKeep
{
    /// <summary>It is a node of a subscription that is not the one in use.</summary>
    Other,

    /// <summary>It is the only node left.</summary>
    Last,

    /// <summary>The subscription's policy groups or rules name it.</summary>
    Used,
}

/// <summary>One node as the core describes it.</summary>
/// <param name="Editable">Whether its name, server and port can be changed.</param>
/// <param name="Keep">Why it cannot be removed; null when it can.</param>
/// <param name="Link">The node as a share link, if it has one.</param>
/// <param name="Qr">That link as a QR code: its rows from the top, <c>#</c> for a dark module.</param>
public sealed record NodeDetails(
    string Id,
    string Name,
    string Protocol,
    string Server,
    ulong Port,
    bool Editable,
    NodeKeep? Keep,
    string? Link,
    IReadOnlyList<string>? Qr)
{
    public bool Equals(NodeDetails? other) =>
        other is not null
        && Id == other.Id
        && Name == other.Name
        && Protocol == other.Protocol
        && Server == other.Server
        && Port == other.Port
        && Editable == other.Editable
        && Keep == other.Keep
        && Link == other.Link
        && Node.SameRows(Qr, other.Qr);

    public override int GetHashCode() => HashCode.Combine(Id, Name, Protocol, Server, Port, Editable, Keep, Link);
}

/// <summary>What the user typed for a node.</summary>
public sealed record NodeDraft(string Name, string Server, string Port)
{
    /// <summary>The core's command that says whether this can be saved.</summary>
    public const string CheckCommand = "check_node";

    /// <summary>The core's command that saves this for a node.</summary>
    public const string SaveCommand = "edit_node";

    /// <summary>The arguments of <see cref="CheckCommand"/>.</summary>
    public JsonElement CheckArgs() =>
        JsonSerializer.SerializeToElement(new Dictionary<string, string> { ["name"] = Name, ["server"] = Server, ["port"] = Port });

    /// <summary>The arguments of <see cref="SaveCommand"/> for the node <paramref name="id"/>.</summary>
    public JsonElement SaveArgs(string id) =>
        JsonSerializer.SerializeToElement(
            new Dictionary<string, string> { ["nodeId"] = id, ["name"] = Name, ["server"] = Server, ["port"] = Port });
}

/// <summary>The node's dialog as it is drawn.</summary>
/// <param name="Title">The node's name, over the dialog.</param>
/// <param name="Protocol">The node's protocol, under the name.</param>
/// <param name="Draft">What the three boxes hold when nothing was typed yet.</param>
/// <param name="Editable">Whether the boxes can be typed in.</param>
/// <param name="Note">Under the boxes: how long a change lasts, or why nothing can be changed.</param>
/// <param name="Problem">Why the last save or removal did not work.</param>
/// <param name="CanSave">Whether "save" can be pressed now.</param>
/// <param name="CanRemove">Whether "remove" can be pressed now.</param>
/// <param name="Keep">Why the node cannot be removed, for a node that can be changed.</param>
/// <param name="Link">The share link.</param>
/// <param name="Qr">The share link's QR code.</param>
public sealed record NodeShown(
    string Id,
    string Title,
    string Protocol,
    NodeDraft Draft,
    bool Editable,
    string Note,
    string? Problem,
    bool CanSave,
    bool CanRemove,
    string? Keep,
    string? Link,
    IReadOnlyList<string>? Qr)
{
    public bool Equals(NodeShown? other) =>
        other is not null
        && Id == other.Id
        && Title == other.Title
        && Protocol == other.Protocol
        && Draft == other.Draft
        && Editable == other.Editable
        && Note == other.Note
        && Problem == other.Problem
        && CanSave == other.CanSave
        && CanRemove == other.CanRemove
        && Keep == other.Keep
        && Link == other.Link
        && Node.SameRows(Qr, other.Qr);

    public override int GetHashCode() => HashCode.Combine(Id, Title, Protocol, Draft, Editable, Note, Problem, Link);
}

/// <summary>
/// One node's dialog. Whether a node can be changed or removed, and its share link and QR code, are decided
/// by the core (its <c>node_details</c> command); whether what the user typed can be saved is decided by the
/// core too (<c>check_node</c>). This reads those answers (<see cref="Read"/>, <see cref="Problem"/>) and
/// turns them into the dialog's sentences (<see cref="Show"/>), the same way the Linux window does
/// (apps/linux-native/src/node.rs).
/// </summary>
public static class Node
{
    /// <summary>The core's command that answers with one node's details.</summary>
    public const string Command = "node_details";

    /// <summary>The core's command that removes a node; its arguments are <see cref="Args"/>.</summary>
    public const string Remove = "remove_node";

    /// <summary>How many modules of white stand around a QR code.</summary>
    private const int Quiet = 4;

    /// <summary>The arguments of <see cref="Command"/> and of <see cref="Remove"/>.</summary>
    public static JsonElement Args(string id) =>
        JsonSerializer.SerializeToElement(new Dictionary<string, string> { ["nodeId"] = id });

    /// <summary>
    /// Reads the core's answer. What is missing reads as empty; a reason this window does not know keeps the
    /// node from being removed all the same.
    /// </summary>
    public static NodeDetails Read(JsonElement answer)
    {
        List<string>? qr = null;
        if (Member(answer, "qr") is { ValueKind: JsonValueKind.Array } rows)
        {
            qr = rows.EnumerateArray()
                .Where(row => row.ValueKind == JsonValueKind.String)
                .Select(row => row.GetString() ?? "")
                .ToList();
            if (qr.Count == 0)
            {
                qr = null;
            }
        }
        var link = Text(answer, "link");
        return new NodeDetails(
            Text(answer, "id") ?? "",
            Text(answer, "name") ?? "",
            Text(answer, "protocol") ?? "",
            Text(answer, "server") ?? "",
            Member(answer, "port") is { ValueKind: JsonValueKind.Number } port && port.TryGetUInt64(out var number) ? number : 0,
            Member(answer, "editable") is { ValueKind: JsonValueKind.True },
            Text(answer, "keep") switch
            {
                null => null,
                "last" => NodeKeep.Last,
                "used" => NodeKeep.Used,
                _ => NodeKeep.Other,
            },
            string.IsNullOrEmpty(link) ? null : link,
            qr);
    }

    /// <summary>
    /// Why the core will not save a draft, in the window's language; null when it will.
    /// <paramref name="answer"/> is what the core said to <see cref="NodeDraft.CheckCommand"/>.
    /// </summary>
    public static string? Problem(Strings strings, JsonElement answer) =>
        Text(answer, "problem") switch
        {
            null => null,
            "server.empty" => strings["node.problem.server.empty"],
            "server.invalid" => strings["node.problem.server.invalid"],
            _ => strings["node.problem.port"],
        };

    /// <summary>
    /// The dialog for what is known now. <paramref name="busy"/> says that the core is saving or removing
    /// the node; <paramref name="problem"/> is why the last save or removal did not work.
    /// </summary>
    public static NodeShown Show(Strings strings, NodeDetails details, bool busy, string? problem) =>
        new(
            details.Id,
            details.Name,
            details.Protocol,
            new NodeDraft(details.Name, details.Server, details.Port.ToString(CultureInfo.InvariantCulture)),
            details.Editable,
            strings[details.Editable ? "node.edit.note" : "node.readonly"],
            problem,
            details.Editable && !busy,
            details.Editable && details.Keep is null && !busy,
            details switch
            {
                { Editable: true, Keep: NodeKeep.Last } => strings["node.keep.last"],
                { Editable: true, Keep: NodeKeep.Used } => strings["node.keep.used"],
                _ => null,
            },
            details.Link,
            details.Qr);

    /// <summary>
    /// A QR code's picture: <c>Side</c> pixels square, four bytes (blue, green, red, opacity) per pixel,
    /// dark modules black on white with a quiet zone of four modules around them. <paramref name="scale"/>
    /// is how many pixels a module is wide.
    /// </summary>
    public static (int Side, byte[] Pixels) Picture(IReadOnlyList<string> rows, int scale)
    {
        var side = (rows.Count + 2 * Quiet) * scale;
        var pixels = new byte[side * side * 4];
        Array.Fill(pixels, (byte)0xff);
        for (var y = 0; y < rows.Count; y++)
        {
            var row = rows[y];
            for (var x = 0; x < row.Length && x < rows.Count; x++)
            {
                if (row[x] != '#')
                {
                    continue;
                }
                for (var dy = 0; dy < scale; dy++)
                {
                    var from = ((((y + Quiet) * scale) + dy) * side + ((x + Quiet) * scale)) * 4;
                    for (var dx = 0; dx < scale; dx++)
                    {
                        // Black, and as opaque as the white around it.
                        pixels[from + (dx * 4)] = 0;
                        pixels[from + (dx * 4) + 1] = 0;
                        pixels[from + (dx * 4) + 2] = 0;
                    }
                }
            }
        }
        return (side, pixels);
    }

    internal static bool SameRows(IReadOnlyList<string>? one, IReadOnlyList<string>? other) =>
        one is null ? other is null : other is not null && one.SequenceEqual(other);

    private static JsonElement? Member(JsonElement value, string name) =>
        value.ValueKind == JsonValueKind.Object && value.TryGetProperty(name, out var member) ? member : null;

    private static string? Text(JsonElement value, string name) =>
        Member(value, name) is { ValueKind: JsonValueKind.String } text ? text.GetString() : null;
}
