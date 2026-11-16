using System.Collections.ObjectModel;
using System.ComponentModel;
using AnixOps.Net.Presentation;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;

namespace AnixOps.Net.App.Pages;

/// <summary>
/// One connection's row in the open dialog. It stays the same object while the connection stays in its
/// place, so that the keyboard focus stays on its button while the numbers change.
/// </summary>
public sealed class ConnectionLine : INotifyPropertyChanged
{
    private ConnectionRow row;

    public ConnectionLine(ConnectionRow row, string closeSays)
    {
        this.row = row;
        CloseSays = closeSays;
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    /// <summary>What the row shows now.</summary>
    public ConnectionRow Row
    {
        get => row;
        set
        {
            if (row == value)
            {
                return;
            }
            row = value;
            // Everything the row says comes from the one value.
            PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(string.Empty));
        }
    }

    public string Title => row.Title;

    public string Details => row.Details;

    /// <summary>Whether the connection can be closed now.</summary>
    public bool CanClose => row.Enabled;

    /// <summary>What the button that shows only a picture does, for the tooltip and a screen reader.</summary>
    public string CloseSays { get; }
}

/// <summary>
/// The open connections over the overview page: a search, the count, "close all" and one row per connection
/// with the button that closes it. What the rows say is decided in <see cref="Connections"/>; this draws it
/// and hands what the user asks for to the page.
/// </summary>
public sealed partial class ConnectionsDialog : ContentDialog
{
    private readonly Strings strings;
    private readonly Action<string> search;
    private readonly Action<ulong> close;
    private readonly Action closeAll;
    private readonly ObservableCollection<ConnectionLine> lines = [];

    /// <param name="search">Called with what the search holds when it changes.</param>
    /// <param name="close">Called with the id of the connection the user closes.</param>
    /// <param name="closeAll">Called when the user closes every connection.</param>
    public ConnectionsDialog(Strings strings, Action<string> search, Action<ulong> close, Action closeAll)
    {
        this.strings = strings;
        this.search = search;
        this.close = close;
        this.closeAll = closeAll;
        InitializeComponent();
        Title = strings["overview.connections"];
        CloseButtonText = strings["action.close"];
        DefaultButton = ContentDialogButton.None;
        List.ItemsSource = lines;
        AutomationProperties.SetName(List, strings["overview.connections"]);
        SearchBox.PlaceholderText = strings["connections.search"];
        AutomationProperties.SetName(SearchBox, strings["connections.search"]);
        CloseAllButton.Content = strings["connections.close_all"];
    }

    /// <summary>
    /// Draws what is known now; null while the core has not said. The same connections in the same order keep
    /// their rows.
    /// </summary>
    public void Show(ConnectionsShown? shown)
    {
        if (shown is null)
        {
            // Nothing to search or close yet, and no sentence about it either.
            lines.Clear();
            CountText.Text = "";
            SearchBox.IsEnabled = SearchBox.Text.Length > 0;
            CloseAllButton.IsEnabled = false;
            NoticeText.Visibility = Visibility.Collapsed;
            List.Visibility = Visibility.Collapsed;
            return;
        }
        var rows = shown.Rows;
        var same = lines.Count == rows.Count && lines.Zip(rows).All(pair => pair.First.Row.Id == pair.Second.Id);
        if (same)
        {
            foreach (var (line, row) in lines.Zip(rows))
            {
                line.Row = row;
            }
        }
        else
        {
            lines.Clear();
            foreach (var row in rows)
            {
                lines.Add(new ConnectionLine(row, strings["connections.close"]));
            }
        }
        CountText.Text = shown.Count;
        // The search stays usable while it leaves nothing, so that it can be changed.
        SearchBox.IsEnabled = shown.CanSearch || SearchBox.Text.Length > 0;
        CloseAllButton.IsEnabled = shown.CanCloseAll;
        NoticeText.Text = shown.Notice ?? "";
        NoticeText.Visibility = shown.Notice is null ? Visibility.Collapsed : Visibility.Visible;
        List.Visibility = shown.Notice is null ? Visibility.Visible : Visibility.Collapsed;
    }

    private void OnContainerContentChanging(ListViewBase sender, ContainerContentChangingEventArgs args)
    {
        if (!args.InRecycleQueue && args.Item is ConnectionLine line)
        {
            // Hosts are shown and read as they are written.
            AutomationProperties.SetName(args.ItemContainer, line.Title);
        }
    }

    private void OnSearchChanged(object sender, TextChangedEventArgs e) => search(SearchBox.Text);

    private void OnClose(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is ConnectionLine { CanClose: true } line)
        {
            close(line.Row.Id);
        }
    }

    private void OnCloseAll(object sender, RoutedEventArgs e) => closeAll();
}
