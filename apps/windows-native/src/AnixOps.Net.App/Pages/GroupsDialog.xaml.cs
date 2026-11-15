using System.Collections.ObjectModel;
using System.ComponentModel;
using AnixOps.Net.Presentation;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;

namespace AnixOps.Net.App.Pages;

/// <summary>
/// One group's row in the open dialog. It stays the same object while the group stays in its place, so that
/// the keyboard focus stays on its button when what the row says changes.
/// </summary>
public sealed class GroupLine : INotifyPropertyChanged
{
    private GroupRow row;

    public GroupLine(GroupRow row, string chooseSays)
    {
        this.row = row;
        ChooseSays = chooseSays;
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    /// <summary>What the row shows now; opening its members reads it.</summary>
    public GroupRow Row
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

    public string Title => row.Tag;

    public string Details => row.Details;

    /// <summary>Whether the user chooses the member of this group.</summary>
    public bool CanChoose => row.Options.Count > 0;

    /// <summary>What the button that shows only a picture does, for the tooltip and a screen reader.</summary>
    public string ChooseSays { get; }
}

/// <summary>
/// The policy groups over the proxies page: one row per group, and the members of the group that was opened.
/// What the rows say is decided in <see cref="Groups"/>; this draws it and hands the member the user chooses
/// to the page.
/// </summary>
public sealed partial class GroupsDialog : ContentDialog
{
    private readonly Strings strings;
    private readonly Action<GroupChoice> chosen;
    private readonly ObservableCollection<GroupLine> lines = [];

    // The group whose members are on screen, and the button that opened them.
    private GroupRow? opened;
    private Button? opener;

    // Set while the dialog itself moves the selection, so that this is not taken for the user's choice.
    private bool drawing;

    /// <param name="chosen">Called with the member the user chooses for a group.</param>
    public GroupsDialog(Strings strings, Action<GroupChoice> chosen)
    {
        this.strings = strings;
        this.chosen = chosen;
        InitializeComponent();
        Title = strings["proxies.groups"];
        CloseButtonText = strings["action.close"];
        DefaultButton = ContentDialogButton.None;
        List.ItemsSource = lines;
        AutomationProperties.SetName(List, strings["proxies.groups"]);
        EmptyText.Text = strings["groups.empty"];
        AutomationProperties.SetName(BackButton, strings["action.back"]);
        ToolTipService.SetToolTip(BackButton, strings["action.back"]);
    }

    /// <summary>Draws what is known now. The same groups with the same members keep their rows.</summary>
    public void Show(IReadOnlyList<GroupRow> rows)
    {
        var same = lines.Count == rows.Count && lines.Zip(rows).All(pair => pair.First.Row.SameShape(pair.Second));
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
                lines.Add(new GroupLine(row, strings["groups.choose"]));
            }
        }
        if (opened is { } group)
        {
            // The members on screen follow their group, and go when the group is no longer the one that
            // was opened.
            if (rows.FirstOrDefault(group.SameShape) is { } now)
            {
                opened = now;
                // While the core is changing the group the member the user chose stays selected, whatever
                // the core still says is in use.
                if (now.Enabled)
                {
                    Select(now.Chosen);
                }
            }
            else
            {
                Leave(refocus: false);
            }
        }
        Arrange();
    }

    private void Select(int? place)
    {
        var at = place ?? -1;
        if (MemberList.SelectedIndex != at)
        {
            drawing = true;
            MemberList.SelectedIndex = at;
            drawing = false;
        }
    }

    private void Arrange()
    {
        var members = opened is not null;
        Members.Visibility = members ? Visibility.Visible : Visibility.Collapsed;
        List.Visibility = !members && lines.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        EmptyText.Visibility = !members && lines.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    private void OnContainerContentChanging(ListViewBase sender, ContainerContentChangingEventArgs args)
    {
        if (!args.InRecycleQueue && args.Item is GroupLine line)
        {
            AutomationProperties.SetName(args.ItemContainer, line.Title);
        }
    }

    private void OnChoose(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is not GroupLine { CanChoose: true } line)
        {
            return;
        }
        opened = line.Row;
        opener = sender as Button;
        GroupText.Text = line.Row.Tag;
        AutomationProperties.SetName(MemberList, line.Row.Tag);
        drawing = true;
        // Names are shown as they are written.
        MemberList.ItemsSource = line.Row.Options.Select(member => member.Label).ToList();
        MemberList.SelectedIndex = line.Row.Chosen ?? -1;
        drawing = false;
        Arrange();
        MemberList.Focus(FocusState.Programmatic);
    }

    private void OnMemberChosen(object sender, SelectionChangedEventArgs e)
    {
        if (drawing || opened is not { } group)
        {
            return;
        }
        var at = MemberList.SelectedIndex;
        // Choosing the member that is in use asks for nothing.
        if (at < 0 || at >= group.Options.Count || at == group.Chosen)
        {
            return;
        }
        chosen(new GroupChoice(group.Tag, group.Options[at].Id));
    }

    /// <summary>Takes the members away and shows the groups again.</summary>
    private void Leave(bool refocus)
    {
        opened = null;
        drawing = true;
        MemberList.ItemsSource = null;
        drawing = false;
        Arrange();
        if (refocus && opener is { } button)
        {
            button.Focus(FocusState.Programmatic);
        }
        opener = null;
    }

    private void OnBack(object sender, RoutedEventArgs e) => Leave(refocus: true);

    /// <summary>Closes the dialog whatever it shows: what it lists belongs to a core that is gone.</summary>
    public void Close() => Hide();
}
