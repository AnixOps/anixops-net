using System.Text.Json;
using Xunit;

namespace AnixOps.Net.Presentation.Tests;

/// <summary>
/// The policy groups without a window. The same checks as the Linux window's
/// (apps/linux-native/src/groups.rs), so that both read the core's answer and draw the rows the same way.
/// </summary>
public sealed class GroupsTests
{
    private static readonly Strings English = Strings.For(Strings.English);
    private static readonly Strings Chinese = Strings.For(Strings.Chinese);

    private static List<JsonElement> Cases()
    {
        var file = Path.Combine(AppContext.BaseDirectory, "fixtures", "view", "cases.json");
        using var document = JsonDocument.Parse(File.ReadAllText(file));
        return document.RootElement.EnumerateArray()
            .Where(sample => sample.GetProperty("command").GetString() == "view.groups")
            .Select(sample => sample.Clone())
            .ToList();
    }

    private static IReadOnlyList<Group> Sample(int index) => Groups.Read(Cases()[index].GetProperty("answer"));

    [Fact]
    public void EverySampleAnswerIsRead()
    {
        var cases = Cases();
        Assert.True(cases.Count >= 3, "the policy group samples are missing");
        foreach (var sample in cases)
        {
            var answer = sample.GetProperty("answer");
            var rows = answer.GetProperty("groups").EnumerateArray().ToList();
            var read = Groups.Read(answer);
            Assert.Equal(rows.Count, read.Count);
            foreach (var (row, group) in rows.Zip(read))
            {
                Assert.Equal(row.GetProperty("tag").GetString(), group.Tag);
                Assert.Equal(row.GetProperty("choosable").GetBoolean(), group.Choosable);
                var members = row.GetProperty("members").EnumerateArray().ToList();
                Assert.Equal(members.Count, group.Members.Count);
                foreach (var (member, one) in members.Zip(group.Members))
                {
                    Assert.Equal(member.GetProperty("id").GetString(), one.Id);
                    Assert.Equal(member.GetProperty("label").GetString(), one.Label);
                }
                // The member the answer marks is the one it names.
                var named = row.GetProperty("selected");
                Assert.Equal(
                    named.ValueKind == JsonValueKind.String ? named.GetString() : null,
                    group.Selected is { } at ? group.Members[at].Id : null);
            }
        }
        Assert.Equal(
            new[] { GroupKind.Select, GroupKind.Auto, GroupKind.Fallback },
            Sample(0).Select(group => group.Kind).ToArray());
        Assert.Equal(GroupKind.Other, Sample(1)[0].Kind);
    }

    [Theory]
    [InlineData("null")]
    [InlineData("{}")]
    [InlineData("[]")]
    [InlineData("""{ "groups": "many" }""")]
    public void AnAnswerThatSaysNothingReadsAsNoGroups(string answer)
    {
        using var document = JsonDocument.Parse(answer);
        Assert.Empty(Groups.Read(document.RootElement));
    }

    [Fact]
    public void AGroupThatSaysNothingIsStillAGroup()
    {
        using var document = JsonDocument.Parse("""{ "groups": [{}] }""");
        Assert.Equal(new[] { new Group("", GroupKind.Other, false, [], null) }, Groups.Read(document.RootElement).ToArray());
    }

    [Fact]
    public void ARowSaysHowTheGroupChoosesAndWhatItUses()
    {
        var groups = Sample(0);
        var rows = Groups.Show(English, groups, []);
        Assert.Equal("Proxy", rows[0].Tag);
        Assert.Equal("Manual · Using Osaka", rows[0].Details);
        Assert.Equal(
            new[]
            {
                new GroupMember("Auto", "Auto"),
                new GroupMember("n1", "Tokyo"),
                new GroupMember("n2", "Osaka"),
                new GroupMember("DIRECT", "DIRECT"),
            },
            rows[0].Options.ToArray());
        Assert.Equal(2, rows[0].Chosen);
        Assert.True(rows[0].Enabled);
        // A group that chooses by itself offers nothing, and says so when it has not chosen yet.
        Assert.Equal("Fastest · Not chosen yet", rows[1].Details);
        Assert.Empty(rows[1].Options);
        Assert.Null(rows[1].Chosen);
        Assert.False(rows[1].Enabled);
        Assert.Equal("Fallback · Using Tokyo", rows[2].Details);
        Assert.False(rows[2].Enabled);
        // A kind this window does not know is not named.
        Assert.Equal("Not chosen yet", Groups.Show(English, Sample(1), [])[0].Details);
        Assert.Equal("手动选择 · 正在用 Osaka", Groups.Show(Chinese, groups, [])[0].Details);
    }

    [Fact]
    public void AGroupTheCoreIsChangingCannotBeAskedAgain()
    {
        var rows = Groups.Show(English, Sample(0), ["Proxy"]);
        Assert.False(rows[0].Enabled);
        Assert.Equal(2, rows[0].Chosen);
    }

    [Fact]
    public void RowsAreEqualWhenTheySayTheSame()
    {
        Assert.Equal(Groups.Show(English, Sample(0), []).ToArray(), Groups.Show(English, Sample(0), []).ToArray());
        Assert.NotEqual(Groups.Show(English, Sample(0), [])[0], Groups.Show(English, Sample(0), ["Proxy"])[0]);
        Assert.True(Groups.Show(English, Sample(0), [])[0].SameShape(Groups.Show(Chinese, Sample(0), ["Proxy"])[0]));
    }

    [Fact]
    public void AChoiceNamesTheCoresCommand()
    {
        var choice = new GroupChoice("Proxy", "n1");
        Assert.Equal("select_native_group_outbound", GroupChoice.Command);
        Assert.Equal("""{"groupTag":"Proxy","outboundTag":"n1"}""", choice.Args().GetRawText());
    }
}
