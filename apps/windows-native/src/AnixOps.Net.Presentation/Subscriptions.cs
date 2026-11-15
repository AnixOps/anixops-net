using System.Text.Json;

namespace AnixOps.Net.Presentation;

/// <summary>One subscription as the core describes it.</summary>
/// <param name="Pasted">
/// Share links pasted into the client: the row is named in this window's wording, not by
/// <paramref name="Name"/>, which is the core's own name for them.
/// </param>
/// <param name="Refreshable">Whether it is a link that can be downloaded again.</param>
/// <param name="Updated">When it was last downloaded, in the core's words.</param>
/// <param name="Error">Why the last update did not work, in the core's words.</param>
/// <param name="Used">Bytes used.</param>
/// <param name="Total">Bytes allowed.</param>
/// <param name="ExpireDate">The day it expires.</param>
/// <param name="Expired">Whether that day has passed.</param>
public sealed record Subscription(
    string Id,
    string Name,
    bool Pasted = false,
    bool Refreshable = false,
    ulong? Nodes = null,
    string? Updated = null,
    string? Error = null,
    ulong? Used = null,
    ulong? Total = null,
    string? ExpireDate = null,
    bool Expired = false);

/// <summary>What the user asked for on one subscription.</summary>
public enum SubscriptionChangeKind
{
    /// <summary>Download it again.</summary>
    Update,

    /// <summary>Call it by another name.</summary>
    Rename,

    /// <summary>Remove it and its nodes.</summary>
    Remove,
}

/// <summary>Something the list of subscriptions asks of the core.</summary>
/// <param name="Id">The subscription it is about.</param>
/// <param name="Name">The new name, for a rename.</param>
public sealed record SubscriptionChange(SubscriptionChangeKind Kind, string Id, string? Name = null)
{
    /// <summary>The core's command.</summary>
    public string Command => Kind switch
    {
        SubscriptionChangeKind.Update => "update_subscription_source",
        SubscriptionChangeKind.Rename => "rename_subscription",
        _ => "remove_subscription",
    };

    /// <summary>The command's arguments.</summary>
    public JsonElement Args()
    {
        var args = new Dictionary<string, string> { ["id"] = Id };
        if (Kind == SubscriptionChangeKind.Rename)
        {
            args["name"] = Name ?? "";
        }
        return JsonSerializer.SerializeToElement(args);
    }
}

/// <summary>One subscription of the list as it is drawn.</summary>
/// <param name="Title">The name over the row.</param>
/// <param name="Details">What is known about it, in one line: nodes, last update, use, expiry.</param>
/// <param name="Error">Why the last update did not work.</param>
/// <param name="Update">What the update button says; null for a subscription that cannot be downloaded again.</param>
/// <param name="Enabled">Whether anything can be asked of this subscription now.</param>
/// <param name="Name">
/// What the box for a new name starts with: the name as it is, empty for pasted links, which have none of
/// their own.
/// </param>
public sealed record SubscriptionRow(string Id, string Title, string Details, string? Error, string? Update, bool Enabled, string Name);

/// <summary>
/// The list of subscriptions of the proxies page. What a subscription's row says (how much was used, when it
/// expires and whether that has passed) is decided by the core, in its <c>subscriptions</c> command: every
/// window gets the same answer. This reads that answer (<see cref="Read"/>) and turns it into the list's rows
/// (<see cref="Show"/>), the same way the Linux window does (apps/linux-native/src/subscriptions.rs).
/// </summary>
public static class Subscriptions
{
    /// <summary>The core's command that answers with the subscriptions. It takes no arguments.</summary>
    public const string Command = "subscriptions";

    /// <summary>Reads the core's answer. What is missing reads as "no subscriptions".</summary>
    public static IReadOnlyList<Subscription> Read(JsonElement answer)
    {
        var read = new List<Subscription>();
        if (Member(answer, "subscriptions") is { ValueKind: JsonValueKind.Array } listed)
        {
            foreach (var row in listed.EnumerateArray())
            {
                read.Add(new Subscription(
                    Text(row, "id") ?? "",
                    Text(row, "name") ?? "",
                    Flag(row, "pasted"),
                    Flag(row, "refreshable"),
                    Number(row, "nodes"),
                    Text(row, "updated"),
                    Text(row, "error"),
                    Number(row, "used"),
                    Number(row, "total"),
                    Text(row, "expireDate"),
                    Flag(row, "expired")));
            }
        }
        return read;
    }

    /// <summary>The list for what is known now. <paramref name="busy"/> holds the ids of the subscriptions the core is still working on.</summary>
    public static IReadOnlyList<SubscriptionRow> Show(Strings strings, IReadOnlyList<Subscription> subscriptions, IReadOnlyCollection<string> busy)
    {
        var rows = new List<SubscriptionRow>();
        foreach (var subscription in subscriptions)
        {
            var working = busy.Contains(subscription.Id);
            var details = new List<string>();
            if (subscription.Nodes is { } nodes)
            {
                details.Add(strings.Format("proxies.count", nodes));
            }
            if (subscription.Updated is { } updated)
            {
                details.Add(strings.Format("subscriptions.updated", updated));
            }
            if (subscription.Used is { } used)
            {
                details.Add(subscription.Total is { } total
                    ? strings.Format("subscriptions.usage", Overview.Size(used), Overview.Size(total))
                    : strings.Format("subscriptions.usage.used", Overview.Size(used)));
            }
            if (subscription.Expired)
            {
                details.Add(strings["subscriptions.expired"]);
            }
            else if (subscription.ExpireDate is { } day)
            {
                details.Add(strings.Format("subscriptions.expire", day));
            }
            rows.Add(new SubscriptionRow(
                subscription.Id,
                Title: subscription.Pasted ? strings["proxies.group.pasted"] : subscription.Name,
                Details: string.Join(" · ", details),
                Error: subscription.Error is { } error ? strings.Format("subscriptions.error", error) : null,
                Update: subscription.Refreshable ? strings[working ? "subscriptions.updating" : "subscriptions.update"] : null,
                Enabled: !working,
                Name: subscription.Pasted ? "" : subscription.Name));
        }
        return rows;
    }

    private static JsonElement? Member(JsonElement value, string name) =>
        value.ValueKind == JsonValueKind.Object && value.TryGetProperty(name, out var member) ? member : null;

    private static string? Text(JsonElement value, string name) =>
        Member(value, name) is { ValueKind: JsonValueKind.String } text ? text.GetString() : null;

    private static bool Flag(JsonElement value, string name) => Member(value, name) is { ValueKind: JsonValueKind.True };

    private static ulong? Number(JsonElement value, string name) =>
        Member(value, name) is { ValueKind: JsonValueKind.Number } number && number.TryGetUInt64(out var whole) ? whole : null;
}
