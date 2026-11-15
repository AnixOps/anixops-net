using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Automation;
using AnixOps.Net.Presentation;
using AnixOps.Net.Testing;
using Xunit;

namespace AnixOps.Net.App.UiTests;

/// <summary>The proxies page: it imports links, lists the core's nodes, searches them, switches and tests them.</summary>
public sealed class ProxiesTests
{
    private const string Link = "Proxies.Link";
    private const string Import = "Proxies.Import";
    private const string Search = "Proxies.Search";
    private const string Test = "Proxies.Test";
    private const string Count = "Proxies.Count";
    private const string Nodes = "Proxies.List";
    private const string Notice = "Proxies.Notice";

    // Addresses kept for documentation (RFC 5737): nothing answers there, so a delay test ends in "no answer".
    private const string FirstNode = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@203.0.113.9:11111#Test";
    private const string SecondNode = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@203.0.113.10:11111#Second";

    private static readonly TimeSpan Patience = TimeSpan.FromSeconds(90);

    // Every node of the test is waited for until its test gives up.
    private static readonly TimeSpan TestPatience = TimeSpan.FromMinutes(4);

    private static void Wait(Func<bool> done, Func<string> otherwise, TimeSpan? patience = null)
    {
        var deadline = DateTime.UtcNow + (patience ?? Patience);
        while (!done())
        {
            Assert.True(DateTime.UtcNow < deadline, otherwise());
            Thread.Sleep(50);
        }
    }

