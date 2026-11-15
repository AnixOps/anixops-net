using System;
using System.IO;
using System.Linq;
using System.Text.Json;
using AnixOps.Net.Presentation;
using Xunit;
using Xunit.Abstractions;

namespace AnixOps.Net.App.UiTests;

/// <summary>The request list, and the first pass mark of docs/architecture/desktop-native.md ("过关标准" 1).</summary>
public sealed class RequestsTests
{
    private const string Count = "Requests.Count";

    /// <summary>The most a filter may take to show, in milliseconds.</summary>
    private const double FilterBudget = 100;

    private readonly ITestOutputHelper output;

    public RequestsTests(ITestOutputHelper output)
    {
        this.output = output;
    }

    [Fact]
    public void TheListSaysHowManyRequestsItShows()
    {
        var strings = Strings.For(Strings.English);
        using var app = AppSession.Start("--language", "en", "--page", Sections.Activity, "--demo-rows", "5000");
        app.WaitForName(Count, strings.Format("requests.count", 5000, 5000));
    }

    [Fact]
    public void WithoutRequestsTheListSaysSo()
    {
        var strings = Strings.For(Strings.English);
        using var app = AppSession.Start("--language", "en", "--page", Sections.Activity);
        app.WaitForName(Count, strings.Format("requests.count", 0, 0));
        app.WaitForName("Requests.Empty", strings["requests.empty"]);
    }

    [Theory]
    [InlineData("example.org 404")]
    [InlineData("POST")]
    [InlineData("no-such-request")]
    public void TypingAFilterNarrowsTheList(string filter)
    {
        const int Rows = 5000;
        var strings = Strings.For(Strings.English);
        var expected = RequestFilter.Apply(DemoRequests.Make(Rows), filter).Count;
        using var app = AppSession.Start("--language", "en", "--page", Sections.Activity, "--demo-rows", "5000");
        app.WaitForName(Count, strings.Format("requests.count", Rows, Rows));
        app.SetText("Requests.Filter", filter);
        app.WaitForName(Count, strings.Format("requests.count", expected, Rows));
        if (expected == 0)
        {
            app.WaitForName("Requests.Empty", strings["requests.none"]);
        }
        // Clearing the filter brings everything back.
        app.SetText("Requests.Filter", "");
        app.WaitForName(Count, strings.Format("requests.count", Rows, Rows));
    }

    [Fact]
    public void RowsAreReadOutAsOneSentence()
    {
        var rows = DemoRequests.Make(200);
        using var app = AppSession.Start("--language", "en", "--page", Sections.Activity, "--demo-rows", "200");
        app.WaitForName(Count, Strings.For(Strings.English).Format("requests.count", 200, 200));
        var names = app.Elements()
            .Where(element => element.Current.ControlType == System.Windows.Automation.ControlType.ListItem)
            .Select(element => element.Current.Name)
            .ToList();
        Assert.Contains(rows[0].ToString(), names);
        Assert.Contains(rows[1].ToString(), names);
    }

    [Fact]
    public void FiftyThousandRequestsFilterWithinBudgetAndScrollWithoutBlankRows()
    {
        const int Rows = 50_000;
        var file = Path.Combine(AppSession.KeepDirectory() ?? Path.GetTempPath(), "requests-measurement.json");
        File.Delete(file);
        using (var app = AppSession.Start(
            "--language", "en", "--page", Sections.Activity, "--demo-rows", "50000", "--measure", file))
        {
            app.WaitForExit(TimeSpan.FromMinutes(3));
        }
        var text = File.ReadAllText(file);
        output.WriteLine(text);
        var measured = JsonSerializer.Deserialize<Measurement>(text) ?? throw new InvalidDataException("no measurement");

        Assert.Null(measured.Error);
        Assert.Equal(Rows, measured.Rows);

        var all = DemoRequests.Make(Rows);
        Assert.NotEmpty(measured.Filters);
        foreach (var sample in measured.Filters)
        {
            Assert.Equal(RequestFilter.Apply(all, sample.Text).Count, sample.Matches);
            Assert.True(
                sample.Milliseconds <= FilterBudget,
                $"filter '{sample.Text}' took {sample.Milliseconds:0.0} ms to show, over {FilterBudget:0} ms");
        }
        Assert.Contains(measured.Filters, sample => sample.Matches == Rows);
        Assert.Contains(measured.Filters, sample => sample.Matches == 0);

        var scroll = measured.Scroll ?? throw new InvalidDataException("the scrolling sweep did not run");
        Assert.True(scroll.Steps >= 200, $"only {scroll.Steps} moves");
        Assert.True(scroll.BlankRows == 0, $"{scroll.BlankRows} rows inside the viewport were blank on the frame after a move");
    }
}
