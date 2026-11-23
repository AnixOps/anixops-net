using System.Text.Json;
using Xunit;

namespace AnixOps.Net.Presentation.Tests;

/// <summary>
/// The proxies page without a window. The same checks as the Linux window's (apps/linux-native/src/proxies.rs),
/// so that both read the core's answer and draw the list the same way.
/// </summary>
public sealed class ProxiesTests
{
    private static readonly Strings English = Strings.For(Strings.English);
    private static readonly Strings Chinese = Strings.For(Strings.Chinese);
    private static readonly ProxiesBusy Idle = default;

    private static List<JsonElement> Cases()
    {
        var file = Path.Combine(AppContext.BaseDirectory, "fixtures", "view", "cases.json");
        using var document = JsonDocument.Parse(File.ReadAllText(file));
        return document.RootElement.EnumerateArray()
            .Where(sample => sample.GetProperty("command").GetString() == "view.proxies")
            .Select(sample => sample.Clone())
            .ToList();
    }

    private static Proxies Sample(int index) => Proxies.Read(Cases()[index].GetProperty("answer"));

    [Fact]
    public void EverySampleAnswerIsRead()
    {
        var cases = Cases();
        Assert.True(cases.Count >= 6, "the proxies samples are missing");
        foreach (var sample in cases)
        {
            var answer = sample.GetProperty("answer");
            var read = Proxies.Read(answer);
            Assert.Equal(answer.GetProperty("total").GetUInt64(), read.Total);
            Assert.Equal(answer.GetProperty("shown").GetUInt64(), read.Shown);
            var sections = answer.GetProperty("sections").EnumerateArray().ToList();
            Assert.Equal(sections.Count, read.Sections.Count);
            foreach (var (section, readSection) in sections.Zip(read.Sections))
            {
                Assert.Equal(section.GetProperty("group").GetString(), readSection.Group);
                Assert.Equal(section.GetProperty("pasted").GetBoolean(), readSection.Pasted);
                var nodes = section.GetProperty("nodes").EnumerateArray().ToList();
                Assert.Equal(nodes.Count, readSection.Nodes.Count);
                foreach (var (node, readNode) in nodes.Zip(readSection.Nodes))
                {
                    Assert.Equal(node.GetProperty("id").GetString(), readNode.Id);
                    Assert.Equal(node.GetProperty("label").GetString(), readNode.Label);
                    Assert.Equal(node.GetProperty("protocol").GetString(), readNode.Protocol);
                    Assert.Equal(node.GetProperty("selected").GetBoolean(), readNode.Selected);
                    var delay = node.GetProperty("delay");
                    var expected = delay.ValueKind == JsonValueKind.Null
                        ? NodeDelay.Untested
                        : delay.GetProperty("ms").ValueKind == JsonValueKind.Null
                            ? NodeDelay.Failed
                            : NodeDelay.Answered(delay.GetProperty("ms").GetUInt64(), delay.GetProperty("tone").GetString() == "fast");
                    Assert.Equal(expected, readNode.Delay);
                }
            }
        }

        // The first sample: two subscriptions and a node outside them, tested fast, slow and without answer.
        var all = Proxies.Read(cases[0].GetProperty("answer"));
        Assert.Equal(4UL, all.Total);
        Assert.Equal(4UL, all.Shown);
        Assert.Equal(new string?[] { "One", "Two", null }, all.Sections.Select(section => section.Group));
        Assert.Equal(new[] { 2, 1, 1 }, all.Sections.Select(section => section.Nodes.Count));
        Assert.Equal(new ProxyNode("a", "Tokyo", "trojan", true, NodeDelay.Answered(120, true)), all.Sections[0].Nodes[0]);
        Assert.Equal(NodeDelay.Answered(800, false), all.Sections[0].Nodes[1].Delay);
        Assert.Equal(NodeDelay.Failed, all.Sections[1].Nodes[0].Delay);
        Assert.Equal(NodeDelay.Untested, all.Sections[2].Nodes[0].Delay);
    }

    [Theory]
    [InlineData("null")]
    [InlineData("[]")]
    [InlineData("{}")]
    [InlineData("""{ "total": "many", "shown": -1, "sections": {} }""")]
    public void AnAnswerThatSaysNothingReadsAsNoNodes(string answer)
    {
        using var document = JsonDocument.Parse(answer);
        var read = Proxies.Read(document.RootElement);
        Assert.Equal(0UL, read.Total);
        Assert.Equal(0UL, read.Shown);
        Assert.Empty(read.Sections);
    }

