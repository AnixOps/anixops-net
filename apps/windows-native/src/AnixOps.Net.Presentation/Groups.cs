using System.Text.Json;

namespace AnixOps.Net.Presentation;

/// <summary>One member of a policy group.</summary>
/// <param name="Id">What the core is told when the member is chosen.</param>
public sealed record GroupMember(string Id, string Label);

/// <summary>How a group comes to the member it uses.</summary>
public enum GroupKind
{
    /// <summary>A kind this window does not know.</summary>
    Other,

    /// <summary>The user chooses.</summary>
    Select,

    /// <summary>The fastest member.</summary>
    Auto,

    /// <summary>The first member that answers.</summary>
    Fallback,
}

/// <summary>One policy group as the core describes it.</summary>
/// <param name="Choosable">Whether the user chooses the member.</param>
/// <param name="Selected">Which of the members is in use, if the core says.</param>
public sealed record Group(string Tag, GroupKind Kind, bool Choosable, IReadOnlyList<GroupMember> Members, int? Selected)
{
    public bool Equals(Group? other) =>
        other is not null
        && Tag == other.Tag
        && Kind == other.Kind
        && Choosable == other.Choosable
        && Selected == other.Selected
        && Members.SequenceEqual(other.Members);

    public override int GetHashCode() => HashCode.Combine(Tag, Kind, Choosable, Selected, Members.Count);
}

/// <summary>The user chose a member for a group.</summary>
/// <param name="Group">The group's name.</param>
/// <param name="Member">The member's id.</param>
public sealed record GroupChoice(string Group, string Member)
{
    /// <summary>The core's command.</summary>
    public const string Command = "select_native_group_outbound";

    /// <summary>The command's arguments.</summary>
    public JsonElement Args() =>
        JsonSerializer.SerializeToElement(new Dictionary<string, string> { ["groupTag"] = Group, ["outboundTag"] = Member });
}

/// <summary>One group of the list as it is drawn.</summary>
/// <param name="Tag">The group's name, over the row.</param>
/// <param name="Details">How the group chooses and what it uses, in one line.</param>
/// <param name="Options">What can be chosen, in the subscription's order; empty for a group the user does not choose in.</param>
/// <param name="Chosen">Which of the options is in use.</param>
/// <param name="Enabled">Whether a member can be chosen now.</param>
public sealed record GroupRow(string Tag, string Details, IReadOnlyList<GroupMember> Options, int? Chosen, bool Enabled)
{
    public bool Equals(GroupRow? other) =>
        other is not null
        && Tag == other.Tag
        && Details == other.Details
        && Chosen == other.Chosen
        && Enabled == other.Enabled
        && Options.SequenceEqual(other.Options);

    public override int GetHashCode() => HashCode.Combine(Tag, Details, Chosen, Enabled, Options.Count);

    /// <summary>Whether this is the same group offering the same members as <paramref name="other"/>.</summary>
    public bool SameShape(GroupRow other) => Tag == other.Tag && Options.SequenceEqual(other.Options);
}

/// <summary>
/// The policy groups of the proxies page. Which groups there are, what kind each is and which member it uses
/// is decided by the core, in its <c>groups</c> command: every window gets the same answer. This reads that
/// answer (<see cref="Read"/>) and turns it into the list's rows (<see cref="Show"/>), the same way the Linux
/// window does (apps/linux-native/src/groups.rs).
/// </summary>
public static class Groups
{
    /// <summary>The core's command that answers with the policy groups. It takes no arguments.</summary>
    public const string Command = "groups";

    /// <summary>Reads the core's answer. What is missing reads as "no groups".</summary>
    public static IReadOnlyList<Group> Read(JsonElement answer)
    {
        var read = new List<Group>();
        if (Member(answer, "groups") is { ValueKind: JsonValueKind.Array } listed)
        {
            foreach (var row in listed.EnumerateArray())
            {
                var members = new List<GroupMember>();
                int? selected = null;
                if (Member(row, "members") is { ValueKind: JsonValueKind.Array } listedMembers)
                {
                    foreach (var member in listedMembers.EnumerateArray())
                    {
                        if (selected is null && Flag(member, "selected"))
                        {
                            selected = members.Count;
                        }
                        members.Add(new GroupMember(Text(member, "id") ?? "", Text(member, "label") ?? ""));
                    }
                }
                read.Add(new Group(
                    Text(row, "tag") ?? "",
                    Text(row, "kind") switch
                    {
                        "select" => GroupKind.Select,
                        "auto" => GroupKind.Auto,
                        "fallback" => GroupKind.Fallback,
                        _ => GroupKind.Other,
                    },
                    Flag(row, "choosable"),
                    members,
                    selected));
            }
        }
        return read;
    }

    /// <summary>The list for what is known now. <paramref name="busy"/> holds the names of the groups the core is still changing.</summary>
    public static IReadOnlyList<GroupRow> Show(Strings strings, IReadOnlyList<Group> groups, IReadOnlyCollection<string> busy)
    {
        var rows = new List<GroupRow>();
        foreach (var group in groups)
        {
            var details = new List<string>();
            switch (group.Kind)
            {
                case GroupKind.Select:
                    details.Add(strings["groups.kind.select"]);
                    break;
                case GroupKind.Auto:
                    details.Add(strings["groups.kind.auto"]);
                    break;
                case GroupKind.Fallback:
                    details.Add(strings["groups.kind.fallback"]);
                    break;
            }
            details.Add(group.Selected is { } at && at < group.Members.Count
                ? strings.Format("groups.using", group.Members[at].Label)
                : strings["groups.undecided"]);
            rows.Add(new GroupRow(
                group.Tag,
                string.Join(" · ", details),
                group.Choosable ? group.Members : [],
                group.Choosable ? group.Selected : null,
                group.Choosable && !busy.Contains(group.Tag)));
        }
        return rows;
    }

    private static JsonElement? Member(JsonElement value, string name) =>
        value.ValueKind == JsonValueKind.Object && value.TryGetProperty(name, out var member) ? member : null;

    private static string? Text(JsonElement value, string name) =>
        Member(value, name) is { ValueKind: JsonValueKind.String } text ? text.GetString() : null;

    private static bool Flag(JsonElement value, string name) => Member(value, name) is { ValueKind: JsonValueKind.True };
}
