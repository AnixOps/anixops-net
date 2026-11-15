using System.Collections.Generic;
using System.Linq;
using System.Windows.Automation;
using AnixOps.Net.Presentation;
using Xunit;

[assembly: CollectionBehavior(DisableTestParallelization = true)]

namespace AnixOps.Net.App.UiTests;

/// <summary>The shell: the window opens, every page can be reached, and assistive technology can name what it finds.</summary>
public sealed class ShellTests
{
    private const string PageTitle = "Page.Title";

    public static IEnumerable<object[]> Languages() => Strings.Languages.Select(language => new object[] { language });

    public static IEnumerable<object[]> Looks() =>
        from language in Strings.Languages
        from theme in new[] { "light", "dark" }
        select new object[] { language, theme };

    [Theory]
    [MemberData(nameof(Languages))]
    public void EveryPageCanBeReachedFromTheSidebar(string language)
    {
        var strings = Strings.For(language);
        using var app = AppSession.Start("--language", language);
        app.WaitForName(PageTitle, strings[Sections.All[0].Title]);
        // Backwards, so that the first step already leaves the page the app opened on.
        foreach (var section in Sections.All.Reverse())
        {
            app.Select(section.AutomationId);
            app.WaitForName(PageTitle, strings[section.Title]);
        }
    }

    [Theory]
    [MemberData(nameof(Languages))]
    public void TheSidebarNamesEveryPage(string language)
    {
        var strings = Strings.For(language);
        using var app = AppSession.Start("--language", language);
        foreach (var section in Sections.All)
        {
            Assert.Equal(strings[section.Title], app.Find(section.AutomationId).Current.Name);
        }
    }

    [Fact]
    public void TheAppOpensOnThePageAskedFor()
    {
        var strings = Strings.For(Strings.English);
        using var app = AppSession.Start("--language", "en", "--page", Sections.Rules);
        app.WaitForName(PageTitle, strings["nav.rules"]);
    }

    [Fact]
    public void TheWindowIsNamedAfterTheApp()
    {
        using var app = AppSession.Start("--language", "en");
        Assert.Equal(Strings.For(Strings.English)["app.name"], app.Window.Current.Name);
    }

    [Theory]
    [MemberData(nameof(Languages))]
    public void EverythingThatTakesTheKeyboardHasAName(string language)
    {
        using var app = AppSession.Start("--language", language);
        app.WaitForName(PageTitle, Strings.For(language)[Sections.All[0].Title]);
        var unnamed = app.Elements()
            .Where(element => element.Current.IsKeyboardFocusable && element.Current.IsControlElement)
            .Where(element => IsOperable(element.Current.ControlType))
            .Where(element => string.IsNullOrWhiteSpace(element.Current.Name))
            .Select(element => $"{element.Current.ControlType.ProgrammaticName} '{element.Current.AutomationId}' ({element.Current.ClassName})")
            .ToList();
        Assert.True(unnamed.Count == 0, "without a name: " + string.Join(", ", unnamed));
    }

    [Theory]
    [MemberData(nameof(Languages))]
    public void TheRequestListNamesEverythingThatTakesTheKeyboard(string language)
    {
        var strings = Strings.For(language);
        using var app = AppSession.Start("--language", language, "--page", Sections.Activity, "--demo-rows", "300");
        app.WaitForName("Requests.Count", strings.Format("requests.count", 300, 300));
        Assert.Equal(strings["requests.filter"], app.Find("Requests.Filter").Current.Name);
        Assert.Equal(strings["requests.table"], app.Find("Requests.Table").Current.Name);
        var unnamed = app.Elements()
            .Where(element => element.Current.IsKeyboardFocusable && element.Current.IsControlElement)
            .Where(element => IsOperable(element.Current.ControlType))
            .Where(element => string.IsNullOrWhiteSpace(element.Current.Name))
            .Select(element => $"{element.Current.ControlType.ProgrammaticName} '{element.Current.AutomationId}' ({element.Current.ClassName})")
            .ToList();
        Assert.True(unnamed.Count == 0, "without a name: " + string.Join(", ", unnamed));
    }

    [Theory]
    [MemberData(nameof(Looks))]
    public void EveryPageIsPictured(string language, string theme)
    {
        var strings = Strings.For(language);
        foreach (var section in Sections.All)
        {
            // The request list is pictured with demo requests in it.
            using var app = AppSession.Start("--language", language, "--theme", theme, "--page", section.Id, "--demo-rows", "2000");
            app.WaitForName(PageTitle, strings[section.Title]);
            app.Screenshot($"{section.Id}-{language}-{theme}");
        }
    }

    private static bool IsOperable(ControlType type) =>
        type == ControlType.Button
        || type == ControlType.ListItem
        || type == ControlType.CheckBox
        || type == ControlType.ComboBox
        || type == ControlType.Edit
        || type == ControlType.Hyperlink
        || type == ControlType.MenuItem
        || type == ControlType.RadioButton
        || type == ControlType.Slider
        || type == ControlType.TabItem;
}
