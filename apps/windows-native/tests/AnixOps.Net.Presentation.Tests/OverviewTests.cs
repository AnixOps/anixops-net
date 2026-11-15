using System.Text.Json;
using Xunit;

namespace AnixOps.Net.Presentation.Tests;

/// <summary>
/// The overview page without a window. The same checks as the Linux window's (apps/linux-native/src/overview.rs),
/// so that both read the core's answer and write numbers the same way.
/// </summary>
public sealed class OverviewTests
{
    private static readonly Strings English = Strings.For(Strings.English);
    private static readonly Strings Chinese = Strings.For(Strings.Chinese);
    private static readonly Traffic Busy = new(UploadRate: 2048, DownloadRate: 1_536_000, UploadTotal: 1_048_576, DownloadTotal: 734_003_200, Connections: 12);

    private static List<JsonElement> Cases()
    {
        var file = Path.Combine(AppContext.BaseDirectory, "fixtures", "view", "cases.json");
        using var document = JsonDocument.Parse(File.ReadAllText(file));
        return document.RootElement.EnumerateArray()
            .Where(sample => sample.GetProperty("command").GetString() == "view.overview")
            .Select(sample => sample.Clone())
            .ToList();
    }

    private static Overview Node(ConnectionState state) =>
        new(state, Direct: false, NodeLabel: "Tokyo", NodeProtocol: "trojan", CanConnect: true, Error: null, Traffic: null);

    [Fact]
    public void EverySampleAnswerIsRead()
    {
        var cases = Cases();
        Assert.True(cases.Count >= 8, "the overview samples are missing");
        foreach (var sample in cases)
        {
            var answer = sample.GetProperty("answer");
            var read = Overview.Read(answer);
            var state = answer.GetProperty("state").GetString();
            Assert.Equal(
                state switch
                {
                    "connected" => ConnectionState.Connected,
                    "failed" => ConnectionState.Failed,
                    "core_error" => ConnectionState.CoreError,
                    "disconnected" => ConnectionState.Disconnected,
                    _ => throw new InvalidDataException($"a state this test does not know: {state}"),
                },
                read.State);
            Assert.Equal(answer.GetProperty("egress").GetString() == "direct", read.Direct);
            Assert.Equal(answer.GetProperty("node").ValueKind == JsonValueKind.Object, read.NodeLabel is not null);
            Assert.Equal(answer.GetProperty("canConnect").GetBoolean(), read.CanConnect);
            Assert.Equal(answer.GetProperty("error").GetString(), read.Error);
            Assert.Equal(answer.GetProperty("traffic").ValueKind == JsonValueKind.Object, read.Traffic is not null);
        }
        var connected = Overview.Read(cases[0].GetProperty("answer"));
        Assert.Equal("Osaka", connected.NodeLabel);
        Assert.Equal("vless", connected.NodeProtocol);
        Assert.Equal(Busy, connected.Traffic);
    }

    [Theory]
    [InlineData("null")]
    [InlineData("[]")]
    [InlineData("{}")]
    [InlineData("""{ "state": 3, "egress": null, "node": "Tokyo", "canConnect": "yes", "error": 7, "traffic": [] }""")]
    [InlineData("""{ "traffic": { "uploadRate": 1, "downloadRate": 2, "uploadTotal": 3, "downloadTotal": -4, "connections": 5 } }""")]
    [InlineData("""{ "traffic": { "uploadRate": 1, "downloadRate": 2, "uploadTotal": 3, "downloadTotal": 4 } }""")]
    public void AnAnswerThatSaysNothingReadsAsNotConnected(string answer)
    {
        using var document = JsonDocument.Parse(answer);
        Assert.Equal(
            new Overview(ConnectionState.Disconnected, Direct: false, NodeLabel: null, NodeProtocol: "", CanConnect: false, Error: null, Traffic: null),
            Overview.Read(document.RootElement));
    }

    [Fact]
    public void ACoreThatDoesNotAnswerLeavesNothingToPress()
    {
        var page = Overview.Show(English, coreAnswers: false, null, Pending.Nothing, null);
        Assert.Equal("Not connected", page.Headline);
        Assert.Equal("The core service is not running, so there is nothing to connect.", page.Detail);
        Assert.False(page.Enabled);
        Assert.Equal(Press.Connect, page.Press);
        Assert.Equal(Overview.Unknown, page.Node);
        Assert.Equal(Overview.Unknown, page.DownloadRate);
        Assert.Equal(Overview.Unknown, page.Connections);
        Assert.Null(page.DownloadTotal);

        // Answering, but the first answer has not come yet.
        page = Overview.Show(English, coreAnswers: true, null, Pending.Nothing, null);
        Assert.Null(page.Detail);
        Assert.False(page.Enabled);
    }

