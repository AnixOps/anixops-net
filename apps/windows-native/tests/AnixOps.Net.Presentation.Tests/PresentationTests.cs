using Xunit;

namespace AnixOps.Net.Presentation.Tests;

public sealed class StringsTests
{
    [Fact]
    public void EveryLanguageHasTheSameKeys()
    {
        var english = Strings.For(Strings.English).Keys.Order().ToArray();
        Assert.NotEmpty(english);
        foreach (var language in Strings.Languages)
        {
            Assert.Equal(english, Strings.For(language).Keys.Order().ToArray());
        }
    }

    [Fact]
    public void TheThreeLanguagesAreOffered()
    {
        Assert.Equal(new[] { "zh", "zh-Hant", "en" }, Strings.Languages);
        Assert.All(Strings.Languages, language => Assert.Equal(language, Strings.For(language).Language));
    }

    [Fact]
    public void TraditionalChineseHasItsOwnWording()
    {
        Assert.Equal("设置", Strings.For(Strings.Chinese)["nav.settings"]);
        Assert.Equal("設定", Strings.For(Strings.TraditionalChinese)["nav.settings"]);
        Assert.Equal("Settings", Strings.For(Strings.English)["nav.settings"]);
    }

    [Theory]
    [InlineData("[]")]
    [InlineData("""{"a": "text"}""")]
    [InlineData("""{"a": {"zh": "一", "en": "one"}}""")]
    [InlineData("""{"a": {"zh": "一", "zh-Hant": "一", "en": 1}}""")]
    [InlineData("""{"a": {"zh": "一", "zh-Hant": "一", "en": "one", "fr": "un"}}""")]
    public void ABrokenCatalogueIsRefused(string catalogue)
    {
        using var stream = new MemoryStream(System.Text.Encoding.UTF8.GetBytes(catalogue));
        Assert.Throws<InvalidDataException>(() => Strings.Read(stream));
    }

    [Fact]
    public void NoWordingIsEmpty()
    {
        foreach (var language in Strings.Languages)
        {
            var strings = Strings.For(language);
            Assert.All(strings.Keys, key => Assert.False(string.IsNullOrWhiteSpace(strings[key]), $"{language}: {key}"));
        }
    }

    [Fact]
    public void AMissingKeyIsAnError()
    {
        Assert.Throws<KeyNotFoundException>(() => Strings.For(Strings.English)["no.such.key"]);
    }

    [Theory]
    [InlineData("zh-CN", Strings.Chinese)]
    [InlineData("zh-Hans-CN", Strings.Chinese)]
    [InlineData("zh", Strings.Chinese)]
    [InlineData("zh-Hans-HK", Strings.Chinese)]
    [InlineData("ZH-TW", Strings.TraditionalChinese)]
    [InlineData("zh-HK", Strings.TraditionalChinese)]
    [InlineData("zh-MO", Strings.TraditionalChinese)]
    [InlineData("zh-Hant", Strings.TraditionalChinese)]
    [InlineData("zh-Hant-CN", Strings.TraditionalChinese)]
    [InlineData("zh-CHT", Strings.TraditionalChinese)]
    [InlineData("zh-Hans-TW", Strings.Chinese)]
    [InlineData("zh-CHS", Strings.Chinese)]
    [InlineData("en-US", Strings.English)]
    [InlineData("de-DE", Strings.English)]
    [InlineData("zhx", Strings.English)]
    [InlineData("", Strings.English)]
    [InlineData(null, Strings.English)]
    public void TheLanguageFollowsTheSystemCulture(string? culture, string expected)
    {
        Assert.Equal(expected, Strings.Preferred(culture));
    }

    [Fact]
    public void AnUnknownLanguageIsEnglish()
    {
        Assert.Equal(Strings.English, Strings.For("fr").Language);
    }
}

public sealed class SectionsTests
{
    [Fact]
    public void EverySectionHasWordingInEveryLanguage()
    {
        foreach (var language in Strings.Languages)
        {
            var strings = Strings.For(language);
            foreach (var section in Sections.All)
            {
                Assert.NotEmpty(strings[section.Title]);
                Assert.NotEmpty(strings[section.Summary]);
            }
        }
    }

