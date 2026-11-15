using System.Collections.ObjectModel;
using System.ComponentModel;
using AnixOps.Net.Presentation;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;

namespace AnixOps.Net.App.Pages;

/// <summary>
/// One subscription's row in the open dialog. It stays the same object while the subscription stays in its
/// place, so that the keyboard focus stays on the button that was pressed when what the row says changes.
/// </summary>
public sealed class SubscriptionLine : INotifyPropertyChanged
{
    private SubscriptionRow row;

    public SubscriptionLine(SubscriptionRow row, string renameSays, string removeSays)
    {
        this.row = row;
        RenameSays = renameSays;
        RemoveSays = removeSays;
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    /// <summary>What the row shows now; its buttons read it when they are pressed.</summary>
    public SubscriptionRow Row
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

    public bool HasDetails => row.Details.Length > 0;

    public string Error => row.Error ?? "";

    public bool HasError => row.Error is not null;

    public string Update => row.Update ?? "";

    public bool CanUpdate => row.Update is not null;

    public bool Enabled => row.Enabled;

    /// <summary>What the buttons that show only a picture do, for the tooltip and a screen reader.</summary>
    public string RenameSays { get; }

    public string RemoveSays { get; }
}

/// <summary>
/// The list of subscriptions over the proxies page: one row per subscription, where a subscription is
/// downloaded again, given another name or removed. What the rows say is decided in
/// <see cref="Subscriptions"/>; this draws it and hands what the user asks for to the page.
/// </summary>
public sealed partial class SubscriptionsDialog : ContentDialog
{
    private readonly Strings strings;
    private readonly Action<SubscriptionChange> changed;
    private readonly ObservableCollection<SubscriptionLine> lines = [];

    // What the question on screen is about, and the button that asked it.
    private SubscriptionChangeKind? asking;
    private string askedOf = "";
    private Button? asker;

    /// <param name="changed">Called with what the user asks for on a subscription.</param>
    public SubscriptionsDialog(Strings strings, Action<SubscriptionChange> changed)
    {
        this.strings = strings;
        this.changed = changed;
        InitializeComponent();
        Title = strings["proxies.subscriptions"];
        CloseButtonText = strings["action.close"];
        DefaultButton = ContentDialogButton.None;
        List.ItemsSource = lines;
        AutomationProperties.SetName(List, strings["proxies.subscriptions"]);
        EmptyText.Text = strings["subscriptions.empty"];
        AutomationProperties.SetName(NameBox, strings["subscriptions.name"]);
        CancelButton.Content = strings["action.cancel"];
    }

    /// <summary>Draws what is known now. The same subscriptions in the same order keep their rows.</summary>
    public void Show(IReadOnlyList<SubscriptionRow> rows)
    {
        var same = lines.Count == rows.Count && lines.Zip(rows).All(pair => pair.First.Row.Id == pair.Second.Id);
        if (same)
        {
            foreach (var (line, row) in lines.Zip(rows))
            {
                line.Row = row;
                if (List.ContainerFromItem(line) is ListViewItem container)
                {
                    AutomationProperties.SetName(container, row.Title);
                }
            }
        }
        else
        {
            lines.Clear();
            foreach (var row in rows)
            {
                lines.Add(new SubscriptionLine(row, strings["subscriptions.rename"], strings["subscriptions.remove"]));
            }
        }
        // A question about a subscription that is gone, or that the core is now working on, is withdrawn.
        if (asking is not null && !rows.Any(row => row.Id == askedOf && row.Enabled))
        {
            Leave(refocus: false);
        }
        Arrange();
    }

    private void Arrange()
    {
        var question = asking is not null;
        Question.Visibility = question ? Visibility.Visible : Visibility.Collapsed;
        List.Visibility = !question && lines.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        EmptyText.Visibility = !question && lines.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        // While a question is asked, the way out is the question's own "cancel".
        CloseButtonText = question ? "" : strings["action.close"];
    }

