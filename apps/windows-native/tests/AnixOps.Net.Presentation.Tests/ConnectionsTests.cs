using System.Text.Json;
using Xunit;

namespace AnixOps.Net.Presentation.Tests;

/// <summary>
/// The open connections without a window. The same checks as the Linux window's
/// (apps/linux-native/src/connections.rs), so that both read the core's answer and draw the rows the same way.
/// </summary>
public sealed class ConnectionsTests
{
    private static readonly Strings English = Strings.For(Strings.English);
    private static readonly Strings Chinese = Strings.For(Strings.Chinese);

    private static List<JsonElement> Cases()
    {
        var file = Path.Combine(AppContext.BaseDirectory, "fixtures", "view", "cases.json");
        using var document = JsonDocument.Parse(File.ReadAllText(file));
        return document.RootElement.EnumerateArray()
            .Where(sample => sample.GetProperty("command").GetString() == "view.connections")
            .Select(sample => sample.Clone())
            .ToList();
    }

    private static ConnectionList Sample(int index) => Connections.Read(Cases()[index].GetProperty("answer"));

    [Fact]
    public void EverySampleAnswerIsRead()
    {
        var cases = Cases();
        Assert.True(cases.Count >= 3, "the connection samples are missing");
        foreach (var sample in cases)
        {
            var answer = sample.GetProperty("answer");
            var list = Connections.Read(answer);
            Assert.Equal(answer.GetProperty("running").GetBoolean(), list.Running);
            Assert.Equal(answer.GetProperty("total").GetUInt64(), list.Total);
            var rows = answer.GetProperty("connections").EnumerateArray().ToList();
            Assert.Equal(rows.Count, list.Open.Count);
            foreach (var (row, connection) in rows.Zip(list.Open))
            {
                Assert.Equal(row.GetProperty("id").GetUInt64(), connection.Id);
                Assert.Equal(row.GetProperty("target").GetString(), connection.Target);
                Assert.Equal(row.GetProperty("network").GetString(), connection.Network);
                Assert.Equal(row.GetProperty("rule").GetString(), connection.Rule);
                Assert.Equal(
                    row.GetProperty("route").EnumerateArray().Select(name => name.GetString()).ToArray(),
                    connection.Route.ToArray());
                Assert.Equal(row.GetProperty("upload").GetUInt64(), connection.Upload);
                Assert.Equal(row.GetProperty("download").GetUInt64(), connection.Download);
            }
        }
    }

    [Theory]
    [InlineData("null")]
    [InlineData("{}")]
    [InlineData("[]")]
    [InlineData("""{ "connections": "many" }""")]
    public void AnAnswerThatSaysNothingReadsAsNoEngineAndNoConnections(string answer)
    {
        using var document = JsonDocument.Parse(answer);
        Assert.Equal(ConnectionList.Nothing, Connections.Read(document.RootElement));
    }

    [Fact]
    public void ARowSaysWhereTheConnectionGoesAndHow()
    {
        var shown = Connections.Show(English, Sample(0), "", [], false);
        Assert.Equal("3 connections", shown.Count);
        Assert.Null(shown.Notice);
        Assert.True(shown.CanSearch && shown.CanCloseAll);
        Assert.Equal(
            new[]
            {
                ("example.org", "MODE,global · n2 · ↑ 0 B  ↓ 0 B"),
                ("[2001:db8::1]:53", "UDP · FINAL · DIRECT · ↑ 64 B  ↓ 128 B"),
                ("www.google.com:443", "DOMAIN-SUFFIX,google.com · Proxy → Tokyo · ↑ 1.5 KB  ↓ 1.0 MB"),
            },
            shown.Rows.Select(row => (row.Title, row.Details)).ToArray());
        Assert.All(shown.Rows, row => Assert.True(row.Enabled));
        Assert.Equal("3 个连接", Connections.Show(Chinese, Sample(0), "", [], false).Count);
    }

    [Fact]
    public void ASearchSaysHowManyOfHowMany()
    {
        var shown = Connections.Show(English, Sample(1), " Tokyo ", [], false);
        Assert.Equal("1 of 3 connections", shown.Count);
        Assert.Single(shown.Rows);
        Assert.Null(shown.Notice);
        // A search that leaves nothing says so, and can still be changed.
        var none = Sample(1) with { Open = [] };
        shown = Connections.Show(English, none, "x", [], false);
        Assert.Equal("0 of 3 connections", shown.Count);
        Assert.Equal("No connection matches the search.", shown.Notice);
        Assert.True(shown.CanSearch && shown.CanCloseAll);
    }

    [Fact]
    public void NothingOpenAndNoEngineSaySo()
    {
        var stopped = Connections.Show(English, Sample(2), "", [], false);
        Assert.Equal("0 connections", stopped.Count);
        Assert.Equal("Open connections are listed here once connected.", stopped.Notice);
        Assert.False(stopped.CanSearch || stopped.CanCloseAll);
        var idle = Connections.Show(English, ConnectionList.Nothing with { Running = true }, "", [], false);
        Assert.Equal("No connections are open right now.", idle.Notice);
        Assert.False(idle.CanCloseAll);
        Assert.Equal("1 connection", Connections.Show(English, Sample(1) with { Total = 1 }, "", [], false).Count);
    }

    [Fact]
    public void AConnectionTheCoreIsClosingCannotBeClosedAgain()
    {
        var shown = Connections.Show(English, Sample(0), "", [2], false);
        Assert.Equal(
            new[] { (3UL, true), (2UL, false), (1UL, true) },
            shown.Rows.Select(row => (row.Id, row.Enabled)).ToArray());
        Assert.True(shown.CanCloseAll);
        var all = Connections.Show(English, Sample(0), "", [], true);
        Assert.All(all.Rows, row => Assert.False(row.Enabled));
        Assert.False(all.CanCloseAll);
    }

    [Fact]
    public void WhatIsShownIsEqualWhenItSaysTheSame()
    {
        Assert.Equal(Connections.Show(English, Sample(0), "", [], false), Connections.Show(English, Sample(0), "", [], false));
        Assert.NotEqual(Connections.Show(English, Sample(0), "", [], false), Connections.Show(English, Sample(0), "", [2], false));
        Assert.Equal(Sample(0), Sample(0));
        Assert.NotEqual(Sample(0), Sample(1));
    }

    [Fact]
    public void ClosingNamesTheCoresCommands()
    {
        Assert.Equal("connections", Connections.Command);
        Assert.Equal("""{"query":"google"}""", Connections.Args("google").GetRawText());
        Assert.Equal("close_connection", Connections.CloseCommand);
        Assert.Equal("""{"id":7}""", Connections.CloseArgs(7).GetRawText());
        Assert.Equal("close_connections", Connections.CloseAllCommand);
    }
}