    [Fact]
    public void IdsAreUnique()
    {
        Assert.Equal(Sections.All.Count, Sections.All.Select(section => section.Id).Distinct().Count());
    }

    [Fact]
    public void SettingsSitsAtTheBottom()
    {
        var footer = Assert.Single(Sections.All, section => section.Footer);
        Assert.Equal(Sections.Settings, footer.Id);
        Assert.Same(footer, Sections.All[^1]);
    }

    [Fact]
    public void FindKnowsEveryIdAndNothingElse()
    {
        Assert.All(Sections.All, section => Assert.Same(section, Sections.Find(section.Id)));
        Assert.Null(Sections.Find("nowhere"));
        Assert.Null(Sections.Find(null));
    }
}

public sealed class LaunchTests
{
    [Fact]
    public void NothingAskedFollowsTheSystem()
    {
        Assert.Equal(new Launch(null, Appearance.System, Sections.Overview), Launch.Parse([]));
    }

    [Fact]
    public void OptionsTakeTheNextArgument()
    {
        var launch = Launch.Parse(["--language", "en", "--theme", "dark", "--page", "rules"]);
        Assert.Equal(new Launch(Strings.English, Appearance.Dark, Sections.Rules), launch);
    }

    [Fact]
    public void OptionsTakeAnEqualsSign()
    {
        var launch = Launch.Parse(["--language=zh", "--theme=light", "--page=settings"]);
        Assert.Equal(new Launch(Strings.Chinese, Appearance.Light, Sections.Settings), launch);
    }

    [Theory]
    [InlineData("--language", "fr")]
    [InlineData("--theme", "purple")]
    [InlineData("--page", "nowhere")]
    [InlineData("--unknown", "value")]
    public void WhatIsNotUnderstoodIsIgnored(string name, string value)
    {
        Assert.Equal(Launch.Parse([]), Launch.Parse([name, value]));
        Assert.Equal(Launch.Parse([]), Launch.Parse([$"{name}={value}"]));
    }

    [Fact]
    public void AnOptionWithoutAValueIsIgnored()
    {
        Assert.Equal(Launch.Parse([]), Launch.Parse(["--theme"]));
        Assert.Equal(new Launch(null, Appearance.Dark, Sections.Overview), Launch.Parse(["--theme", "dark", "--page"]));
    }

    [Fact]
    public void ThePipeOfAnotherServiceCanBeNamed()
    {
        Assert.Null(Launch.Parse([]).Pipe);
        Assert.Equal("AnixOpsNet.Test", Launch.Parse(["--pipe", "AnixOpsNet.Test"]).Pipe);
        // A name, not a path: the window only ever opens a pipe on this machine.
        Assert.Null(Launch.Parse(["--pipe", @"\\other\pipe\AnixOpsNet.Core"]).Pipe);
        Assert.Null(Launch.Parse(["--pipe", ""]).Pipe);
    }

    [Fact]
    public void TheLastValueWins()
    {
        Assert.Equal(Appearance.Light, Launch.Parse(["--theme", "dark", "--theme", "light"]).Appearance);
    }

    [Fact]
    public void AnUnknownOptionDoesNotSwallowTheNextOne()
    {
        Assert.Equal(Appearance.Dark, Launch.Parse(["--verbose", "--theme", "dark"]).Appearance);
    }
}

public sealed class RequestsTests
{
    [Fact]
    public void DemoRequestsAreTheSameEveryTime()
    {
        var first = DemoRequests.Make(500);
        var second = DemoRequests.Make(500);
        Assert.Equal(first.Select(row => row.ToString()), second.Select(row => row.ToString()));
        Assert.Equal(first.Select(row => row.Size), second.Select(row => row.Size));
    }

    [Fact]
    public void ALongerDemoListStartsWithTheShorterOne()
    {
        var shorter = DemoRequests.Make(100);
        var longer = DemoRequests.Make(1000);
        Assert.Equal(shorter.Select(row => row.ToString()), longer.Take(100).Select(row => row.ToString()));
    }