    private void OnContainerContentChanging(ListViewBase sender, ContainerContentChangingEventArgs args)
    {
        if (!args.InRecycleQueue && args.Item is SubscriptionLine line)
        {
            AutomationProperties.SetName(args.ItemContainer, line.Title);
        }
    }

    private static SubscriptionLine? LineOf(object sender) => (sender as FrameworkElement)?.DataContext as SubscriptionLine;

    private void OnUpdate(object sender, RoutedEventArgs e)
    {
        if (LineOf(sender) is { Enabled: true } line)
        {
            changed(new SubscriptionChange(SubscriptionChangeKind.Update, line.Row.Id));
        }
    }

    private void OnRename(object sender, RoutedEventArgs e)
    {
        if (LineOf(sender) is not { Enabled: true } line)
        {
            return;
        }
        Ask(SubscriptionChangeKind.Rename, line, sender as Button);
        QuestionText.Text = strings["subscriptions.rename"];
        NameBox.Text = line.Row.Name;
        NameBox.Visibility = Visibility.Visible;
        ConfirmButton.Content = strings["subscriptions.save"];
        ConfirmButton.Style = (Style)Application.Current.Resources["AccentButtonStyle"];
        Arrange();
        NameBox.Focus(FocusState.Programmatic);
        NameBox.SelectAll();
    }

    private void OnRemove(object sender, RoutedEventArgs e)
    {
        if (LineOf(sender) is not { Enabled: true } line)
        {
            return;
        }
        Ask(SubscriptionChangeKind.Remove, line, sender as Button);
        QuestionText.Text = strings.Format("subscriptions.remove.confirm", line.Row.Title);
        NameBox.Visibility = Visibility.Collapsed;
        ConfirmButton.Content = strings["subscriptions.remove"];
        ConfirmButton.Style = (Style)Application.Current.Resources["DefaultButtonStyle"];
        Arrange();
        // Enter does not remove anything: the answer that is ready is the harmless one.
        CancelButton.Focus(FocusState.Programmatic);
    }

    private void Ask(SubscriptionChangeKind kind, SubscriptionLine line, Button? button)
    {
        asking = kind;
        askedOf = line.Row.Id;
        asker = button;
    }

    /// <summary>Takes the question away and shows the list again.</summary>
    private void Leave(bool refocus)
    {
        asking = null;
        Arrange();
        if (refocus && asker is { IsEnabled: true } button)
        {
            button.Focus(FocusState.Programmatic);
        }
        asker = null;
    }

    private void OnConfirm(object sender, RoutedEventArgs e)
    {
        if (asking is not { } kind)
        {
            return;
        }
        if (kind == SubscriptionChangeKind.Rename)
        {
            var name = NameBox.Text.Trim();
            // A name of nothing is no name; the core would say so too.
            if (name.Length == 0)
            {
                NameBox.Focus(FocusState.Programmatic);
                return;
            }
            Leave(refocus: true);
            changed(new SubscriptionChange(kind, askedOf, name));
        }
        else
        {
            Leave(refocus: false);
            changed(new SubscriptionChange(kind, askedOf));
        }
    }

    private void OnCancel(object sender, RoutedEventArgs e) => Leave(refocus: true);

    private void OnNameKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == Windows.System.VirtualKey.Enter)
        {
            e.Handled = true;
            OnConfirm(sender, e);
        }
    }

    private void OnClosing(ContentDialog sender, ContentDialogClosingEventArgs args)
    {
        // Escape while a question is asked takes the question back, not the whole list away.
        if (asking is not null && !closingForGood)
        {
            args.Cancel = true;
            Leave(refocus: true);
        }
    }

    private bool closingForGood;

    /// <summary>Closes the dialog whatever it shows: what it lists belongs to a core that is gone.</summary>
    public void Close()
    {
        closingForGood = true;
        Hide();
    }
}
