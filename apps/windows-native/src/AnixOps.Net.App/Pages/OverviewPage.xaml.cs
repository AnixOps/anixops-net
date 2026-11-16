using AnixOps.Net.Ipc;
using AnixOps.Net.Presentation;
using AnixOps.Net.Session;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace AnixOps.Net.App.Pages;

/// <summary>
/// The overview page: whether the client is connected, the button that connects and disconnects, the node
/// in use and the traffic counters. It asks the core once a second while it is shown; what the answer means
/// for the page is decided in <see cref="Overview"/>. The card that counts the open connections opens their
/// list (<see cref="ConnectionsDialog"/>), which is asked for once a second too while it is shown.
/// </summary>
public sealed partial class OverviewPage : Page
{
    /// <summary>What the page works with.</summary>
    /// <param name="Strings">The interface language's wording.</param>
    /// <param name="Core">The window's link to the core.</param>
    public sealed record Model(Strings Strings, CoreLink Core);

    // The overview is read from memory; an answer that takes this long is not coming.
    private static readonly TimeSpan AskPatience = TimeSpan.FromSeconds(10);

    // Connecting starts the engine and changes the system's settings; it is given the time that takes.
    private static readonly TimeSpan PressPatience = TimeSpan.FromSeconds(120);

    private readonly DispatcherTimer timer = new() { Interval = TimeSpan.FromSeconds(1) };
    private Strings? strings;
    private CoreLink? core;
    private bool active;

    private Overview? overview;
    private Pending pending;

    // Why the user's last press did not work, and why the core would not give the overview.
    private string? failure;
    private string? trouble;

    // Questions are numbered so that an answer overtaken by a newer question is dropped.
    private int asked;
    private int unanswered;

    // The list of open connections while it is shown: what its search holds, what the core listed for that,
    // and what the core is still closing.
    private ConnectionsDialog? listDialog;
    private string query = "";
    private ConnectionList? listed;
    private readonly HashSet<ulong> closing = [];
    private bool closingAll;
    private bool listing;

    // Counts the cores that went away: what was asked of one says nothing about the next.
    private int era;

    public OverviewPage()
    {
        InitializeComponent();
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

        Heading.Text = strings["nav.overview"];
        NodeLabel.Text = strings["overview.node"];
        TrafficHeading.Text = strings["overview.traffic"];
        DownloadLabel.Text = strings["overview.download"];
        UploadLabel.Text = strings["overview.upload"];
        ConnectionsLabel.Text = strings["overview.connections"];
        // The button shows only a picture: what it does is said in its tooltip and to a screen reader.
        AutomationProperties.SetName(OpenConnectionsButton, strings["overview.connections.open"]);
        ToolTipService.SetToolTip(OpenConnectionsButton, strings["overview.connections.open"]);
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
        // A dialog does not outlive the page it is over.
        listDialog?.Hide();
    }

    private bool Connected => core?.Status.State == LinkState.Connected;

    // Raised on a thread of the link; the page is touched on its own.
    private void OnCoreChanged(LinkStatus status) => DispatcherQueue.TryEnqueue(() =>
    {
        if (!active)
        {
            return;
        }
        if (!Connected)
        {
            // What a core that is gone said last is not what the one that comes back will say.
            asked++;
            era++;
            overview = null;
            failure = null;
            trouble = null;
            listed = null;
            closing.Clear();
            closingAll = false;
        }
        Draw();
        OnTick(this, EventArgs.Empty);
        ListConnections();
    });

    private async void OnTick(object? sender, object e)
    {
        ListConnections();
        // While the core is busy connecting or disconnecting it is asked again once that is done.
        if (!active || unanswered > 0 || pending != Pending.Nothing || !Connected)
        {
            return;
        }
        await RefreshAsync();
        if (active)
        {
            Draw();
        }
    }

