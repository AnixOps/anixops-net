using System.Text.Json;
using Xunit;

namespace AnixOps.Net.Presentation.Tests;

/// <summary>
/// The list of subscriptions without a window. The same checks as the Linux window's
/// (apps/linux-native/src/subscriptions.rs), so that both read the core's answer and draw the rows the same
/// way.
/// </summary>
public sealed class SubscriptionsTests
{
    private static readonly Strings English = Strings.For(Strings.English);
    private static readonly Strings Chinese = Strings.For(Strings.Chinese);

    private static List<JsonElement> Cases()
    {
        var file = Path.Combine(AppContext.BaseDirectory, "fixtures", "view", "cases.json");
        using var document = JsonDocument.Parse(File.ReadAllText(file));
        return document.RootElement.EnumerateArray()
            .Where(sample => sample.GetProperty("command").GetString() == "view.subscriptions")
            .Select(sample => sample.Clone())
            .ToList();
    }

    private static IReadOnlyList<Subscription> Sample(int index) => Subscriptions.Read(Cases()[index].GetProperty("answer"));

    private static string? Text(JsonElement row, string name) =>
        row.GetProperty(name).ValueKind == JsonValueKind.String ? row.GetProperty(name).GetString() : null;

    private static ulong? Number(JsonElement row, string name) =>
        row.GetProperty(name).ValueKind == JsonValueKind.Number ? row.GetProperty(name).GetUInt64() : null;

    [Fact]
    public void EverySampleAnswerIsRead()
    {
        var cases = Cases();
        Assert.True(cases.Count >= 3, "the subscription samples are missing");
        foreach (var sample in cases)
        {
            var answer = sample.GetProperty("answer");
            var rows = answer.GetProperty("subscriptions").EnumerateArray().ToList();
            var read = Subscriptions.Read(answer);
            Assert.Equal(rows.Count, read.Count);
            foreach (var (row, subscription) in rows.Zip(read))
            {
                Assert.Equal(row.GetProperty("id").GetString(), subscription.Id);
                Assert.Equal(row.GetProperty("name").GetString(), subscription.Name);
                Assert.Equal(row.GetProperty("pasted").GetBoolean(), subscription.Pasted);
                Assert.Equal(row.GetProperty("refreshable").GetBoolean(), subscription.Refreshable);
                Assert.Equal(Number(row, "nodes"), subscription.Nodes);
                Assert.Equal(Text(row, "updated"), subscription.Updated);
                Assert.Equal(Text(row, "error"), subscription.Error);
                Assert.Equal(Number(row, "used"), subscription.Used);
                Assert.Equal(Number(row, "total"), subscription.Total);
                Assert.Equal(Text(row, "expireDate"), subscription.ExpireDate);
                Assert.Equal(row.GetProperty("expired").GetBoolean(), subscription.Expired);
            }
        }
    }

    [Theory]
    [InlineData("null")]
    [InlineData("{}")]
    [InlineData("[]")]
    [InlineData("""{ "subscriptions": "many" }""")]
    public void AnAnswerThatSaysNothingReadsAsNoSubscriptions(string answer)
    {
        using var document = JsonDocument.Parse(answer);
        Assert.Empty(Subscriptions.Read(document.RootElement));
    }

    [Fact]
    public void ARowThatSaysNothingIsStillARow()
    {
        using var document = JsonDocument.Parse("""{ "subscriptions": [{}] }""");
        Assert.Equal(new[] { new Subscription("", "") }, Subscriptions.Read(document.RootElement));
    }

    [Fact]
    public void ARowSaysWhatIsKnownInOneLine()
    {
        var before = Sample(0);
        var rows = Subscriptions.Show(English, before, []);
        Assert.Equal(
            new SubscriptionRow(
                "s1",
                "One",
                "12 nodes · Updated 2026-11-15 12:00 · 1000 B of 4.9 KB used · Expires 2033-05-18",
                Error: null,
                Update: "Update",
                Enabled: true,
                Name: "One"),
            rows[0]);
        // Nothing known but why the last update did not work.
        Assert.Equal("other.example", rows[1].Title);
        Assert.Equal("", rows[1].Details);
        Assert.Equal("The last update did not work: the server answered 403", rows[1].Error);
        // Pasted links: this window's wording for the name, nothing to download again, and no name of
        // their own to start a new one from.
        Assert.Equal("Pasted links", rows[2].Title);
        Assert.Equal("2 nodes · Updated 2026-11-15 12:02 · 7 B used", rows[2].Details);
        Assert.Null(rows[2].Update);
        Assert.Equal("", rows[2].Name);
        Assert.Equal("粘贴的链接", Subscriptions.Show(Chinese, before, [])[2].Title);

        // The same subscription one second later: the day has come.
        var after = Sample(1);
        Assert.Equal(
            "12 个节点 · 更新于 2026-11-15 12:00 · 已用 1000 B / 4.9 KB · 已到期",
            Subscriptions.Show(Chinese, after, [])[0].Details);
    }

    [Fact]
    public void ASubscriptionTheCoreIsWorkingOnCannotBeAskedAgain()
    {
        var rows = Subscriptions.Show(English, Sample(0), ["s1"]);
        Assert.Equal("Updating", rows[0].Update);
        Assert.False(rows[0].Enabled);
        // The others do not wait for it.
        Assert.Equal("Update", rows[1].Update);
        Assert.True(rows[1].Enabled);
        Assert.True(rows[2].Enabled);
    }

    [Fact]
    public void ChangesNameTheCoresCommands()
    {
        var update = new SubscriptionChange(SubscriptionChangeKind.Update, "s1");
        Assert.Equal("update_subscription_source", update.Command);
        Assert.Equal("""{"id":"s1"}""", update.Args().GetRawText());
        var rename = new SubscriptionChange(SubscriptionChangeKind.Rename, "s1", "Work");
        Assert.Equal("rename_subscription", rename.Command);
        Assert.Equal("""{"id":"s1","name":"Work"}""", rename.Args().GetRawText());
        var remove = new SubscriptionChange(SubscriptionChangeKind.Remove, "s1");
        Assert.Equal("remove_subscription", remove.Command);
        Assert.Equal("""{"id":"s1"}""", remove.Args().GetRawText());
    }
}
