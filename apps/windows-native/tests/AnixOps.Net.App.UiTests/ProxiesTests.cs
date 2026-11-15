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

/// <summary>
/// The proxies page: it imports links, lists the core's nodes, searches them, switches and tests them, and
/// its list of subscriptions updates, renames and removes one; its policy groups choose a member.
/// </summary>
public sealed class ProxiesTests
{
    private const string Link = "Proxies.Link";
    private const string Import = "Proxies.Import";
    private const string Search = "Proxies.Search";
    private const string Test = "Proxies.Test";
    private const string Count = "Proxies.Count";
    private const string Nodes = "Proxies.List";
    private const string Notice = "Proxies.Notice";
    private const string OpenSubscriptions = "Proxies.Subscriptions";
    private const string SubscriptionList = "Subscriptions.List";
    private const string Update = "Subscriptions.Update";
    private const string Rename = "Subscriptions.Rename";
    private const string Remove = "Subscriptions.Remove";
    private const string Question = "Subscriptions.Question";
    private const string NewName = "Subscriptions.Name";
    private const string Confirm = "Subscriptions.Confirm";
    private const string Cancel = "Subscriptions.Cancel";
    private const string OpenGroups = "Proxies.Groups";
    private const string GroupList = "Groups.List";
    private const string Choose = "Groups.Choose";
    private const string GroupName = "Groups.Group";
    private const string MemberList = "Groups.Members";
    private const string Back = "Groups.Back";

    // Addresses kept for documentation (RFC 5737): nothing answers there, so a delay test ends in "no answer".
    private const string FirstNode = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@203.0.113.9:11111#Test";
    private const string ThirdNode = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@203.0.113.20:11111#Alpha";
    private const string FourthNode = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@203.0.113.21:11111#Beta";
    private const string SecondNode = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@203.0.113.10:11111#Second";

    // A subscription with rules and policy groups.
    private const string Routed = """
        proxies:
          - name: HK
            type: ss
            server: 203.0.113.30
            port: 11111
            cipher: aes-256-gcm
            password: f43c0eee-13b9-4f07-bec9-d4b744141503
          - name: JP
            type: ss
            server: 203.0.113.31
            port: 11112
            cipher: aes-256-gcm
            password: f43c0eee-13b9-4f07-bec9-d4b744141504
        proxy-groups:
          - name: Proxy
            type: select
            proxies:
              - Auto
              - HK
              - DIRECT
          - name: Auto
            type: url-test
            proxies:
              - JP
              - HK
            url: http://www.gstatic.com/generate_204
            interval: 300
        rules:
          - DOMAIN-SUFFIX,example.org,DIRECT
          - MATCH,Proxy

        """;

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
        app.WaitForName(Count, Proxies.Count(strings, 1));
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
        app.WaitForName(Count, Proxies.Count(strings, (ulong)nodes.Count));

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