    [Fact]
    public void TheListIsDrawnAsNodesUnderTheirSubscriptions()
    {
        var all = Sample(0);
        var page = Proxies.Show(English, true, all, Idle, null);
        Assert.Equal("4 nodes", page.Count);
        Assert.Null(page.Notice);
        Assert.Equal(new ProxyRow(0, "One", "a", "Tokyo", "trojan", "120 ms", DelayTone.Fast, true), page.Rows[0]);
        Assert.Equal(
            new[]
            {
                (0, "One", "120 ms", DelayTone.Fast),
                (0, "One", "800 ms", DelayTone.Slow),
                (1, "Two", "No answer", DelayTone.Failed),
                (2, "Nodes outside subscriptions", "", DelayTone.Plain),
            },
            page.Rows.Select(row => (row.Section, row.Heading, row.Delay, row.Tone)));
        Assert.Same(page.Rows[0], page.Selected);
        Assert.True(page.CanImport);
        Assert.True(page.CanTest);
        Assert.True(page.CanSwitch);
        Assert.Equal("Import", page.Import);
        Assert.Equal("Test latency", page.Test);
        Assert.Equal("trojan, 120 ms, In use", page.Rows[0].Description(English));
        Assert.Equal("vless", page.Rows[3].Description(English));

        page = Proxies.Show(Chinese, true, all, Idle, null);
        Assert.Equal("4 个节点", page.Count);
        Assert.Equal("未归入订阅的节点", page.Rows[3].Heading);
        Assert.Equal("shadowsocks, 超时", page.Rows[2].Description(Chinese));
    }

    [Fact]
    public void SubscriptionsOfOneNameStayApartAndPastedLinksAreNamedInTheWindowsLanguage()
    {
        // Two subscriptions called "One", then links pasted into the client.
        var all = Sample(6);
        var page = Proxies.Show(English, true, all, Idle, null);
        Assert.Equal(
            new[] { (0, "One"), (1, "One"), (2, "Pasted links"), (2, "Pasted links") },
            page.Rows.Select(row => (row.Section, row.Heading)));
        Assert.Equal("粘贴的链接", Proxies.Show(Strings.For(Strings.Chinese), true, all, Idle, null).Rows[3].Heading);
        Assert.Equal("貼上的連結", Proxies.Show(Strings.For(Strings.TraditionalChinese), true, all, Idle, null).Rows[3].Heading);
    }

    [Fact]
    public void ASearchSaysHowManyItLeavesAndWhenItLeavesNone()
    {
        var page = Proxies.Show(English, true, Sample(1), Idle, null);
        Assert.Equal("2 of 4 nodes", page.Count);
        Assert.Null(page.Notice);
        // The node in use is not among the ones the search leaves.
        Assert.Null(page.Selected);
        Assert.Equal(new[] { 0, 1 }, page.Rows.Select(row => row.Section));

        page = Proxies.Show(English, true, Sample(3), Idle, null);
        Assert.Equal("0 of 4 nodes", page.Count);
        Assert.Equal("No node matches the search.", page.Notice);
        Assert.Empty(page.Rows);
        Assert.False(page.CanTest);
        Assert.True(page.CanImport);
    }

    [Fact]
    public void WithoutNodesOrWithoutTheCoreThePageSaysSo()
    {
        var page = Proxies.Show(English, true, new Proxies(0, 0, []), Idle, null);
        Assert.Equal("0 nodes", page.Count);
        Assert.Equal("No nodes yet. Import a subscription and its nodes appear here.", page.Notice);
        Assert.True(page.CanImport);
        Assert.False(page.CanTest);

        page = Proxies.Show(English, false, null, Idle, null);
        Assert.Equal("", page.Count);
        Assert.Equal("The core service is not running, so there is nothing to connect.", page.Notice);
        Assert.False(page.CanImport);
        Assert.False(page.CanTest);
        Assert.False(page.CanSwitch);

        // Answering, but the first answer has not come yet: nothing to say, nothing to test.
        page = Proxies.Show(English, true, null, Idle, null);
        Assert.Null(page.Notice);
        Assert.True(page.CanImport);
        Assert.False(page.CanTest);
    }

