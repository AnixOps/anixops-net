using System.Collections.ObjectModel;
using System.Text.Json;
using AnixOps.Net.Ipc;
using AnixOps.Net.Presentation;
using AnixOps.Net.Session;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Navigation;
using PolicyGroups = AnixOps.Net.Presentation.Groups;

namespace AnixOps.Net.App.Pages;

/// <summary>One subscription's nodes under its name: a group of the node list.</summary>
public sealed class NodeGroup : ObservableCollection<ProxyRow>
{
    /// <summary>The subscription's name.</summary>
    public string Heading { get; set; } = "";
}

/// <summary>
/// The proxies page: where a subscription is imported, the search, the delay test and the list of nodes
/// under their subscriptions. It asks the core once a second while it is shown; what the answer means for
/// the page is decided in <see cref="Proxies"/>. The selected row is the node in use; selecting another row
/// asks the core to use that node. A row's button opens the node's details, where it is changed, removed or
/// shared.
/// </summary>
public sealed partial class ProxiesPage : Page
{
    /// <summary>What the page works with.</summary>
    /// <param name="Strings">The interface language's wording.</param>
    /// <param name="Core">The window's link to the core.</param>
    public sealed record Model(Strings Strings, CoreLink Core);

    // The list is read from memory; an answer that takes this long is not coming.
    private static readonly TimeSpan AskPatience = TimeSpan.FromSeconds(10);

    // Importing downloads the subscription; switching can restart the engine.
    private static readonly TimeSpan ActPatience = TimeSpan.FromSeconds(120);

    // A node that does not answer takes seconds, and a subscription can have thousands.
    private static readonly TimeSpan TestPatience = TimeSpan.FromMinutes(30);

    // A checkup runs latency rounds and, when asked, a download of several seconds; the core answers when
    // it is over.
    private static readonly TimeSpan CheckupPatience = TimeSpan.FromMinutes(2);

    private static readonly JsonElement NoArgs = JsonSerializer.SerializeToElement(new Dictionary<string, string>());

    private readonly DispatcherTimer timer = new() { Interval = TimeSpan.FromSeconds(1) };
    private readonly ObservableCollection<NodeGroup> groups = [];
    private Strings? strings;
    private CoreLink? core;
    private bool active;

    private Proxies? proxies;

    // The subscriptions the core says there are (null while it has not said), the ones it is still working
    // on, and the list of them while it is open.
    private IReadOnlyList<Subscription>? subscriptions;
    private readonly HashSet<string> changing = [];
    private SubscriptionsDialog? dialog;

    // The policy groups the core says there are (null while it has not said), the ones it is still choosing
    // a member for, and the list of them while it is open.
    private IReadOnlyList<Group>? policies;
    private readonly HashSet<string> choosing = [];
    private GroupsDialog? policyDialog;

    // The node whose details are open (null when none are), whether the core is saving or removing it, why
    // that did not work, and the dialog that shows it. "opening" is set while the core is asked for a
    // node's details, so that a second press does not ask again.
    private NodeDetails? node;
    private bool nodeBusy;
    private string? nodeProblem;
    private NodeDialog? nodeDialog;

    // The checkup of the node whose details are open: whether it runs, what it found and why it did not
    // work. Checkups are numbered so that the answer for a dialog that has closed since is dropped.
    private int checkups;
    private bool checkupRunning;
    private IReadOnlyList<CheckupSection>? checkupSections;
    private string? checkupProblem;
    private bool opening;
    private string query = "";
    private ProxiesBusy busy;

    // Why the last thing the user asked for did not work, and why the core would not give the list.
    private string? failure;
    private string? trouble;

    // Questions are numbered so that an answer overtaken by a newer question is dropped.
    private int asked;
    private int unanswered;

    // Counts the changes of the link to the core, so that the answer to something asked of a core that has
    // gone since is not taken for the present one's.
    private int era;

    // The rows the list holds now, to change only what differs.
    private IReadOnlyList<ProxyRow> drawn = [];

