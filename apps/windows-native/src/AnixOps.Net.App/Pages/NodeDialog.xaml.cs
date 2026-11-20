using System.Runtime.InteropServices;
using System.Runtime.InteropServices.WindowsRuntime;
using AnixOps.Net.Presentation;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.ApplicationModel.DataTransfer;

namespace AnixOps.Net.App.Pages;

/// <summary>
/// One node over the proxies page: its name, server and port, the button that removes it, the node checkup,
/// and its share link with a QR code. What it says is decided in <see cref="Node"/>; this draws it and hands what the user
/// asks for to the page.
/// </summary>
public sealed partial class NodeDialog : ContentDialog
{
    // How many pixels a module of the QR code is wide in its picture; the picture is scaled to its place.
    private const int Module = 6;

    private readonly Strings strings;
    private readonly Action<NodeDraft> save;
    private readonly Action remove;
    private readonly Action<bool> checkup;

    // The checkup as last drawn: whether it runs, whether it ran before, and what it showed; the page draws
    // every second and the results are only built again when they changed.
    private bool checking;
    private bool checkedOnce;
    private bool checkupDrawn;
    private IReadOnlyList<CheckupSection>? drawnSections;
    private string? drawnProblem;

    // What the dialog shows now; null before the first draw.
    private NodeShown? shown;

    // Whether the removal is being asked about.
    private bool asking;
    private bool closingForGood;

    /// <param name="save">Called with what the user typed when it is to be saved.</param>
    /// <param name="remove">Called when the user said that the node is to be removed.</param>
    /// <param name="checkup">Called when the user starts a checkup; true adds the download.</param>
    public NodeDialog(Strings strings, Action<NodeDraft> save, Action remove, Action<bool> checkup)
    {
        this.strings = strings;
        this.save = save;
        this.remove = remove;
        this.checkup = checkup;
        InitializeComponent();
        CloseButtonText = strings["action.close"];
        DefaultButton = ContentDialogButton.None;
        Label(NameBox, "node.name");
        Label(ServerBox, "node.server");
        Label(PortBox, "node.port");
        SaveButton.Content = strings["action.save"];
        RemoveButton.Content = strings["node.remove"];
        ShareHeading.Text = strings["node.share"];
        NoLinkText.Text = strings["node.share.none"];
        AutomationProperties.SetName(QrImage, strings["node.qr"]);
        CopyButton.Content = strings["node.copy"];
        QuestionNote.Text = strings["node.edit.note"];
        ConfirmButton.Content = strings["subscriptions.remove"];
        CancelButton.Content = strings["action.cancel"];
        CheckupHeading.Text = strings["node.checkup"];
        ThroughputBox.Content = strings["checkup.throughput"];
        AutomationProperties.SetName(CheckupRing, strings["checkup.running"]);
        ShowCheckup(running: false, sections: null, problem: null);
    }

    /// <summary>
    /// Draws the checkup: whether it runs, what it found (null before it ran) and why it did not work.
    /// </summary>
    public void ShowCheckup(bool running, IReadOnlyList<CheckupSection>? sections, string? problem)
    {
        if (checkupDrawn && running == checking && ReferenceEquals(sections, drawnSections) && problem == drawnProblem)
        {
            return;
        }
        checkupDrawn = true;
        drawnSections = sections;
        drawnProblem = problem;
        checking = running;
        checkedOnce |= sections is not null || problem is not null;
        CheckupNote.Text = Checkup.Note(strings, ThroughputBox.IsChecked == true);
        ThroughputBox.IsEnabled = !running;
        CheckupButton.IsEnabled = !running;
        CheckupButton.Content = strings[running ? "checkup.running" : checkedOnce ? "checkup.again" : "checkup.start"];
        CheckupRing.IsActive = running;
        CheckupRing.Visibility = running ? Visibility.Visible : Visibility.Collapsed;
        Write(CheckupProblem, problem);
        CheckupResults.Children.Clear();
        foreach (var section in sections ?? Array.Empty<CheckupSection>())
        {
            CheckupResults.Children.Add(new TextBlock
            {
                Text = section.Heading,
                Margin = new Thickness(0, 4, 0, 0),
                Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"],
            });
            foreach (var line in section.Lines)
            {
                var row = new Grid { ColumnSpacing = 12 };
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
                var label = new TextBlock { Text = line.Label, TextWrapping = TextWrapping.Wrap };
                // Marked values take the colour of a problem; the others that of a note.
                var value = new TextBlock
                {
                    Text = line.Value,
                    IsTextSelectionEnabled = true,
                    Foreground = line.Warn ? CheckupProblem.Foreground : NoteText.Foreground,
                };
                Grid.SetColumn(value, 1);
                AutomationProperties.SetName(value, $"{line.Label} {line.Value}");
                row.Children.Add(label);
                row.Children.Add(value);
                CheckupResults.Children.Add(row);
            }
            if (section.Note is { } note)
            {
                CheckupResults.Children.Add(new TextBlock
                {
                    Text = note,
                    Foreground = NoteText.Foreground,
                    Style = (Style)Application.Current.Resources["CaptionTextBlockStyle"],
                    TextWrapping = TextWrapping.Wrap,
                });
            }
        }
    }

    private void OnThroughput(object sender, RoutedEventArgs e) =>
        CheckupNote.Text = Checkup.Note(strings, ThroughputBox.IsChecked == true);

    private void OnCheckup(object sender, RoutedEventArgs e)
    {
        if (!checking)
        {
            checkup(ThroughputBox.IsChecked == true);
        }
    }