    [Fact]
    public void TheButtonConnectsOrDisconnectsAndWaitsWhileTheCoreWorks()
    {
        var off = Node(ConnectionState.Disconnected);
        var page = Overview.Show(English, true, off, Pending.Nothing, null);
        Assert.Equal(("Connect", Press.Connect, true), (page.Button, page.Press, page.Enabled));
        Assert.Equal("Tokyo · trojan", page.Node);

        page = Overview.Show(English, true, off, Pending.Connecting, null);
        Assert.Equal("Connecting", page.Headline);
        Assert.False(page.Enabled);

        var on = Node(ConnectionState.Connected) with { Traffic = Busy };
        page = Overview.Show(English, true, on, Pending.Nothing, null);
        Assert.Equal("Connected", page.Headline);
        Assert.Equal(("Disconnect", Press.Disconnect, true), (page.Button, page.Press, page.Enabled));
        Assert.Equal("1.5 MB/s", page.DownloadRate);
        Assert.Equal("2.0 KB/s", page.UploadRate);
        Assert.Equal("700.0 MB in total", page.DownloadTotal);
        Assert.Equal("1.0 MB in total", page.UploadTotal);
        Assert.Equal("12", page.Connections);

        page = Overview.Show(Chinese, true, on, Pending.Disconnecting, null);
        Assert.Equal("正在断开", page.Headline);
        Assert.False(page.Enabled);
        Assert.Equal("累计 700.0 MB", page.DownloadTotal);
    }

    [Fact]
    public void WithoutANodeThereIsNothingToConnectThroughUnlessTrafficLeavesDirectly()
    {
        var none = Node(ConnectionState.Disconnected) with { NodeLabel = null, NodeProtocol = "", CanConnect = false };
        var page = Overview.Show(English, true, none, Pending.Nothing, null);
        Assert.Equal("No node chosen yet", page.Node);
        Assert.False(page.Enabled);

        page = Overview.Show(English, true, none with { Direct = true, CanConnect = true }, Pending.Nothing, null);
        Assert.Equal("Direct, not through a node", page.Node);
        Assert.True(page.Enabled);

        // A node whose protocol the core did not name is shown by its name alone.
        page = Overview.Show(English, true, Node(ConnectionState.Disconnected) with { NodeProtocol = "" }, Pending.Nothing, null);
        Assert.Equal("Tokyo", page.Node);

        // Connected can always be disconnected.
        Assert.True(Overview.Show(English, true, Node(ConnectionState.Connected) with { CanConnect = false }, Pending.Nothing, null).Enabled);
    }

    [Fact]
    public void TheReasonShownIsTheLastPressFirstAndTheCoresOtherwise()
    {
        var failed = Node(ConnectionState.Failed) with { Error = "the local port 7890 is in use" };
        var page = Overview.Show(English, true, failed, Pending.Nothing, null);
        Assert.Equal("Could not connect", page.Headline);
        Assert.Equal("the local port 7890 is in use", page.Error);
        Assert.Equal("not for this user", Overview.Show(English, true, failed, Pending.Nothing, "not for this user").Error);

        page = Overview.Show(English, true, Node(ConnectionState.CoreError), Pending.Nothing, null);
        Assert.Equal("The core stopped unexpectedly", page.Headline);
        Assert.Null(page.Error);
    }

    [Theory]
    [InlineData(0UL, "0 B")]
    [InlineData(1023UL, "1023 B")]
    [InlineData(1024UL, "1.0 KB")]
    [InlineData(1280UL, "1.2 KB")]
    [InlineData(1331UL, "1.3 KB")]
    [InlineData(1536UL, "1.5 KB")]
    [InlineData(1_048_575UL, "1.0 MB")]
    [InlineData(1_048_576UL, "1.0 MB")]
    [InlineData(734_003_200UL, "700.0 MB")]
    [InlineData(1_073_741_824UL, "1.0 GB")]
    [InlineData(5_497_558_138_880UL, "5.0 TB")]
    [InlineData(ulong.MaxValue, "16384.0 PB")]
    public void SizesStepBy1024WithOneDecimal(ulong bytes, string expected)
    {
        Assert.Equal(expected, Overview.Size(bytes));
    }

    [Fact]
    public void RatesArePerSecondAndPressesNameTheCoresCommands()
    {
        Assert.Equal("0 B/s", Overview.Rate(0));
        Assert.Equal("1.5 MB/s", Overview.Rate(1_536_000));
        Assert.Equal(("connect", Pending.Connecting), (Overview.CommandFor(Press.Connect), Overview.PendingFor(Press.Connect)));
        Assert.Equal(("disconnect", Pending.Disconnecting), (Overview.CommandFor(Press.Disconnect), Overview.PendingFor(Press.Disconnect)));
    }
}