    /// <summary>Asks the core for the overview and keeps the answer, unless a newer question went out meanwhile.</summary>
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
            var answer = await core.AskAsync(Overview.Command, patience: AskPatience);
            if (mine == asked && Connected)
            {
                overview = Overview.Read(answer);
                trouble = null;
            }
        }
        catch (RefusedException refused)
        {
            if (mine == asked)
            {
                overview = null;
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

    private async void OnPressed(object sender, RoutedEventArgs e)
    {
        if (core is null || !active || !Shown().Enabled)
        {
            return;
        }
        var press = Shown().Press;
        pending = Overview.PendingFor(press);
        failure = null;
        Draw();

        string? problem = null;
        try
        {
            await core.AskAsync(Overview.CommandFor(press), patience: PressPatience);
        }
        catch (Exception error) when (error is RefusedException or IOException or TimeoutException or OperationCanceledException or ObjectDisposedException)
        {
            problem = error.Message;
        }
        // The page keeps saying "connecting" until it knows what came of it.
        await RefreshAsync();
        pending = Pending.Nothing;
        // A core that went away meanwhile is what the page says, not how the question to it ended.
        failure = Connected ? problem : null;
        if (active)
        {
            Draw();
        }
    }

    private async void OnOpenConnections(object sender, RoutedEventArgs e)
    {
        // One dialog at a time.
        if (strings is null || !active || listDialog is not null)
        {
            return;
        }
        var opened = new ConnectionsDialog(strings, Search, CloseConnection, CloseConnections) { XamlRoot = XamlRoot, RequestedTheme = ActualTheme };
        listDialog = opened;
        DrawConnections();
        ListConnections();
        try
        {
            await opened.ShowAsync();
        }
        finally
        {
            listDialog = null;
            query = "";
            listed = null;
            if (active)
            {
                OpenConnectionsButton.Focus(FocusState.Programmatic);
            }
        }
    }

    /// <summary>Asks the core for the open connections while their list is shown, unless an earlier question is still out.</summary>
    private async void ListConnections()
    {
        if (core is null || !active || listDialog is null || listing || !Connected)
        {
            return;
        }
        listing = true;
        var mine = query;
        var from = era;
        ConnectionList? answer = null;
        try
        {
            answer = Connections.Read(await core.AskAsync(Connections.Command, Connections.Args(mine), AskPatience));
        }
        catch (Exception error) when (error is RefusedException or IOException or TimeoutException or OperationCanceledException or ObjectDisposedException)
        {
            // Asked again at the next tick; what was listed stays until then.
        }
        finally
        {
            listing = false;
        }
        if (!active || listDialog is null || !Connected || from != era)
        {
            // The list was closed meanwhile, or the answer is from a core that has gone since.
            return;
        }
        if (mine != query)
        {
            // An answer to a search the user has typed past: ask for the one that stands now.
            ListConnections();
            return;
        }
        if (answer is not null)
        {
            listed = answer;
            DrawConnections();
        }
    }

    private void Search(string typed)
    {
        if (typed == query)
        {
            return;
        }
        query = typed;
        ListConnections();
    }

    private void CloseConnection(ulong id)
    {
        // A connection is closed once.
        if (!closingAll && closing.Add(id))
        {
            Close(id);
        }
    }

    private void CloseConnections()
    {
        if (!closingAll)
        {
            closingAll = true;
            Close(null);
        }
    }

    /// <summary>Has the core close one connection, or all of them for null.</summary>
    private async void Close(ulong? id)
    {
        if (core is null || !Connected)
        {
            closing.Clear();
            closingAll = false;
            return;
        }
        DrawConnections();
        var from = era;
        try
        {
            if (id is { } one)
            {
                await core.AskAsync(Connections.CloseCommand, Connections.CloseArgs(one), AskPatience);
            }
            else
            {
                await core.AskAsync(Connections.CloseAllCommand, patience: AskPatience);
            }
        }
        catch (Exception error) when (error is RefusedException or IOException or TimeoutException or OperationCanceledException or ObjectDisposedException)
        {
            // A connection that ended by itself meanwhile is as closed as one the core closed: the list that
            // is asked for next says what is open.
        }
        if (from != era)
        {
            // The core went away meanwhile, and what it was closing with it.
            return;
        }
        if (id is { } done)
        {
            closing.Remove(done);
        }
        else
        {
            closingAll = false;
        }
        DrawConnections();
        // The list as it is after this, not at the next tick.
        ListConnections();
    }

    /// <summary>Draws the list of open connections, if it is shown.</summary>
    private void DrawConnections()
    {
        if (strings is null || listDialog is not { } dialog)
        {
            return;
        }
        // Without an engine the list says so; with one, it waits for what the core lists.
        var known = listed ?? (Connected && overview?.State == ConnectionState.Connected ? null : ConnectionList.Nothing);
        dialog.Show(known is null ? null : Connections.Show(strings, known, query, closing, closingAll));
    }

    private OverviewShown Shown() =>
        Overview.Show(strings!, core?.Status.State != LinkState.Unavailable, overview, pending, failure ?? trouble);

    private void Draw()
    {
        if (strings is null)
        {
            return;
        }
        var shown = Shown();
        StateText.Text = shown.Headline;
        var on = overview?.State == ConnectionState.Connected && pending == Pending.Nothing;
        DotOn.Visibility = on ? Visibility.Visible : Visibility.Collapsed;
        DotOff.Visibility = on ? Visibility.Collapsed : Visibility.Visible;
        Write(DetailText, shown.Detail);
        Write(ErrorText, shown.Error);

        ConnectButton.Content = shown.Button;
        ConnectButton.IsEnabled = shown.Enabled;
        // Connecting is the thing to do on this page; disconnecting is offered, not urged.
        ConnectButton.Style = (Style)Application.Current.Resources[shown.Press == Press.Connect ? "AccentButtonStyle" : "DefaultButtonStyle"];

        NodeText.Text = shown.Node;
        DownloadRate.Text = shown.DownloadRate;
        UploadRate.Text = shown.UploadRate;
        // A space keeps the line's height while there is no total to show.
        DownloadTotal.Text = shown.DownloadTotal ?? " ";
        UploadTotal.Text = shown.UploadTotal ?? " ";
        ConnectionsText.Text = shown.Connections;
        DrawConnections();
    }

    private static void Write(TextBlock block, string? text)
    {
        block.Text = text ?? "";
        block.Visibility = text is null ? Visibility.Collapsed : Visibility.Visible;
    }
}