    /// <summary>The row of the subscription with this name in the open list, or null.</summary>
    private static AutomationElement? Row(AppSession app, string title)
    {
        try
        {
            return app.Find(SubscriptionList)
                .FindAll(TreeScope.Descendants, new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.ListItem))
                .Cast<AutomationElement>()
                .FirstOrDefault(item => item.Current.Name == title);
        }
        catch (ElementNotAvailableException)
        {
            // The list changed while it was read.
            return null;
        }
    }

    /// <summary>The sentences a row shows.</summary>
    private static List<string> Says(AutomationElement? row)
    {
        if (row is null)
        {
            return [];
        }
        try
        {
            return row
                .FindAll(TreeScope.Descendants, new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Text))
                .Cast<AutomationElement>()
                .Select(text => text.Current.Name)
                .ToList();
        }
        catch (ElementNotAvailableException)
        {
            return [];
        }
    }

    private static AutomationElement? ButtonIn(AutomationElement row, string automationId) =>
        row.FindFirst(TreeScope.Descendants, new PropertyCondition(AutomationElement.AutomationIdProperty, automationId));

    /// <summary>Presses a row's button once the row is there and the button can be pressed.</summary>
    private static void PressIn(AppSession app, string title, string automationId)
    {
        AutomationElement? button = null;
        Wait(
            () =>
            {
                try
                {
                    button = Row(app, title) is { } row ? ButtonIn(row, automationId) : null;
                    return button is { Current.IsEnabled: true };
                }
                catch (ElementNotAvailableException)
                {
                    return false;
                }
            },
            () => $"the subscription '{title}' has no '{automationId}' that can be pressed");
        Assert.True(button!.TryGetCurrentPattern(InvokePattern.Pattern, out var invoke), $"'{automationId}' cannot be pressed");
        ((InvokePattern)invoke).Invoke();
    }

    [Theory]
    [InlineData(Strings.Chinese, "light")]
    [InlineData(Strings.English, "dark")]
    public async Task SubscriptionsAreListedUpdatedRenamedAndRemovedThroughTheService(string language, string theme)
    {
        if (!ServiceProcess.Available())
        {
            return;
        }
        var strings = Strings.For(language);
        using var service = ServiceProcess.Start();
        using var app = AppSession.Start(
            "--language", language, "--theme", theme, "--page", Sections.Proxies, "--pipe", service.PipeName);
        app.WaitForName(Notice, strings["proxies.empty"]);
        Assert.Equal(strings["proxies.subscriptions"], app.Find(OpenSubscriptions).Current.Name);
        WaitForEnabled(app, OpenSubscriptions);
        await using var second = await SecondWindow.JoinAsync(service.PipeName);

        async Task<IReadOnlyList<Subscription>> ListedAsync() => Subscriptions.Read(await second.AskAsync(Subscriptions.Command));

        // Share links pasted into the client, and a subscription this test hands out: one node at first, two
        // from the second download on.
        await second.AskAsync("import_subscription", JsonSerializer.Serialize(new { location = FirstNode }));
        var before = (await ListedAsync()).Select(subscription => subscription.Id).ToHashSet();
        using var server = new SubscriptionServer(download => download == 1 ? ThirdNode : ThirdNode + "\n" + FourthNode);
        await second.AskAsync("import_subscription", JsonSerializer.Serialize(new { location = server.Link }));
        var mine = Assert.Single(await ListedAsync(), subscription => !before.Contains(subscription.Id));
        Assert.False(mine.Pasted);
        Assert.True(mine.Refreshable);
        Assert.Equal(1UL, mine.Nodes);
        Assert.Equal(1000UL, mine.Used);
        Assert.Equal(5000UL, mine.Total);
        Assert.Equal("2033-05-18", mine.ExpireDate);
        Assert.False(mine.Expired);
        Assert.Null(mine.Error);
        var title = mine.Name;

        async Task<Subscription?> MineAsync() => (await ListedAsync()).FirstOrDefault(subscription => subscription.Id == mine.Id);

        async Task UntilAsync(Func<Subscription?, bool> done, string otherwise)
        {
            var deadline = DateTime.UtcNow + Patience;
            while (!done(await MineAsync()))
            {
                Assert.True(DateTime.UtcNow < deadline, otherwise);
                await Task.Delay(100);
            }
        }

        // The list: the subscription's row says how many nodes, how much was used and when it expires, in
        // one line.
        Press(app, OpenSubscriptions);
        Assert.Equal(strings["proxies.subscriptions"], app.Find(SubscriptionList).Current.Name);
        string[] expected =
        [
            Proxies.Count(strings, 1),
            strings.Format("subscriptions.usage", "1000 B", "4.9 KB"),
            strings.Format("subscriptions.expire", "2033-05-18"),
        ];
        Wait(
            () => Says(Row(app, title)).Any(line => expected.All(part => line.Contains(part))),
            () => $"the row of '{title}' says [{string.Join(" | ", Says(Row(app, title)))}], expected one line with [{string.Join(" | ", expected)}]");

        // Pasted links have a row too, named in the window's language, and nothing to download again.
        var pasted = strings["proxies.group.pasted"];
        Wait(() => Row(app, pasted) is not null, () => $"no row '{pasted}'");
        Assert.Null(ButtonIn(Row(app, pasted)!, Update));
        Assert.NotNull(ButtonIn(Row(app, pasted)!, Rename));
        Assert.Equal(strings["subscriptions.rename"], ButtonIn(Row(app, title)!, Rename)!.Current.Name);
        Assert.Equal(strings["subscriptions.remove"], ButtonIn(Row(app, title)!, Remove)!.Current.Name);
        Assert.Equal(strings["subscriptions.update"], ButtonIn(Row(app, title)!, Update)!.Current.Name);

        // Update: the link is downloaded again and now hands out two nodes.
        PressIn(app, title, Update);
        Wait(() => server.Downloads >= 2, () => $"the link was downloaded {server.Downloads} times");
        await UntilAsync(subscription => subscription?.Nodes == 2, "the core does not hold the subscription's two nodes");
        var two = Proxies.Count(strings, 2);
        Wait(
            () => Says(Row(app, title)).Any(line => line.Contains(two)),
            () => $"the row of '{title}' says [{string.Join(" | ", Says(Row(app, title)))}], expected '{two}'");

        // Rename: the question starts with the name as it is and takes another.
        const string renamed = "My nodes";
        PressIn(app, title, Rename);
        app.WaitForName(Question, strings["subscriptions.rename"]);
        Assert.Equal(strings["subscriptions.name"], app.Find(NewName).Current.Name);
        Assert.Equal(title, ((ValuePattern)app.Find(NewName).GetCurrentPattern(ValuePattern.Pattern)).Current.Value);
        Assert.Equal(strings["subscriptions.save"], app.Find(Confirm).Current.Name);
        Assert.Equal(strings["action.cancel"], app.Find(Cancel).Current.Name);
        app.SetText(NewName, renamed);
        Press(app, Confirm);
        await UntilAsync(subscription => subscription?.Name == renamed, $"the core does not call the subscription '{renamed}'");
        Wait(() => Row(app, renamed) is not null && Row(app, title) is null, () => $"the list does not show the subscription as '{renamed}'");
        app.Screenshot($"proxies-subscriptions-{language}-{theme}");

        // Remove: asked first, and "cancel" leaves everything as it is.
        PressIn(app, renamed, Remove);
        app.WaitForName(Question, strings.Format("subscriptions.remove.confirm", renamed));
        Assert.Equal(strings["subscriptions.remove"], app.Find(Confirm).Current.Name);
        Press(app, Cancel);
        Wait(() => Row(app, renamed) is not null, () => "cancelling the removal took the row away");
        Assert.NotNull(await MineAsync());
        PressIn(app, renamed, Remove);
        app.WaitForName(Question, strings.Format("subscriptions.remove.confirm", renamed));
        Press(app, Confirm);
        await UntilAsync(subscription => subscription is null, "the core still has the subscription");
        Wait(() => Row(app, pasted) is not null && Row(app, renamed) is null, () => "the removed subscription is still listed");
        // Its nodes went with it: the pasted link's node is the one left.
        var nodes = await NodesAsync(second);
        Assert.Equal(new[] { "Test" }, nodes.Select(node => node.Label));
    }

    /// <summary>The element with this automation id if it is on screen now, without waiting for it.</summary>
    private static AutomationElement? Present(AppSession app, string automationId) =>
        app.Window.FindFirst(TreeScope.Descendants, new PropertyCondition(AutomationElement.AutomationIdProperty, automationId));

    /// <summary>The rows of the list with this automation id that exist as controls, in order; none when the list is not there.</summary>
    private static List<AutomationElement> RowsOf(AppSession app, string list)
    {
        try
        {
            return Present(app, list) is { } found
                ? found
                    .FindAll(TreeScope.Descendants, new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.ListItem))
                    .Cast<AutomationElement>()
                    .ToList()
                : [];
        }
        catch (ElementNotAvailableException)
        {
            // The list changed while it was read.
            return [];
        }
    }

    private static AutomationElement? RowOfGroup(AppSession app, string tag)
    {
        try
        {
            return RowsOf(app, GroupList).FirstOrDefault(item => item.Current.Name == tag);
        }
        catch (ElementNotAvailableException)
        {
            return null;
        }
    }

    /// <summary>The members on screen as (name, selected), in order.</summary>
    private static List<(string Name, bool Selected)> Members(AppSession app)
    {
        try
        {
            return RowsOf(app, MemberList)
                .Select(item => (
                    item.Current.Name,
                    ((SelectionItemPattern)item.GetCurrentPattern(SelectionItemPattern.Pattern)).Current.IsSelected))
                .ToList();
        }
        catch (Exception problem) when (problem is ElementNotAvailableException or InvalidOperationException)
        {
            return [];
        }
    }

    [Theory]
    [InlineData(Strings.Chinese, "light")]
    [InlineData(Strings.English, "dark")]
    public async Task PolicyGroupsAreListedAndAMemberIsChosenThroughTheService(string language, string theme)
    {
        if (!ServiceProcess.Available())
        {
            return;
        }
        var strings = Strings.For(language);
        using var service = ServiceProcess.Start();
        using var app = AppSession.Start(
            "--language", language, "--theme", theme, "--page", Sections.Proxies, "--pipe", service.PipeName);
        app.WaitForName(Notice, strings["proxies.empty"]);
        WaitForEnabled(app, OpenSubscriptions);
        // Without a subscription that has groups there is nothing to open.
        Assert.Null(Present(app, OpenGroups));
        await using var second = await SecondWindow.JoinAsync(service.PipeName);

        async Task<IReadOnlyList<Group>> ListedAsync() => Groups.Read(await second.AskAsync(Groups.Command));

        Assert.Empty(await ListedAsync());
        using var server = new SubscriptionServer(_ => Routed);
        await second.AskAsync("import_subscription", JsonSerializer.Serialize(new { location = server.Link }));
        var mine = Assert.Single(Subscriptions.Read(await second.AskAsync(Subscriptions.Command)));
        var groups = await ListedAsync();
        var proxy = Assert.Single(groups, group => group.Tag == "Proxy");
        Assert.Equal(GroupKind.Select, proxy.Kind);
        Assert.True(proxy.Choosable);
        Assert.Equal("Auto", proxy.Selected is { } inUse ? proxy.Members[inUse].Label : null);
        var automatic = Assert.Single(groups, group => group.Tag == "Auto");
        Assert.Equal(GroupKind.Auto, automatic.Kind);
        Assert.False(automatic.Choosable);

        // The groups: each row says how the group chooses and what it uses.
        app.WaitForName(OpenGroups, strings["proxies.groups"]);
        Press(app, OpenGroups);
        Assert.Equal(strings["proxies.groups"], app.Find(GroupList).Current.Name);
        var manual = strings["groups.kind.select"] + " · " + strings.Format("groups.using", "Auto");
        Wait(
            () => Says(RowOfGroup(app, "Proxy")).Contains(manual),
            () => $"the row of 'Proxy' says [{string.Join(" | ", Says(RowOfGroup(app, "Proxy")))}], expected '{manual}'");
        Wait(
            () => Says(RowOfGroup(app, "Auto")).Any(line => line.StartsWith(strings["groups.kind.auto"], StringComparison.Ordinal)),
            () => $"the row of 'Auto' says [{string.Join(" | ", Says(RowOfGroup(app, "Auto")))}]");
        // Only a group the user chooses in opens its members.
        Assert.Null(ButtonIn(RowOfGroup(app, "Auto")!, Choose));
        var open = ButtonIn(RowOfGroup(app, "Proxy")!, Choose);
        Assert.True(open is not null, "the group 'Proxy' does not offer to choose a member");
        Assert.Equal(strings["groups.choose"], open!.Current.Name);

        // The members: the selected one is the member in use, and selecting another uses that one.
        Assert.True(open.TryGetCurrentPattern(InvokePattern.Pattern, out var invoke), "the members of 'Proxy' cannot be opened");
        ((InvokePattern)invoke).Invoke();
        app.WaitForName(GroupName, "Proxy");
        Assert.Equal("Proxy", app.Find(MemberList).Current.Name);
        Assert.Equal(strings["action.back"], app.Find(Back).Current.Name);
        var expected = proxy.Members.Select(member => (member.Label, member.Label == "Auto")).ToList();
        Wait(
            () => Members(app).SequenceEqual(expected),
            () => $"the members are [{string.Join(", ", Members(app))}], expected [{string.Join(", ", expected)}]");
        var target = RowsOf(app, MemberList).First(item => item.Current.Name == "HK");
        Assert.True(target.TryGetCurrentPattern(SelectionItemPattern.Pattern, out var selection), "a member cannot be selected");
        ((SelectionItemPattern)selection).Select();
        var deadline = DateTime.UtcNow + Patience;
        while (true)
        {
            var now = (await ListedAsync()).FirstOrDefault(group => group.Tag == "Proxy");
            var label = now?.Selected is { } at ? now.Members[at].Label : null;
            if (label == "HK")
            {
                break;
            }
            Assert.True(DateTime.UtcNow < deadline, $"the group 'Proxy' uses '{label}' at the core, expected 'HK'");
            await Task.Delay(100);
        }
        var chosen = proxy.Members.Select(member => (member.Label, member.Label == "HK")).ToList();
        Wait(
            () => Members(app).SequenceEqual(chosen),
            () => $"the members are [{string.Join(", ", Members(app))}], expected [{string.Join(", ", chosen)}]");
        app.Screenshot($"proxies-group-members-{language}-{theme}");

        // Back at the groups, the row says what the group uses now.
        Press(app, Back);
        var uses = strings["groups.kind.select"] + " · " + strings.Format("groups.using", "HK");
        Wait(
            () => Says(RowOfGroup(app, "Proxy")).Contains(uses),
            () => $"the row of 'Proxy' says [{string.Join(" | ", Says(RowOfGroup(app, "Proxy")))}], expected '{uses}'");
        app.Screenshot($"proxies-groups-{language}-{theme}");

        // Without the subscription there are no groups: the dialog goes, and the page's button with it.
        await second.AskAsync("remove_subscription", JsonSerializer.Serialize(new { id = mine.Id }));
        Assert.Empty(await ListedAsync());
        Wait(
            () => Present(app, GroupList) is null && Present(app, OpenGroups) is null,
            () => "the policy groups are still offered after their subscription was removed");
    }
}
