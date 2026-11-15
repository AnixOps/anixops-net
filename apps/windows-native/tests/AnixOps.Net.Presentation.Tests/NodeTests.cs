using System.Text.Json;
using Xunit;

namespace AnixOps.Net.Presentation.Tests;

/// <summary>
/// One node's dialog without a window. The same checks as the Linux window's (apps/linux-native/src/node.rs),
/// so that both read the core's answers and say the same about a node.
/// </summary>
public sealed class NodeTests
{
    private static readonly Strings English = Strings.For(Strings.English);
    private static readonly Strings Chinese = Strings.For(Strings.Chinese);

    private const string Answer = """
        {
          "id": "n1", "name": "HK", "protocol": "Shadowsocks", "server": "hk.example", "port": 8443,
          "editable": true, "keep": null,
          "link": "ss://abc@hk.example:8443#HK",
          "qr": ["#.#", ".#.", "#.#"]
        }
        """;

    private static JsonElement Json(string text)
    {
        using var document = JsonDocument.Parse(text);
        return document.RootElement.Clone();
    }

    private static NodeDetails Sample() => Node.Read(Json(Answer));

    [Fact]
    public void TheCoresAnswerIsRead()
    {
        Assert.Equal(
            new NodeDetails("n1", "HK", "Shadowsocks", "hk.example", 8443, true, null, "ss://abc@hk.example:8443#HK", ["#.#", ".#.", "#.#"]),
            Sample());
        Assert.Equal(NodeKeep.Last, Node.Read(Json("""{ "keep": "last" }""")).Keep);
        Assert.Equal(NodeKeep.Used, Node.Read(Json("""{ "keep": "used" }""")).Keep);
        Assert.Equal(NodeKeep.Other, Node.Read(Json("""{ "keep": "other" }""")).Keep);
        // A reason this window does not know still keeps the node.
        Assert.Equal(NodeKeep.Other, Node.Read(Json("""{ "keep": "new" }""")).Keep);
        var nothing = new NodeDetails("", "", "", "", 0, false, null, null, null);
        Assert.Equal(nothing, Node.Read(Json("null")));
        Assert.Equal(nothing, Node.Read(Json("""{ "link": "", "qr": [] }""")));
    }

    [Fact]
    public void ANodeOfTheSubscriptionInUseCanBeChangedAndRemoved()
    {
        var shown = Node.Show(English, Sample(), busy: false, problem: null);
        Assert.Equal("HK", shown.Title);
        Assert.Equal("Shadowsocks", shown.Protocol);
        Assert.Equal(new NodeDraft("HK", "hk.example", "8443"), shown.Draft);
        Assert.True(shown.Editable && shown.CanSave && shown.CanRemove);
        Assert.Equal("Changes and removals last until the subscription is next updated.", shown.Note);
        Assert.Null(shown.Keep);
        Assert.Equal("ss://abc@hk.example:8443#HK", shown.Link);
        Assert.Equal(new[] { "#.#", ".#.", "#.#" }, shown.Qr);
        // While the core works on the node nothing more is asked of it.
        var busy = Node.Show(English, Sample(), busy: true, problem: null);
        Assert.True(busy.Editable && !busy.CanSave && !busy.CanRemove);
        Assert.Equal(shown, Node.Show(English, Sample(), busy: false, problem: null));
        Assert.NotEqual(shown, busy);
    }

    [Fact]
    public void ANodeThatMustStaySaysWhy()
    {
        var last = Node.Show(Chinese, Sample() with { Keep = NodeKeep.Last }, busy: false, problem: null);
        Assert.True(last.CanSave && !last.CanRemove);
        Assert.Equal("只剩这一个节点，不能删除。", last.Keep);
        Assert.Equal(
            "The subscription's policy groups or rules use this node, so it cannot be removed.",
            Node.Show(English, Sample() with { Keep = NodeKeep.Used }, busy: false, problem: null).Keep);
        // A node of another subscription: nothing to change, and one sentence says so.
        var other = Node.Show(English, Sample() with { Editable = false, Keep = NodeKeep.Other }, busy: false, problem: "no");
        Assert.True(!other.Editable && !other.CanSave && !other.CanRemove);
        Assert.Null(other.Keep);
        Assert.Equal("Only nodes of the subscription in use can be changed or removed.", other.Note);
        Assert.Equal("no", other.Problem);
    }

    [Fact]
    public void WhatWasTypedNamesTheCoresCommands()
    {
        var draft = new NodeDraft("香港", "hk.example", "443");
        Assert.Equal("check_node", NodeDraft.CheckCommand);
        Assert.Equal("edit_node", NodeDraft.SaveCommand);
        var check = draft.CheckArgs();
        Assert.Equal("香港", check.GetProperty("name").GetString());
        Assert.Equal("hk.example", check.GetProperty("server").GetString());
        Assert.Equal("443", check.GetProperty("port").GetString());
        var save = draft.SaveArgs("n1");
        Assert.Equal("n1", save.GetProperty("nodeId").GetString());
        Assert.Equal("香港", save.GetProperty("name").GetString());
        Assert.Equal("hk.example", save.GetProperty("server").GetString());
        Assert.Equal("443", save.GetProperty("port").GetString());
        Assert.Equal("node_details", Node.Command);
        Assert.Equal("remove_node", Node.Remove);
        Assert.Equal("""{"nodeId":"n1"}""", Node.Args("n1").GetRawText());
    }

    [Theory]
    [InlineData("server.empty", "Enter the server address.")]
    [InlineData("server.invalid", "The server address cannot contain spaces or slashes.")]
    [InlineData("port.invalid", "The port must be a number from 1 to 65535.")]
    public void TheCoresObjectionIsSaidInTheWindowsLanguage(string code, string said)
    {
        Assert.Equal(said, Node.Problem(English, Json($$"""{ "problem": "{{code}}" }""")));
    }

    [Theory]
    [InlineData("""{ "problem": null }""")]
    [InlineData("{}")]
    [InlineData("null")]
    public void NoObjectionIsNoProblem(string answer)
    {
        Assert.Null(Node.Problem(English, Json(answer)));
    }

    [Fact]
    public void AQrCodeIsDrawnBlackOnWhiteWithAQuietZone()
    {
        var (side, pixels) = Node.Picture(["#.", ".#"], 2);
        // Two modules and four of quiet zone on each side, two pixels each.
        Assert.Equal(20, side);
        Assert.Equal(20 * 20 * 4, pixels.Length);
        int At(int x, int y) => pixels[((y * side) + x) * 4];
        Assert.Equal(0xff, At(0, 0));
        Assert.Equal(0xff, At(7, 7));
        Assert.Equal((0, 0), (At(8, 8), At(9, 9)));
        Assert.Equal((0xff, 0xff), (At(10, 8), At(8, 10)));
        Assert.Equal((0, 0), (At(10, 10), At(11, 11)));
        Assert.Equal(0xff, At(12, 12));
        // Every pixel is opaque.
        Assert.All(Enumerable.Range(0, side * side), pixel => Assert.Equal(0xff, (int)pixels[(pixel * 4) + 3]));
    }
}