    // Set while the page itself changes the list, so that this is not taken for the user choosing a node.
    private bool syncing;

    public ProxiesPage()
    {
        InitializeComponent();
        Groups.Source = groups;
        timer.Tick += OnTick;
        Unloaded += (_, _) => Leave();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        if (e.Parameter is not Model model)
        {
            return;
        }
        strings = model.Strings;
        core = model.Core;
        active = true;

        Heading.Text = strings["nav.proxies"];
        LinkBox.PlaceholderText = strings["proxies.import.link"];
        AutomationProperties.SetName(LinkBox, strings["proxies.import.link"]);
        SearchBox.PlaceholderText = strings["proxies.search"];
        AutomationProperties.SetName(SearchBox, strings["proxies.search"]);
        AutomationProperties.SetName(NodeList, strings["proxies.list"]);
        SubscriptionsButton.Content = strings["proxies.subscriptions"];
        GroupsButton.Content = strings["proxies.groups"];
        Draw();

        core.Changed += OnCoreChanged;
        timer.Start();
        OnTick(this, EventArgs.Empty);
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        base.OnNavigatedFrom(e);
        Leave();
    }

    private void Leave()
    {
        if (!active)
        {
            return;
        }
        active = false;
        timer.Stop();
        dialog?.Close();
        policyDialog?.Close();
        nodeDialog?.Close();
        if (core is not null)
        {
            core.Changed -= OnCoreChanged;
        }
    }

    private bool Connected => core?.Status.State == LinkState.Connected;

    // Raised on a thread of the link; the page is touched on its own.
    private void OnCoreChanged(LinkStatus status) => DispatcherQueue.TryEnqueue(() =>
    {
        if (!active)
        {
            return;
        }
        // What was known and what was asked belong to the connection that was.
        era++;
        asked++;
        proxies = null;
        subscriptions = null;
        changing.Clear();
        policies = null;
        choosing.Clear();
        node = null;
        nodeBusy = false;
        nodeProblem = null;
        ForgetCheckup();
        busy = default;
        failure = null;
        trouble = null;
        Draw();
        OnTick(this, EventArgs.Empty);
    });

    private async void OnTick(object? sender, object e)
    {
        if (!active || unanswered > 0 || !Connected)
        {
            return;
        }
        await RefreshAsync();
        if (active)
        {
            Draw();
        }
    }

    /// <summary>Asks the core for the list the search leaves and keeps the answer, unless a newer question went out meanwhile.</summary>
    private async Task RefreshAsync()
    {
        if (core is null)
        {
            return;
        }
        var mine = ++asked;
        unanswered++;
        try
        {
            var answer = await core.AskAsync(Proxies.Command, Proxies.Args(query), AskPatience);
            if (mine == asked && Connected)
            {
                proxies = Proxies.Read(answer);
                trouble = null;
            }
        }
        catch (RefusedException refused)
        {
            if (mine == asked)
            {
                proxies = null;
                trouble = refused.Message;
            }
        }
        catch (Exception error) when (error is IOException or TimeoutException or OperationCanceledException or ObjectDisposedException)
        {
            // The link says so itself when the core is gone; a slow answer is asked for again.
        }
        finally
        {
            unanswered--;
        }
        await RefreshSubscriptionsAsync();
    }

    /// <summary>Asks the core which subscriptions and which policy groups there are. An answer that does not come leaves what was known.</summary>
    private async Task RefreshSubscriptionsAsync()
    {
        if (core is null)
        {
            return;
        }
        var asking = era;
        try
        {
            var answer = await core.AskAsync(Subscriptions.Command, NoArgs, AskPatience);
            if (asking == era && Connected)
            {
                subscriptions = Subscriptions.Read(answer);
            }
        }
        catch (Exception error) when (error is RefusedException or IOException or TimeoutException or OperationCanceledException or ObjectDisposedException)
        {
            // Asked for again at the next tick.
        }
        try
        {
            var answer = await core.AskAsync(PolicyGroups.Command, NoArgs, AskPatience);
            if (asking == era && Connected)
            {
                policies = PolicyGroups.Read(answer);
            }
        }
        catch (Exception error) when (error is RefusedException or IOException or TimeoutException or OperationCanceledException or ObjectDisposedException)
        {
            // Asked for again at the next tick.
        }
    }

