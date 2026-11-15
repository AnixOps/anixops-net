using System.Diagnostics;
using System.Text.Json;
using AnixOps.Net.Presentation;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;

namespace AnixOps.Net.App.Pages;

/// <summary>The request list: a filter box over a virtualising table.</summary>
public sealed partial class RequestsPage : Page
{
    /// <summary>What the page shows.</summary>
    /// <param name="Strings">The interface language's wording.</param>
    /// <param name="Rows">The requests, oldest first.</param>
    /// <param name="Demo">Whether the requests are made up.</param>
    /// <param name="Measure">A file to write the list's timings to, after which the app exits; or null.</param>
    public sealed record Model(Strings Strings, IReadOnlyList<RequestRow> Rows, bool Demo, string? Measure);

    private IReadOnlyList<RequestRow> all = [];
    private IReadOnlyList<RequestRow> shown = [];
    private Strings? strings;
    private string shownFilter = "";

    public RequestsPage()
    {
        InitializeComponent();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        if (e.Parameter is not Model model)
        {
            return;
        }
        strings = model.Strings;
        all = model.Rows;

        Heading.Text = strings["nav.activity"];
        Notice.Message = strings[model.Demo ? "requests.demo" : "page.unfinished"];
        FilterBox.PlaceholderText = strings["requests.filter"];
        AutomationProperties.SetName(FilterBox, strings["requests.filter"]);
        AutomationProperties.SetName(Table, strings["requests.table"]);
        SeqHeader.Text = strings["column.seq"];
        MethodHeader.Text = strings["column.method"];
        StatusHeader.Text = strings["column.status"];
        HostHeader.Text = strings["column.host"];
        PathHeader.Text = strings["column.path"];
        TypeHeader.Text = strings["column.type"];
        SizeHeader.Text = strings["column.size"];
        DurationHeader.Text = strings["column.duration"];
        Show("");

        if (model.Measure is { } file)
        {
            Loaded += async (_, _) => await MeasureAsync(file);
        }
    }

    private void OnFilterChanged(object sender, TextChangedEventArgs e) => Show(FilterBox.Text);

    private void Show(string filter)
    {
        if (strings is null)
        {
            return;
        }
        var rows = RequestFilter.Apply(all, filter);
        Table.ItemsSource = rows;
        shown = rows;
        CountText.Text = strings.Format("requests.count", rows.Count, all.Count);
        Empty.Text = strings[all.Count == 0 ? "requests.empty" : "requests.none"];
        Empty.Visibility = rows.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        shownFilter = filter;
    }

    // A row is read out as one sentence instead of eight separate texts.
    private void OnContainerContentChanging(ListViewBase sender, ContainerContentChangingEventArgs args)
    {
        if (!args.InRecycleQueue && args.Item is RequestRow row)
        {
            AutomationProperties.SetName(args.ItemContainer, row.ToString());
        }
    }

    // Everything below measures the list for docs/architecture/desktop-native.md ("过关标准" 1): how long a
    // filter takes to show, and whether any row inside the viewport is blank on the frame after a move.

    private static readonly string[] MeasuredFilters = ["api", "api 404", "example.org", "no-such-request", "users json", ""];
    private const int ScrollSteps = 240;
    private static readonly JsonSerializerOptions Indented = new() { WriteIndented = true };

    private async Task MeasureAsync(string file)
    {
        var filters = new List<FilterSample>();
        ScrollSample? scroll = null;
        string? error = null;
        try
        {
            await FramesAsync(5);
            foreach (var text in MeasuredFilters)
            {
                // Typing the same text again changes nothing, so start from something else.
                if (FilterBox.Text == text)
                {
                    FilterBox.Text = "example";
                    await UntilAsync(() => shownFilter == "example");
                    await FramesAsync(2);
                }
                var watch = Stopwatch.StartNew();
                FilterBox.Text = text;
                await UntilAsync(() => shownFilter == text);
                await FramesAsync(2);
                watch.Stop();
                filters.Add(new FilterSample(text, shown.Count, watch.Elapsed.TotalMilliseconds));
            }
            scroll = await SweepAsync();
        }
        catch (Exception problem)
        {
            error = problem.ToString();
        }
        var directory = Path.GetDirectoryName(Path.GetFullPath(file));
        if (!string.IsNullOrEmpty(directory))
        {
            Directory.CreateDirectory(directory);
        }
        File.WriteAllText(file, JsonSerializer.Serialize(new Measurement(all.Count, filters, scroll, error), Indented));
        Application.Current.Exit();
    }

