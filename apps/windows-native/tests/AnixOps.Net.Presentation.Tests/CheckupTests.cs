using System.Text.Json;
using Xunit;

namespace AnixOps.Net.Presentation.Tests;

/// <summary>
/// The node checkup without a window, read from the core's own samples (crates/client-view/fixtures). The same
/// checks as the Linux window's (apps/linux-native/src/checkup.rs).
/// </summary>
public sealed class CheckupTests
{
    private static readonly Strings English = Strings.For(Strings.English);
    private static readonly Strings Chinese = Strings.For(Strings.Chinese);

    private static List<JsonElement> Answers()
    {
        var file = Path.Combine(AppContext.BaseDirectory, "fixtures", "view", "cases.json");
        using var document = JsonDocument.Parse(File.ReadAllText(file));
        return document.RootElement.EnumerateArray()
            .Where(sample => sample.GetProperty("command").GetString() == "view.checkup")
            .Select(sample => sample.GetProperty("answer").Clone())
            .ToList();
    }

    [Fact]
    public void TheCommandAndItsArguments()
    {
        Assert.Equal("node_checkup", Checkup.Command);
        Assert.Equal("""{"nodeId":"a"}""", Checkup.Args("a", throughput: false).GetRawText());
        Assert.Equal(
            """{"nodeId":"a","options":{"throughput":{"max_bytes":200000000}}}""",
            Checkup.Args("a", throughput: true).GetRawText());
    }

    [Fact]
    public void AFullCheckupReadsAsLatencyThenDownload()
    {
        var sections = Checkup.Show(English, Answers()[0]);
        Assert.Equal(new[] { "Latency", "Download" }, sections.Select(section => section.Heading));

        var latency = sections[0].Lines;
        Assert.Equal(
            new[] { "Handshake", "First handshake", "Later handshakes", "Request latency", "Jitter", "Loss", "Reason" },
            latency.Select(line => line.Label));
        Assert.Equal(new CheckupLine("Handshake", "42 ms", false), latency[0]);
        Assert.Equal(new CheckupLine("Jitter", "3.5 ms", false), latency[4]);
        Assert.Equal(new CheckupLine("Loss", "20%", true), latency[5]);
        Assert.Equal(new CheckupLine("Reason", "Timed out", true), latency[6]);
        Assert.Null(sections[0].Note);

        var download = sections[1].Lines;
        Assert.Equal(new CheckupLine("Average speed", "94.5 Mbps", false), download[0]);
        Assert.Equal(new CheckupLine("Peak speed", "120.3 Mbps", false), download[1]);
        Assert.Equal(new CheckupLine("Downloaded", Overview.Size(94_500_000), false), download[2]);
        Assert.Equal(new CheckupLine("Added latency", "142 ms", true), download[5]);
        Assert.Equal(new CheckupLine("Loaded loss", "0%", false), download[6]);
    }

    [Fact]
    public void ANodeThatDoesNotAnswerShowsDashesAndTheReason()
    {
        var sections = Checkup.Show(Chinese, Answers()[1]);
        var latency = Assert.Single(sections);
        Assert.Equal("延迟", latency.Heading);
        Assert.Equal(new CheckupLine("握手延迟", "—", false), latency.Lines[0]);
        Assert.DoesNotContain(latency.Lines, line => line.Label == "后续握手");
        Assert.Equal(new CheckupLine("丢包", "100%", true), latency.Lines[^2]);
        Assert.Equal(new CheckupLine("失败原因", "连接失败", true), latency.Lines[^1]);
    }

    [Fact]
    public void WhatIsNotUnderstoodIsLeftOut()
    {
        Assert.Empty(Checkup.Show(English, JsonDocument.Parse("null").RootElement));
        Assert.Empty(Checkup.Show(English, JsonDocument.Parse("""{"sections":[{"id":"upload","rows":[]}]}""").RootElement));
        var odd = Checkup.Show(English, JsonDocument.Parse(
            """{"sections":[{"id":"latency","rows":[{"id":"later","kind":"ms","value":1},{"id":"failure","kind":"failure","value":"new_code","warn":true}],"capped":true}]}""").RootElement);
        Assert.Equal(new CheckupLine("Reason", "new_code", true), Assert.Single(odd[0].Lines));
        Assert.Equal(English["checkup.capped"], odd[0].Note);
    }

    [Fact]
    public void TheNoteSaysWhatIsSent()
    {
        Assert.Contains("1.1.1.1", Checkup.Note(English, throughput: false));
        Assert.DoesNotContain("speed.cloudflare.com", Checkup.Note(English, throughput: false));
        Assert.Contains("speed.cloudflare.com", Checkup.Note(English, throughput: true));
    }
}