    /// <summary>Asks again at once, not at the next tick: also while an earlier question is still out.</summary>
    private async void RefreshNow()
    {
        if (!active || !Connected)
        {
            return;
        }
        await RefreshAsync();
        if (active)
        {
            Draw();
        }
    }

    private async void Act(ProxyAction action)
    {
        if (core is null || !active)
        {
            return;
        }
        var asking = era;
        busy = busy.With(action.Kind, true);
        failure = null;
        Draw();

        string? problem = null;
        try
        {
            await core.AskAsync(action.Command, action.Args(), action.Kind == ProxyActionKind.Test ? TestPatience : ActPatience);
        }
        catch (Exception error) when (error is RefusedException or IOException or TimeoutException or OperationCanceledException or ObjectDisposedException)
        {
            problem = error.Message;
        }
        if (asking != era)
        {
            // The core that was asked is gone; what it was busy with went with it.
            return;
        }
        // The list as it is after this, before the page stops saying that the core is busy with it.
        await RefreshAsync();
        if (asking != era)
        {
            return;
        }
        busy = busy.With(action.Kind, false);
        failure = problem;
        if (action.Kind == ProxyActionKind.Import && problem is null)
        {
            LinkBox.Text = "";
        }
        if (active)
        {
            Draw();
        }
    }

    /// <summary>Asks the core for a change to one subscription. The others do not wait for it.</summary>
    private async void Change(SubscriptionChange change)
    {
        if (core is null || !active || !Connected || !changing.Add(change.Id))
        {
            return;
        }
        var asking = era;
        failure = null;
        Draw();

        string? problem = null;
        try
        {
            await core.AskAsync(change.Command, change.Args(), ActPatience);
        }
        catch (Exception error) when (error is RefusedException or IOException or TimeoutException or OperationCanceledException or ObjectDisposedException)
        {
            problem = error.Message;
        }
        if (asking != era)
        {
            // The core that was asked is gone; what it was busy with went with it.
            return;
        }
        // The nodes and the subscriptions as they are after this, before the row can be asked again.
        await RefreshAsync();
        if (asking != era)
        {
            return;
        }
        changing.Remove(change.Id);
        failure = problem;
        if (active)
        {
            Draw();
        }
    }

    /// <summary>Tells the core which member the user chose for a policy group. The other groups do not wait for it.</summary>
    private async void Choose(GroupChoice choice)
    {
        if (core is null || !active || !Connected || !choosing.Add(choice.Group))
        {
            return;
        }
        var asking = era;
        failure = null;
        Draw();

        string? problem = null;
        try
        {
            await core.AskAsync(GroupChoice.Command, choice.Args(), ActPatience);
        }
        catch (Exception error) when (error is RefusedException or IOException or TimeoutException or OperationCanceledException or ObjectDisposedException)
        {
            problem = error.Message;
        }
        if (asking != era)
        {
            // The core that was asked is gone; what it was busy with went with it.
            return;
        }
        // The nodes and the groups as they are after this, before a member can be chosen in the group again.
        await RefreshAsync();
        if (asking != era)
        {
            return;
        }
        choosing.Remove(choice.Group);
        failure = problem;
        if (active)
        {
            Draw();
        }
    }

    /// <summary>Whether a dialog is over the page: Windows shows one at a time.</summary>
    private bool DialogOpen => dialog is not null || policyDialog is not null || nodeDialog is not null;

    private void OnDetailsLoaded(object sender, RoutedEventArgs e)
    {
        // The button shows only a picture: what it does is said in its tooltip and to a screen reader.
        if (sender is Button button && strings is not null)
        {
            AutomationProperties.SetName(button, strings["node.details"]);
            ToolTipService.SetToolTip(button, strings["node.details"]);
        }
    }

