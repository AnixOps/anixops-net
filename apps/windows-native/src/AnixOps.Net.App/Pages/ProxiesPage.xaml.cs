using System.Collections.ObjectModel;
using AnixOps.Net.Ipc;
using AnixOps.Net.Presentation;
using AnixOps.Net.Session;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Navigation;

namespace AnixOps.Net.App.Pages;

/// <summary>One subscription's nodes under its name: a group of the node list.</summary>
public sealed class NodeGroup(string heading) : ObservableCollection<ProxyRow>
{
    /// <summary>The subscription's name.</summary>
    public string Heading { get; } = heading;
}

/// <summary>
/// The proxies page: where a subscription is imported, the search, the delay test and the list of nodes
/// under their subscriptions. It asks the core once a second while it is shown; what the answer means for
/// the page is decided in <see cref="Proxies"/>. The selected row is the node in use; selecting another row
/// asks the core to use that node.
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

    private readonly DispatcherTimer timer = new() { Interval = TimeSpan.FromSeconds(1) };
    private readonly ObservableCollection<NodeGroup> groups = [];
    private Strings? strings;
    private CoreLink? core;
    private bool active;

    private Proxies? proxies;
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
                var group = new NodeGroup(run.First().Heading);
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