    [Fact]
    public void DemoRequestsAreNumberedFromOneAndVaried()
    {
        var rows = DemoRequests.Make(5000);
        Assert.Equal(Enumerable.Range(1, 5000).Select(number => (long)number), rows.Select(row => row.Seq));
        Assert.True(rows.Select(row => row.Host).Distinct().Count() >= 8);
        Assert.True(rows.Select(row => row.Method).Distinct().Count() >= 5);
        Assert.Contains(rows, row => row.Status is null);
        Assert.Contains(rows, row => row.Status == 404);
        Assert.All(rows, row => Assert.Matches(@"\.example\.(com|org|net)$", row.Host));
    }

    [Fact]
    public void AnEmptyFilterKeepsTheListItself()
    {
        var rows = DemoRequests.Make(50);
        Assert.Same(rows, RequestFilter.Apply(rows, ""));
        Assert.Same(rows, RequestFilter.Apply(rows, "   "));
        Assert.Same(rows, RequestFilter.Apply(rows, null));
    }

    [Fact]
    public void EveryWordMustOccurInAnyCase()
    {
        RequestRow[] rows =
        [
            new(1, "GET", 200, "api.example.com", "/v1/items/1", "json", 10, 5),
            new(2, "POST", 404, "www.example.org", "/users/profile/2", "html", 10, 5),
            new(3, "GET", null, "api.example.com", "/v1/users/3", "json", 0, 5),
        ];
        Assert.Equal(new long[] { 1, 3 }, RequestFilter.Apply(rows, "API").Select(row => row.Seq));
        Assert.Equal(new long[] { 3 }, RequestFilter.Apply(rows, "api users").Select(row => row.Seq));
        Assert.Equal(new long[] { 2 }, RequestFilter.Apply(rows, " 404  post ").Select(row => row.Seq));
        Assert.Empty(RequestFilter.Apply(rows, "api 404"));
    }

    [Fact]
    public void ARequestWithoutAResponseShowsAnEllipsis()
    {
        var row = new RequestRow(7, "GET", null, "api.example.com", "/v1", "json", 0, 12);
        Assert.Equal("…", row.StatusText);
        Assert.Equal("GET … api.example.com/v1", row.ToString());
    }

    [Theory]
    [InlineData(0, "0 B")]
    [InlineData(999, "999 B")]
    [InlineData(1000, "1.0 kB")]
    [InlineData(1500, "1.5 kB")]
    [InlineData(2_000_000, "2.0 MB")]
    [InlineData(3_500_000_000, "3.5 GB")]
    public void SizesUseDecimalUnits(long bytes, string expected)
    {
        Assert.Equal(expected, Measures.Size(bytes));
    }

    [Theory]
    [InlineData(0, "0 ms")]
    [InlineData(999, "999 ms")]
    [InlineData(1000, "1.00 s")]
    [InlineData(1234, "1.23 s")]
    public void DurationsSwitchToSecondsAtOneSecond(int milliseconds, string expected)
    {
        Assert.Equal(expected, Measures.Duration(milliseconds));
    }

    [Fact]
    public void ACountIsWrittenWithoutGrouping()
    {
        Assert.Equal("1234 of 50000 requests", Strings.For(Strings.English).Format("requests.count", 1234, 50000));
        Assert.Equal("1234 / 50000 条请求", Strings.For(Strings.Chinese).Format("requests.count", 1234, 50000));
        Assert.Equal("1234 / 50000 筆請求", Strings.For(Strings.TraditionalChinese).Format("requests.count", 1234, 50000));
    }
}

public sealed class LaunchRequestsTests
{
    [Fact]
    public void DemoRowsAndMeasureAreRead()
    {
        var launch = Launch.Parse(["--demo-rows", "50000", "--measure=C:\\out\\m.json"]);
        Assert.Equal(50000, launch.DemoRows);
        Assert.Equal("C:\\out\\m.json", launch.Measure);
    }

    [Theory]
    [InlineData("-1")]
    [InlineData("many")]
    [InlineData("1000001")]
    [InlineData("1e3")]
    public void ACountThatIsNotAPlainNumberInRangeIsIgnored(string value)
    {
        Assert.Equal(0, Launch.Parse(["--demo-rows", value]).DemoRows);
    }
}
