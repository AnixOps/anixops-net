using System;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Automation;
using AnixOps.Net.Presentation;
using AnixOps.Net.Testing;
using Xunit;

namespace AnixOps.Net.App.UiTests;

/// <summary>The overview page: it says what the core says, and its button connects and disconnects.</summary>
public sealed class OverviewTests
{
    private const string State = "Overview.State";
    private const string Detail = "Overview.Detail";
    private const string Connect = "Overview.Connect";
    private const string Node = "Overview.Node";
    private const string DownloadRate = "Overview.DownloadRate";
    private const string UploadRate = "Overview.UploadRate";
    private const string DownloadTotal = "Overview.DownloadTotal";
    private const string UploadTotal = "Overview.UploadTotal";
    private const string Connections = "Overview.Connections";

    // Starting the engine on a busy CI machine takes its time.
    private static readonly TimeSpan Patience = TimeSpan.FromSeconds(90);

    private static string Name(AppSession app, string automationId) => app.Find(automationId).Current.Name;

    private static string WaitFor(AppSession app, string automationId, Func<string, bool> done, string expected)
    {
        var deadline = DateTime.UtcNow + Patience;
        while (true)
        {
            var text = Name(app, automationId);
            if (done(text))
            {
                return text;
            }
            Assert.True(DateTime.UtcNow < deadline, $"'{automationId}' reads '{text}', expected {expected}");
            Thread.Sleep(50);
        }
    }

    private static void WaitForEnabled(AppSession app, string automationId, bool enabled)
    {
        var deadline = DateTime.UtcNow + Patience;
        while (app.Find(automationId).Current.IsEnabled != enabled)
        {
            Assert.True(DateTime.UtcNow < deadline, $"'{automationId}' did not become {(enabled ? "enabled" : "disabled")}");
            Thread.Sleep(50);
        }
    }

    private static void Press(AppSession app, string automationId)
    {
        var button = app.Find(automationId);
        Assert.True(button.TryGetCurrentPattern(InvokePattern.Pattern, out var invoke), $"'{automationId}' cannot be pressed");
        ((InvokePattern)invoke).Invoke();
    }

    [Theory]
    [InlineData(Strings.Chinese)]
    [InlineData(Strings.English)]
    public void WithoutTheServiceThereIsNothingToPress(string language)
    {
        var strings = Strings.For(language);
        // A pipe nobody listens on, so that a service installed on this machine is not found instead.
        using var app = AppSession.Start("--language", language, "--pipe", "AnixOpsNet.Test." + Guid.NewGuid().ToString("N"));
        app.WaitForName(Detail, strings["overview.no_core"]);
        Assert.Equal(strings["overview.state.disconnected"], Name(app, State));
        var button = app.Find(Connect);
        Assert.Equal(strings["overview.connect"], button.Current.Name);
        Assert.False(button.Current.IsEnabled);
        Assert.Equal(Overview.Unknown, Name(app, Node));
        Assert.Equal(Overview.Unknown, Name(app, DownloadRate));
        Assert.Equal(Overview.Unknown, Name(app, UploadRate));
        Assert.Equal(Overview.Unknown, Name(app, Connections));
        app.Screenshot($"overview-no-core-{language}");
    }

    [Theory]
    [InlineData(Strings.Chinese, "light")]
    [InlineData(Strings.English, "dark")]
    public async Task TheButtonConnectsAndDisconnectsThroughTheService(string language, string theme)
    {
        if (!ServiceProcess.Available())
        {
            return;
        }
        var strings = Strings.For(language);
        using var service = ServiceProcess.Start();
        using var app = AppSession.Start("--language", language, "--theme", theme, "--pipe", service.PipeName);

        // A new installation: no node, so nothing to connect through.
        app.WaitForName(Node, strings["overview.node.none"]);
        Assert.Equal(strings["overview.state.disconnected"], Name(app, State));
        Assert.False(app.Find(Connect).Current.IsEnabled);

        // Connecting writes the second window's proxy settings, not the machine's real ones.
        await using var second = await SecondWindow.JoinAsync(service.PipeName);

        // Traffic leaves directly: connecting works without a node, and needs no server anywhere.
        await second.AskAsync("set_direct_egress", """{ "enabled": true }""");
        app.WaitForName(Node, strings["overview.node.direct"]);
        WaitForEnabled(app, Connect, true);
        Assert.Equal(strings["overview.connect"], Name(app, Connect));

        Press(app, Connect);
        WaitFor(app, State, text => text == strings["overview.state.connected"], $"'{strings["overview.state.connected"]}'");
        WaitFor(app, Connect, text => text == strings["overview.disconnect"], $"'{strings["overview.disconnect"]}'");
        WaitForEnabled(app, Connect, true);
        // The counters are the engine's own: numbers with units, not the mark for "unknown".
        var rate = new Regex(@"^\d+(\.\d)? (B|KB|MB|GB)/s$");
        WaitFor(app, DownloadRate, rate.IsMatch, "a rate");
        Assert.Matches(rate, Name(app, UploadRate));
        Assert.Matches(@"^\d+$", Name(app, Connections));
        var total = new Regex(Regex.Escape(strings["overview.total"]).Replace(@"\{0}", @"\d+(\.\d)? (B|KB|MB|GB)"));
        Assert.Matches(total, Name(app, DownloadTotal));
        Assert.Matches(total, Name(app, UploadTotal));
        Assert.True(second.Proxy.Read().Enabled, "connecting did not point the second window's proxy settings at the engine");
        Assert.Equal("connected", (await second.AskAsync(Overview.Command)).GetProperty("state").GetString());
        app.Screenshot($"overview-connected-{language}-{theme}");

        Press(app, Connect);
        WaitFor(app, State, text => text == strings["overview.state.disconnected"], $"'{strings["overview.state.disconnected"]}'");
        WaitFor(app, Connect, text => text == strings["overview.connect"], $"'{strings["overview.connect"]}'");
        WaitFor(app, DownloadRate, text => text == Overview.Unknown, "no rate");
        Assert.Equal(Overview.Unknown, Name(app, Connections));
        Assert.False(second.Proxy.Read().Enabled, "disconnecting left the proxy settings pointing at the engine");
        Assert.Equal("disconnected", (await second.AskAsync(Overview.Command)).GetProperty("state").GetString());
    }
}