    /// <summary>Asks the core about the node of the row whose button was pressed and shows the answer in a dialog.</summary>
    private async void OnDetails(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is not ProxyRow row
            || strings is null || core is null || !active || !Connected || DialogOpen || opening)
        {
            return;
        }
        var asking = era;
        opening = true;
        NodeDetails details;
        try
        {
            details = Node.Read(await core.AskAsync(Node.Command, Node.Args(row.Id), AskPatience));
        }
        catch (Exception error) when (error is RefusedException or IOException or TimeoutException or OperationCanceledException or ObjectDisposedException)
        {
            if (asking == era && active)
            {
                failure = error.Message;
                Draw();
            }
            return;
        }
        finally
        {
            opening = false;
        }
        if (asking != era || !active || DialogOpen)
        {
            return;
        }
        node = details;
        nodeBusy = false;
        nodeProblem = null;
        failure = null;
        ForgetCheckup();
        var opened = new NodeDialog(strings, SaveNode, RemoveNode, CheckNode) { XamlRoot = XamlRoot, RequestedTheme = ActualTheme };
        nodeDialog = opened;
        Draw();
        try
        {
            await opened.ShowAsync();
        }
        finally
        {
            nodeDialog = null;
            node = null;
            nodeProblem = null;
            ForgetCheckup();
            if (active)
            {
                NodeList.Focus(FocusState.Programmatic);
            }
        }
    }

    /// <summary>
    /// Saves what the user typed for the node whose details are open: the core says first whether it can be
    /// saved, and afterwards what the node is now.
    /// </summary>
    private async void SaveNode(NodeDraft draft)
    {
        if (core is null || strings is null || !active || !Connected || nodeBusy || node is not { } details)
        {
            return;
        }
        var asking = era;
        nodeBusy = true;
        nodeProblem = null;
        Draw();

        string? problem = null;
        NodeDetails? now = null;
        try
        {
            problem = Node.Problem(strings, await core.AskAsync(NodeDraft.CheckCommand, draft.CheckArgs(), AskPatience));
            if (problem is null)
            {
                await core.AskAsync(NodeDraft.SaveCommand, draft.SaveArgs(details.Id), ActPatience);
                now = Node.Read(await core.AskAsync(Node.Command, Node.Args(details.Id), AskPatience));
            }
        }
        catch (Exception error) when (error is RefusedException or IOException or TimeoutException or OperationCanceledException or ObjectDisposedException)
        {
            problem ??= error.Message;
        }
        if (asking != era)
        {
            // The core that was asked is gone; what it was busy with went with it.
            return;
        }
        // The list as it is after this, before the node can be saved again.
        await RefreshAsync();
        if (asking != era)
        {
            return;
        }
        nodeBusy = false;
        if (node?.Id == details.Id)
        {
            nodeProblem = problem;
            node = now ?? node;
        }
        if (active)
        {
            Draw();
        }
    }

    /// <summary>Drops the checkup of a dialog that closed: its answer, when it comes, is not drawn.</summary>
    private void ForgetCheckup()
    {
        checkups++;
        checkupRunning = false;
        checkupSections = null;
        checkupProblem = null;
    }

    /// <summary>Runs a checkup of the node whose details are open; <paramref name="throughput"/> adds the download.</summary>
    private async void CheckNode(bool throughput)
    {
        if (core is null || strings is null || !active || !Connected || checkupRunning || node is not { } details)
        {
            return;
        }
        var mine = ++checkups;
        checkupRunning = true;
        checkupProblem = null;
        Draw();

        IReadOnlyList<CheckupSection>? sections = null;
        string? problem = null;
        try
        {
            sections = Checkup.Show(strings, await core.AskAsync(Checkup.Command, Checkup.Args(details.Id, throughput), CheckupPatience));
        }
        catch (Exception error) when (error is RefusedException or IOException or TimeoutException or OperationCanceledException or ObjectDisposedException)
        {
            problem = error.Message;
        }
        if (mine != checkups)
        {
            return;
        }
        checkupRunning = false;
        checkupSections = sections;
        checkupProblem = problem;
        if (active)
        {
            Draw();
        }
    }

    /// <summary>Removes the node whose details are open. Its dialog goes with it.</summary>
    private async void RemoveNode()
    {
        if (core is null || !active || !Connected || nodeBusy || node is not { } details)
        {
            return;
        }
        var asking = era;
        nodeBusy = true;
        nodeProblem = null;
        Draw();

        string? problem = null;
        try
        {
            await core.AskAsync(Node.Remove, Node.Args(details.Id), ActPatience);
        }
        catch (Exception error) when (error is RefusedException or IOException or TimeoutException or OperationCanceledException or ObjectDisposedException)
        {
            problem = error.Message;
        }
        if (asking != era)
        {
            return;
        }
        await RefreshAsync();
        if (asking != era)
        {
            return;
        }
        nodeBusy = false;
        if (node?.Id == details.Id)
        {
            if (problem is null)
            {
                node = null;
            }
            else
            {
                nodeProblem = problem;
            }
        }
        if (active)
        {
            Draw();
        }
    }

    private async void OnGroups(object sender, RoutedEventArgs e)
    {
        // One dialog at a time.
        if (strings is null || DialogOpen || policies is not { Count: > 0 })
        {
            return;
        }
        var opened = new GroupsDialog(strings, Choose) { XamlRoot = XamlRoot, RequestedTheme = ActualTheme };
        policyDialog = opened;
        Draw();
        try
        {
            await opened.ShowAsync();
        }
        finally
        {
            policyDialog = null;
            if (active && GroupsButton.Visibility == Visibility.Visible)
            {
                GroupsButton.Focus(FocusState.Programmatic);
            }
        }
    }

    private async void OnSubscriptions(object sender, RoutedEventArgs e)
    {
        if (strings is null || DialogOpen || subscriptions is null)
        {
            return;
        }
        var opened = new SubscriptionsDialog(strings, Change) { XamlRoot = XamlRoot, RequestedTheme = ActualTheme };
        dialog = opened;
        Draw();
        try
        {
            await opened.ShowAsync();
        }
        finally
        {
            dialog = null;
            if (active)
            {
                SubscriptionsButton.Focus(FocusState.Programmatic);
            }
        }
    }

    private void OnImport(object sender, RoutedEventArgs e)
    {
        var location = LinkBox.Text.Trim();
        if (location.Length > 0 && Shown().CanImport)
        {
            Act(new ProxyAction(ProxyActionKind.Import, location));
        }
    }

    private void OnLinkKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == Windows.System.VirtualKey.Enter)
        {
            e.Handled = true;
            OnImport(sender, e);
        }
    }

    private void OnTest(object sender, RoutedEventArgs e)
    {
        if (Shown().CanTest)
        {
            Act(new ProxyAction(ProxyActionKind.Test, query));
        }
    }

    private void OnSearchChanged(object sender, TextChangedEventArgs e)
    {
        if (SearchBox.Text == query)
        {
            return;
        }
        query = SearchBox.Text;
        RefreshNow();
    }

    private void OnSelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (syncing || NodeList.SelectedItem is not ProxyRow row)
        {
            return;
        }
        // Choosing the node that is already in use asks for nothing.
        if (!row.Selected && Shown().CanSwitch)
        {
            Act(new ProxyAction(ProxyActionKind.Switch, row.Id));
        }
    }

    private void OnGroupHeading(FrameworkElement sender, DataContextChangedEventArgs args)
    {
        if (sender is TextBlock heading)
        {
            heading.Text = (args.NewValue as NodeGroup)?.Heading ?? "";
        }
    }

    private void OnContainerContentChanging(ListViewBase sender, ContainerContentChangingEventArgs args)
    {
        if (!args.InRecycleQueue && args.Item is ProxyRow row && strings is not null)
        {
            AutomationProperties.SetName(args.ItemContainer, row.Label);
            AutomationProperties.SetHelpText(args.ItemContainer, row.Description(strings));
        }
    }

    private ProxiesShown Shown() =>
        Proxies.Show(strings!, core?.Status.State != LinkState.Unavailable, proxies, busy, failure ?? trouble);

    private void Draw()
    {
        if (strings is null)
        {
            return;
        }
        var shown = Shown();
        ImportButton.Content = shown.Import;
        ImportButton.IsEnabled = shown.CanImport;
        TestButton.Content = shown.Test;
        TestButton.IsEnabled = shown.CanTest;
        CountText.Text = shown.Count;
        Write(ErrorText, shown.Error);
        Write(NoticeText, shown.Notice);
        NodeList.Visibility = shown.Notice is null ? Visibility.Visible : Visibility.Collapsed;
        SubscriptionsButton.IsEnabled = subscriptions is not null;
        GroupsButton.Visibility = policies is { Count: > 0 } ? Visibility.Visible : Visibility.Collapsed;
        if (policyDialog is not null)
        {
            if (policies is not { Count: > 0 })
            {
                // The core that listed them is gone, or a subscription without groups came into use: there
                // is nothing left to choose in.
                policyDialog.Close();
            }
            else
            {
                policyDialog.Show(PolicyGroups.Show(strings, policies, choosing));
            }
        }
        if (nodeDialog is not null)
        {
            if (node is null)
            {
                // The node was removed, or the core that described it is gone.
                nodeDialog.Close();
            }
            else
            {
                nodeDialog.Show(Node.Show(strings, node, nodeBusy, nodeProblem));
                nodeDialog.ShowCheckup(checkupRunning, checkupSections, checkupProblem);
            }
        }
        if (dialog is not null)
        {
            if (subscriptions is null)
            {
                // The core that listed them is gone: nothing of what the dialog shows can be asked for.
                dialog.Close();
            }
            else
            {
                dialog.Show(Subscriptions.Show(strings, subscriptions, changing));
            }
        }

        syncing = true;
        try
        {
            Sync(shown.Rows);
            // While the core is switching, the row the user chose stays selected, whatever the core still
            // says is in use.
            // The row is looked for among the ones the list holds: an equal row of a later answer is not it.
            var inUse = drawn.FirstOrDefault(row => row.Selected);
            if (!busy.Switching && !ReferenceEquals(NodeList.SelectedItem, inUse))
            {
                NodeList.SelectedItem = inUse;
            }
        }
        finally
        {
            syncing = false;
        }
    }

    /// <summary>Makes the list hold these rows, changing only what differs so that it stays where the user scrolled it.</summary>
    private void Sync(IReadOnlyList<ProxyRow> rows)
    {
        if (drawn.SequenceEqual(rows))
        {
            return;
        }
        var sameShape = drawn.Count == rows.Count
            && drawn.Zip(rows).All(pair => pair.First.Section == pair.Second.Section && pair.First.Heading == pair.Second.Heading);
        if (sameShape)
        {
            // The same nodes with other delays or another one in use.
            var place = 0;
            foreach (var group in groups)
            {
                for (var index = 0; index < group.Count; index++, place++)
                {
                    if (group[index] != rows[place])
                    {
                        group[index] = rows[place];
                    }
                }
            }
        }
        else
        {
            groups.Clear();
            foreach (var run in rows.GroupBy(row => row.Section))
            {
                var group = new NodeGroup { Heading = run.First().Heading };
                foreach (var row in run)
                {
                    group.Add(row);
                }
                groups.Add(group);
            }
        }
        drawn = groups.SelectMany(group => group).ToList();
    }

    private static void Write(TextBlock block, string? text)
    {
        block.Text = text ?? "";
        block.Visibility = text is null ? Visibility.Collapsed : Visibility.Visible;
    }
}