    private async Task<ScrollSample> SweepAsync()
    {
        var rows = shown;
        var viewer = Descendant<ScrollViewer>(Table) ?? throw new InvalidOperationException("the list has no scroll viewer");
        var first = Table.ContainerFromIndex(0) as ListViewItem ?? throw new InvalidOperationException("the first row is not shown");
        var rowHeight = first.ActualHeight;
        var viewport = viewer.ViewportHeight;
        var range = viewer.ScrollableHeight;
        if (rowHeight <= 0 || viewport <= 0 || range <= 0)
        {
            throw new InvalidOperationException($"nothing to scroll: row {rowHeight}, viewport {viewport}, range {range}");
        }

        var blank = 0;
        var worst = 0.0;
        var state = 0x2545F4914F6CDD1DUL;
        for (var step = 0; step < ScrollSteps; step++)
        {
            // Half the moves page down from the top, as the wheel and Page Down do; the other half jump
            // anywhere, as dragging the scroll bar does.
            double target;
            if (step < ScrollSteps / 2)
            {
                target = Math.Min(range, (step + 1) * viewport * 0.9);
            }
            else
            {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                target = Math.Floor(state % 1_000_000 / 1_000_000.0 * range);
            }
            if (Math.Abs(viewer.VerticalOffset - target) < 1)
            {
                continue;
            }
            var watch = Stopwatch.StartNew();
            viewer.ChangeView(null, target, null, disableAnimation: true);
            await UntilAsync(() => Math.Abs(viewer.VerticalOffset - target) < 1);
            await FramesAsync(1);
            watch.Stop();
            worst = Math.Max(worst, watch.Elapsed.TotalMilliseconds);

            var top = (int)Math.Floor(viewer.VerticalOffset / rowHeight);
            var bottom = Math.Min(rows.Count - 1, (int)Math.Floor((viewer.VerticalOffset + viewport - 1) / rowHeight));
            for (var index = top; index <= bottom; index++)
            {
                if (Table.ContainerFromIndex(index) is not ListViewItem { ContentTemplateRoot: not null })
                {
                    blank++;
                }
            }
        }
        return new ScrollSample(ScrollSteps, blank, worst, rowHeight, viewport);
    }

    /// <summary>Completes after this many frames have been prepared.</summary>
    private static Task FramesAsync(int count)
    {
        var done = new TaskCompletionSource();
        void OnRendering(object? sender, object e)
        {
            if (--count <= 0)
            {
                CompositionTarget.Rendering -= OnRendering;
                done.TrySetResult();
            }
        }
        CompositionTarget.Rendering += OnRendering;
        return done.Task;
    }

    /// <summary>Completes on the first frame on which the condition holds; fails after ten seconds.</summary>
    private static Task UntilAsync(Func<bool> condition)
    {
        var done = new TaskCompletionSource();
        var watch = Stopwatch.StartNew();
        void OnRendering(object? sender, object e)
        {
            if (condition())
            {
                CompositionTarget.Rendering -= OnRendering;
                done.TrySetResult();
            }
            else if (watch.Elapsed > TimeSpan.FromSeconds(10))
            {
                CompositionTarget.Rendering -= OnRendering;
                done.TrySetException(new TimeoutException("the list did not settle within ten seconds"));
            }
        }
        CompositionTarget.Rendering += OnRendering;
        return done.Task;
    }

    private static T? Descendant<T>(DependencyObject root)
        where T : DependencyObject
    {
        for (var index = 0; index < VisualTreeHelper.GetChildrenCount(root); index++)
        {
            var child = VisualTreeHelper.GetChild(root, index);
            if (child is T found)
            {
                return found;
            }
            if (Descendant<T>(child) is { } deeper)
            {
                return deeper;
            }
        }
        return null;
    }
}
