using System;
using System.Threading;
using AnixOps.Net.Presentation;
using AnixOps.Net.Testing;
using Xunit;

namespace AnixOps.Net.App.UiTests;

/// <summary>The window and the core service: the window says whether the service answers.</summary>
public sealed class CoreTests
{
    private const string Status = "Core.Status";
    private static readonly TimeSpan Patience = TimeSpan.FromSeconds(30);

    /// <summary>What the status reads once connected, up to the version.</summary>
    internal static string ConnectedPrefix(Strings strings) => strings.Format("core.connected", "").TrimEnd();

    internal static string WaitForStatus(AppSession app, Func<string, bool> done, string expected)
    {
        var deadline = DateTime.UtcNow + Patience;
        while (true)
        {
            var text = app.Find(Status).Current.Name;
            if (done(text))
            {
                return text;
            }
            Assert.True(DateTime.UtcNow < deadline, $"'{Status}' reads '{text}', expected {expected}");
            Thread.Sleep(50);
        }
    }

    [Theory]
    [InlineData(Strings.Chinese)]
    [InlineData(Strings.English)]
    public void WithoutTheServiceTheWindowSaysSo(string language)
    {
        var strings = Strings.For(language);
        // A pipe nobody listens on, so that a service installed on this machine is not found instead.
        using var app = AppSession.Start("--language", language, "--pipe", "AnixOpsNet.Test." + Guid.NewGuid().ToString("N"));
        app.WaitForName(Status, strings["core.unavailable"]);
        app.Screenshot($"core-unavailable-{language}");
    }

    [Theory]
    [InlineData(Strings.Chinese)]
    [InlineData(Strings.English)]
    public void TheWindowFollowsTheServiceComingAndGoing(string language)
    {
        if (!ServiceProcess.Available())
        {
            return;
        }
        var strings = Strings.For(language);
        var prefix = ConnectedPrefix(strings);
        using var service = ServiceProcess.Start();
        using var app = AppSession.Start("--language", language, "--pipe", service.PipeName);

        var connected = WaitForStatus(app, text => text.StartsWith(prefix, StringComparison.Ordinal) && text.Length > prefix.Length, $"'{prefix} <version>'");
        // The version is the service's own, as it said in its greeting.
        Assert.Matches(@"\d+\.\d+\.\d+", connected[prefix.Length..]);
        app.Screenshot($"core-connected-{language}");

        service.Stop();
        app.WaitForName(Status, strings["core.unavailable"]);

        using var again = ServiceProcess.Start(service.PipeName, service.Directory);
        WaitForStatus(app, text => text.StartsWith(prefix, StringComparison.Ordinal) && text.Length > prefix.Length, $"'{prefix} <version>'");
    }
}

/// <summary>
/// The installed client: run by the installer's test in CI (.github/scripts/native-msi-lifecycle.ps1) with
/// ANIXOPS_NET_APP_EXE naming the installed window. Elsewhere it does nothing.
/// </summary>
public sealed class InstalledTests
{
    /// <summary>Set to 1 when the client is installed on this machine and its service runs.</summary>
    public const string InstalledVariable = "ANIXOPS_NET_INSTALLED";

    [Fact]
    public void TheInstalledWindowReachesTheInstalledService()
    {
        if (Environment.GetEnvironmentVariable(InstalledVariable) != "1")
        {
            return;
        }
        var strings = Strings.For(Strings.English);
        var prefix = CoreTests.ConnectedPrefix(strings);
        // No pipe is named: the window finds the service the installer registered.
        using var app = AppSession.Start("--language", "en");
        var connected = CoreTests.WaitForStatus(app, text => text.StartsWith(prefix, StringComparison.Ordinal) && text.Length > prefix.Length, $"'{prefix} <version>'");
        Assert.Matches(@"\d+\.\d+\.\d+", connected[prefix.Length..]);
        app.Screenshot("installed-connected");
    }
}