    [Fact]
    public void WhatTheCoreIsBusyWithCannotBeAskedAgainAndTheRestCan()
    {
        var all = Sample(0);
        var busy = Idle.With(ProxyActionKind.Import, true);
        var page = Proxies.Show(English, true, all, busy, null);
        Assert.Equal((false, true, true), (page.CanImport, page.CanTest, page.CanSwitch));
        Assert.Equal("Importing", page.Import);

        busy = busy.With(ProxyActionKind.Test, true).With(ProxyActionKind.Import, false);
        page = Proxies.Show(English, true, all, busy, null);
        Assert.Equal((true, false, true), (page.CanImport, page.CanTest, page.CanSwitch));
        Assert.Equal("Import", page.Import);
        Assert.Equal("Testing", page.Test);

        busy = busy.With(ProxyActionKind.Switch, true);
        page = Proxies.Show(English, true, all, busy, null);
        Assert.Equal((true, false, false), (page.CanImport, page.CanTest, page.CanSwitch));
        Assert.Equal(Idle, busy.With(ProxyActionKind.Test, false).With(ProxyActionKind.Switch, false));

        page = Proxies.Show(English, true, all, Idle, "the link gave no nodes");
        Assert.Equal("the link gave no nodes", page.Error);
    }

    [Fact]
    public void OneNodeIsNotMany()
    {
        Assert.Equal("1 node", Proxies.Count(English, 1));
        Assert.Equal("0 nodes", Proxies.Count(English, 0));
        Assert.Equal("12 nodes", Proxies.Count(English, 12));
        Assert.Equal("1 个节点", Proxies.Count(Chinese, 1));
    }

    [Fact]
    public void ActionsNameTheCoresCommands()
    {
        var import = new ProxyAction(ProxyActionKind.Import, "ss://x");
        Assert.Equal("import_subscription", import.Command);
        Assert.Equal("""{"location":"ss://x"}""", import.Args().GetRawText());
        var test = new ProxyAction(ProxyActionKind.Test, "tokyo");
        Assert.Equal("test_proxies", test.Command);
        Assert.Equal("""{"query":"tokyo"}""", test.Args().GetRawText());
        var use = new ProxyAction(ProxyActionKind.Switch, "a");
        Assert.Equal("switch_node", use.Command);
        Assert.Equal("""{"nodeId":"a"}""", use.Args().GetRawText());
        Assert.Equal("""{"query":""}""", Proxies.Args("").GetRawText());
    }

    [Fact]
    public void AFileIsReadByTheWindowAndItsContentSent()
    {
        // The core service opens no path a window names: the window reads the file as the user.
        var dir = Directory.CreateTempSubdirectory("anixops-net-import-");
        try
        {
            var file = Path.Combine(dir.FullName, "nodes.txt");
            File.WriteAllText(file, "ss://x");
            Assert.True(SubscriptionFile.IsPath(file));
            Assert.False(SubscriptionFile.IsPath("https://panel.test/s"));
            Assert.False(SubscriptionFile.IsPath("ss://x"));
            Assert.False(SubscriptionFile.IsPath("  "));
            Assert.Equal("import_subscription_text", SubscriptionFile.Command);
            Assert.Equal("""{"name":"nodes.txt","content":"ss://x"}""", SubscriptionFile.Args($" {file} ", English).GetRawText());

            // A folder, a missing file and one past the limit say why, in the window's language.
            var folder = Assert.Throws<IOException>(() => SubscriptionFile.Args(dir.FullName, English));
            Assert.Equal($"{dir.FullName} is not a file.", folder.Message);
            var missing = Path.Combine(dir.FullName, "missing.txt");
            Assert.StartsWith($"{missing} could not be read:", Assert.Throws<IOException>(() => SubscriptionFile.Args(missing, English)).Message);
            var big = Path.Combine(dir.FullName, "big.txt");
            using (var stream = File.Create(big))
            {
                stream.SetLength(SubscriptionFile.MaxBytes + 1);
            }
            Assert.Contains("larger than 16 MiB", Assert.Throws<IOException>(() => SubscriptionFile.Args(big, English)).Message);
        }
        finally
        {
            dir.Delete(true);
        }
    }
}
