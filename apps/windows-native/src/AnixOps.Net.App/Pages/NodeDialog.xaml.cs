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
/// One node over the proxies page: its name, server and port, the button that removes it, and its share
/// link with a QR code. What it says is decided in <see cref="Node"/>; this draws it and hands what the user
/// asks for to the page.
/// </summary>
public sealed partial class NodeDialog : ContentDialog
{
    // How many pixels a module of the QR code is wide in its picture; the picture is scaled to its place.
    private const int Module = 6;

    private readonly Strings strings;
    private readonly Action<NodeDraft> save;
    private readonly Action remove;

    // What the dialog shows now; null before the first draw.
    private NodeShown? shown;

    // Whether the removal is being asked about.
    private bool asking;
    private bool closingForGood;

    /// <param name="save">Called with what the user typed when it is to be saved.</param>
    /// <param name="remove">Called when the user said that the node is to be removed.</param>
    public NodeDialog(Strings strings, Action<NodeDraft> save, Action remove)
    {
        this.strings = strings;
        this.save = save;
        this.remove = remove;
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