    /// <summary>The rows of the node list that exist as controls, in order.</summary>
    private static List<AutomationElement> Items(AppSession app) =>
        app.Find(Nodes)
            .FindAll(TreeScope.Descendants, new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.ListItem))
            .Cast<AutomationElement>()
            .ToList();

    private static List<string> Labels(AppSession app)
    {
        try
        {
            return Items(app).Select(item => item.Current.Name).ToList();
        }
        catch (ElementNotAvailableException)
        {
            // The list changed while it was read.
            return [];
        }
    }

    private static void Press(AppSession app, string automationId)
    {
        var button = app.Find(automationId);
        Assert.True(button.TryGetCurrentPattern(InvokePattern.Pattern, out var invoke), $"'{automationId}' cannot be pressed");
        ((InvokePattern)invoke).Invoke();
    }

    private static void WaitForEnabled(AppSession app, string automationId)
    {
        Wait(() => app.Find(automationId).Current.IsEnabled, () => $"'{automationId}' did not become enabled");
    }

    /// <summary>The core's nodes as (id, label, in use), in the order of the list.</summary>
    private static async Task<List<(string Id, string Label, bool Selected)>> NodesAsync(SecondWindow second)
    {
        var answer = await second.AskAsync(Proxies.Command);
        var nodes = new List<(string, string, bool)>();
        foreach (var section in answer.GetProperty("sections").EnumerateArray())
        {
            foreach (var node in section.GetProperty("nodes").EnumerateArray())
            {
                nodes.Add((
                    node.GetProperty("id").GetString() ?? "",
                    node.GetProperty("label").GetString() ?? "",
                    node.GetProperty("selected").ValueKind == JsonValueKind.True));
            }
        }
        return nodes;
    }

    [Theory]
    [InlineData(Strings.Chinese)]
    [InlineData(Strings.English)]
    public void WithoutTheServiceThereIsNothingToImportInto(string language)
    {
        var strings = Strings.For(language);
        // A pipe nobody listens on, so that a service installed on this machine is not found instead.
        using var app = AppSession.Start(
            "--language", language, "--page", Sections.Proxies, "--pipe", "AnixOpsNet.Test." + Guid.NewGuid().ToString("N"));
        app.WaitForName(Notice, strings["overview.no_core"]);
        Assert.Equal(strings["nav.proxies"], app.Find("Page.Title").Current.Name);
        Assert.Equal(strings["proxies.import"], app.Find(Import).Current.Name);
        Assert.False(app.Find(Import).Current.IsEnabled);
        Assert.Equal(strings["proxies.test"], app.Find(Test).Current.Name);
        Assert.False(app.Find(Test).Current.IsEnabled);
        Assert.Equal(strings["proxies.import.link"], app.Find(Link).Current.Name);
        Assert.Equal(strings["proxies.search"], app.Find(Search).Current.Name);
    }

    [Theory]
    [InlineData(Strings.Chinese, "light")]
    [InlineData(Strings.English, "dark")]
    public async Task NodesAreImportedSearchedSwitchedAndTestedThroughTheService(string language, string theme)
    {
        if (!ServiceProcess.Available())
        {
            return;
        }
        var strings = Strings.For(language);
        using var service = ServiceProcess.Start();
        using var app = AppSession.Start(
            "--language", language, "--theme", theme, "--page", Sections.Proxies, "--pipe", service.PipeName);

        // A new installation: no nodes, and nothing to test.
        app.WaitForName(Notice, strings["proxies.empty"]);
        WaitForEnabled(app, Import);
        Assert.False(app.Find(Test).Current.IsEnabled);

        // What a switch may set off writes the second window's settings, not the machine's real ones.
        await using var second = await SecondWindow.JoinAsync(service.PipeName);

        // A link typed into the page and imported with its button.
        app.SetText(Link, FirstNode);
        Press(app, Import);
        Wait(() => Labels(app).SequenceEqual(["Test"]), () => $"the list holds [{string.Join(", ", Labels(app))}], expected the imported node");
        app.WaitForName(Count, strings.Format("proxies.count", 1));
        // The box is emptied once the link is in.
        Wait(
            () => ((ValuePattern)app.Find(Link).GetCurrentPattern(ValuePattern.Pattern)).Current.Value.Length == 0,
            () => "the link stayed in its box after the import");
        WaitForEnabled(app, Test);

        // What another window of the user imports shows up here too.
        await second.AskAsync("import_subscription", JsonSerializer.Serialize(new { location = FirstNode + "\n" + SecondNode }));
        var nodes = await NodesAsync(second);
        Assert.Contains(nodes, node => node.Label == "Second");
        Wait(
            () => Labels(app).SequenceEqual(nodes.Select(node => node.Label)),
            () => $"the list holds [{string.Join(", ", Labels(app))}], expected [{string.Join(", ", nodes.Select(node => node.Label))}]");
        app.WaitForName(Count, strings.Format("proxies.count", nodes.Count));

        // The search is the core's: the page shows what it leaves, and says how many of how many.
        app.SetText(Search, "second");
        Wait(() => Labels(app).SequenceEqual(["Second"]), () => $"the search left [{string.Join(", ", Labels(app))}]");
        app.WaitForName(Count, strings.Format("proxies.count.shown", 1, nodes.Count));
        app.SetText(Search, "no such node");
        app.WaitForName(Notice, strings["proxies.no_match"]);
        Assert.False(app.Find(Test).Current.IsEnabled);
        app.SetText(Search, "");
        Wait(() => Labels(app).Count == nodes.Count, () => $"clearing the search left [{string.Join(", ", Labels(app))}]");

        // The selected row is the node in use, and selecting another row uses that node.
        var place = nodes.FindIndex(node => !node.Selected);
        Assert.True(place >= 0, "every node is in use");
        var target = nodes[place];
        var row = Items(app)[place];
        Assert.Equal(target.Label, row.Current.Name);
        Assert.True(row.TryGetCurrentPattern(SelectionItemPattern.Pattern, out var selection), "a node cannot be selected");
        ((SelectionItemPattern)selection).Select();
        var deadline = DateTime.UtcNow + Patience;
        while (true)
        {
            var inUse = (await NodesAsync(second)).Where(node => node.Selected).Select(node => node.Id).ToList();
            if (inUse.SequenceEqual([target.Id]))
            {
                break;
            }
            Assert.True(DateTime.UtcNow < deadline, $"the core uses [{string.Join(", ", inUse)}], expected {target.Id}");
            await Task.Delay(100);
        }
        Wait(
            () =>
            {
                try
                {
                    var items = Items(app);
                    return items.Count == nodes.Count
                        && Enumerable.Range(0, items.Count).All(index =>
                            ((SelectionItemPattern)items[index].GetCurrentPattern(SelectionItemPattern.Pattern)).Current.IsSelected == (index == place)
                            && items[index].Current.HelpText.Contains(strings["proxies.selected"]) == (index == place));
                }
                catch (Exception problem) when (problem is ElementNotAvailableException or InvalidOperationException)
                {
                    return false;
                }
            },
            () => "the list does not show the chosen node, and only it, as the one in use");

        // The delay test: nothing answers at these addresses, and every row says so.
        Press(app, Test);
        Wait(
            () =>
            {
                try
                {
                    var items = Items(app);
                    return items.Count == nodes.Count && items.All(item => item.Current.HelpText.Contains(strings["proxies.delay.failed"]));
                }
                catch (ElementNotAvailableException)
                {
                    return false;
                }
            },
            () => $"not every node says '{strings["proxies.delay.failed"]}' after the test",
            TestPatience);
        app.WaitForName(Test, strings["proxies.test"]);
        // The core remembers the test for every window.
        var answer = await second.AskAsync(Proxies.Command);
        Assert.All(Proxies.Read(answer).Sections.SelectMany(section => section.Nodes), node => Assert.Equal(NodeDelay.Failed, node.Delay));
        app.Screenshot($"proxies-nodes-{language}-{theme}");
    }
}
