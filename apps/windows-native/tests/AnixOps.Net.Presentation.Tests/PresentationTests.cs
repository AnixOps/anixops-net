using Xunit;

namespace AnixOps.Net.Presentation.Tests;

public sealed class StringsTests
{
    [Fact]
    public void EveryLanguageHasTheSameKeys()
    {
        var chinese = Strings.For(Strings.Chinese).Keys.Order().ToArray();
        var english = Strings.For(Strings.English).Keys.Order().ToArray();
        Assert.Equal(chinese, english);
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
    [InlineData("ZH-TW", Strings.Chinese)]
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