    private void Label(TextBox box, string wording)
    {
        box.Header = strings[wording];
        AutomationProperties.SetName(box, strings[wording]);
    }

    /// <summary>
    /// Draws what is known now. The boxes are only written when the node behind them changed: what the user
    /// is typing is not taken away by an answer about something else.
    /// </summary>
    public void Show(NodeShown now)
    {
        var before = shown;
        if (before == now)
        {
            return;
        }
        var first = before is null || before.Id != now.Id;
        shown = now;
        if (first || before!.Draft != now.Draft)
        {
            NameBox.Text = now.Draft.Name;
            ServerBox.Text = now.Draft.Server;
            PortBox.Text = now.Draft.Port;
        }
        Title = now.Title;
        Write(ProtocolText, now.Protocol.Length > 0 ? now.Protocol : null);
        foreach (var box in new[] { NameBox, ServerBox, PortBox })
        {
            box.IsReadOnly = !now.Editable;
        }
        NoteText.Text = now.Note;
        Write(ProblemText, now.Problem);
        Write(KeepText, now.Keep);
        Buttons.Visibility = now.Editable ? Visibility.Visible : Visibility.Collapsed;
        SaveButton.IsEnabled = now.CanSave;
        RemoveButton.IsEnabled = now.CanRemove;

        Write(LinkText, now.Link);
        NoLinkText.Visibility = now.Link is null ? Visibility.Visible : Visibility.Collapsed;
        CopyButton.Visibility = now.Link is null ? Visibility.Collapsed : Visibility.Visible;
        if (first || before!.Link != now.Link)
        {
            CopyButton.Content = strings["node.copy"];
        }
        if (first || !SameRows(before!.Qr, now.Qr))
        {
            QrImage.Source = now.Qr is { } rows ? Picture(rows) : null;
        }
        QrImage.Visibility = now.Qr is null ? Visibility.Collapsed : Visibility.Visible;

        // A question about a node that can no longer be removed is withdrawn.
        if (asking && !now.CanRemove)
        {
            Leave(refocus: false);
        }
        Arrange();
    }

    private static bool SameRows(IReadOnlyList<string>? one, IReadOnlyList<string>? other) =>
        one is null ? other is null : other is not null && one.SequenceEqual(other);

    private static WriteableBitmap Picture(IReadOnlyList<string> rows)
    {
        var (side, pixels) = Node.Picture(rows, Module);
        var bitmap = new WriteableBitmap(side, side);
        using (var stream = bitmap.PixelBuffer.AsStream())
        {
            stream.Write(pixels, 0, pixels.Length);
        }
        bitmap.Invalidate();
        return bitmap;
    }

    private void Arrange()
    {
        Question.Visibility = asking ? Visibility.Visible : Visibility.Collapsed;
        Form.Visibility = asking ? Visibility.Collapsed : Visibility.Visible;
        // While the question is asked, the way out is the question's own "cancel".
        CloseButtonText = asking ? "" : strings["action.close"];
    }

    private NodeDraft Typed() => new(NameBox.Text, ServerBox.Text, PortBox.Text);

    private void OnSave(object sender, RoutedEventArgs e)
    {
        if (shown is { CanSave: true })
        {
            save(Typed());
        }
    }

    // Enter in a box saves, as the button does.
    private void OnBoxKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == Windows.System.VirtualKey.Enter)
        {
            e.Handled = true;
            OnSave(sender, e);
        }
    }

    private void OnRemove(object sender, RoutedEventArgs e)
    {
        if (shown is not { CanRemove: true } node)
        {
            return;
        }
        asking = true;
        QuestionText.Text = strings.Format("node.remove.confirm", node.Title);
        Arrange();
        // Enter does not remove anything: the answer that is ready is the harmless one.
        CancelButton.Focus(FocusState.Programmatic);
    }

    private void OnConfirm(object sender, RoutedEventArgs e)
    {
        if (!asking)
        {
            return;
        }
        Leave(refocus: false);
        if (shown is { CanRemove: true })
        {
            remove();
        }
    }

    private void OnCancel(object sender, RoutedEventArgs e) => Leave(refocus: true);

    /// <summary>Takes the question away and shows the node again.</summary>
    private void Leave(bool refocus)
    {
        asking = false;
        Arrange();
        if (refocus && RemoveButton.IsEnabled)
        {
            RemoveButton.Focus(FocusState.Programmatic);
        }
    }

    private void OnCopy(object sender, RoutedEventArgs e)
    {
        if (shown?.Link is not { } link)
        {
            return;
        }
        try
        {
            var package = new DataPackage();
            package.SetText(link);
            Clipboard.SetContent(package);
            // Kept after this window is gone.
            Clipboard.Flush();
        }
        catch (Exception problem) when (problem is COMException or UnauthorizedAccessException)
        {
            // Another program holds the clipboard: the button does not say "copied", and the link can
            // still be selected and copied by hand.
            return;
        }
        CopyButton.Content = strings["node.copied"];
    }

    private void OnClosing(ContentDialog sender, ContentDialogClosingEventArgs args)
    {
        // Escape while the question is asked takes the question back, not the whole dialog away.
        if (asking && !closingForGood)
        {
            args.Cancel = true;
            Leave(refocus: true);
        }
    }

    /// <summary>Closes the dialog whatever it shows: the node is gone, or the core that described it is.</summary>
    public void Close()
    {
        closingForGood = true;
        Hide();
    }

    private static void Write(TextBlock block, string? text)
    {
        block.Text = text ?? "";
        block.Visibility = text is null ? Visibility.Collapsed : Visibility.Visible;
    }
}
