using System.Text.Json;
using System.Text.Json.Nodes;
using AnixOps.Net.Ipc;
using Microsoft.Win32;
using Xunit;

namespace AnixOps.Net.Session.Tests;

/// <summary>A registry key of the test's own under HKEY_CURRENT_USER, removed afterwards.</summary>
public sealed class ScratchKey : IDisposable
{
    public ScratchKey()
    {
        Path = $@"Software\AnixOps\Net\Tests\{Guid.NewGuid():N}";
    }

    public string Path { get; }

    public void Dispose() => Registry.CurrentUser.DeleteSubKeyTree(Path, throwOnMissingSubKey: false);
}

public sealed class FakeProxyStore(ProxySettings settings) : IProxyStore
{
    public ProxySettings Settings { get; set; } = settings;
    public int Writes { get; private set; }

    public ProxySettings Read() => Settings;

    public void Write(ProxySettings settings)
    {
        Settings = settings;
        Writes++;
    }
}

public sealed class FakeLoginEntry : ILoginEntry
{
    public bool Enabled { get; private set; }

    public void Set(bool enabled) => Enabled = enabled;
}

public sealed class SessionHandlerTests
{
    private static readonly ProxySettings Off = new(false, "", "");

    private static JsonElement Sample(string name) =>
        JsonDocument.Parse(File.ReadAllText(System.IO.Path.Combine(AppContext.BaseDirectory, "fixtures", "service", name))).RootElement.Clone();

    private static JsonElement Json(string text) => JsonDocument.Parse(text).RootElement.Clone();

    private static void AssertSame(JsonElement expected, JsonElement actual) =>
        Assert.True(
            JsonNode.DeepEquals(JsonNode.Parse(expected.GetRawText()), JsonNode.Parse(actual.GetRawText())),
            $"expected {expected.GetRawText()}, got {actual.GetRawText()}");

    private static (SessionHandler Handler, FakeProxyStore Proxy, FakeLoginEntry Login) Make(ProxySettings? current = null)
    {
        var proxy = new FakeProxyStore(current ?? Off);
        var login = new FakeLoginEntry();
        return (new SessionHandler(proxy, login), proxy, login);
    }

    [Fact]
    public async Task TheWindowSaysItIsOnWindows()
    {
        var (handler, _, _) = Make();
        AssertSame(Sample("describe-answer-windows.json"), await handler.RequestAsync(SessionHandler.Describe, Message.EmptyObject));
    }

    [Fact]
    public async Task ApplyingTheProxyAnswersWithWhatWasThereAndWhatWasWritten()
    {
        var (handler, proxy, _) = Make();
        var answer = await handler.RequestAsync(SessionHandler.ApplyProxy, Sample("apply-proxy-request.json"));
        AssertSame(Sample("apply-proxy-answer-windows.json"), answer);
        Assert.Equal(new ProxySettings(true, "127.0.0.1:7890", "<local>"), proxy.Settings);
    }

    [Fact]
    public async Task RestoringPutsBackWhatWasThere()
    {
        var (handler, proxy, _) = Make(ProxySettings.Local(7890));
        await handler.RequestAsync(SessionHandler.RestoreProxy, Sample("restore-proxy-request-windows.json"));
        Assert.Equal(Off, proxy.Settings);
    }

    [Fact]
    public async Task ApplyThenRestoreLeavesTheUsersOwnProxyAsItWas()
    {
        var own = new ProxySettings(true, "proxy.example.com:3128", "*.example.org;<local>");
        var (handler, proxy, _) = Make(own);
        var backup = await handler.RequestAsync(SessionHandler.ApplyProxy, Json("""{ "port": 1080 }"""));
        Assert.Equal(ProxySettings.Local(1080), proxy.Settings);
        await handler.RequestAsync(SessionHandler.RestoreProxy, JsonSerializer.SerializeToElement(new { backup }));
        Assert.Equal(own, proxy.Settings);
    }

    [Theory]
    [InlineData(true, "127.0.0.1:8080", "<local>")]
    [InlineData(false, "127.0.0.1:7890", "<local>")]
    [InlineData(true, "127.0.0.1:7890", "")]
    public async Task SettingsSomeoneElseChangedAreLeftAlone(bool enabled, string server, string bypass)
    {
        var theirs = new ProxySettings(enabled, server, bypass);
        var (handler, proxy, _) = Make(theirs);
        await handler.RequestAsync(SessionHandler.RestoreProxy, Sample("restore-proxy-request-windows.json"));
        Assert.Equal(theirs, proxy.Settings);
        Assert.Equal(0, proxy.Writes);
    }

    [Theory]
    [InlineData("{}")]
    [InlineData("""{ "port": 0 }""")]
    [InlineData("""{ "port": 65536 }""")]
    [InlineData("""{ "port": "7890" }""")]
    [InlineData("""{ "port": 78.5 }""")]
    [InlineData("null")]
    public async Task APortThatIsNotOneIsRefusedAndNothingIsWritten(string args)
    {
        var (handler, proxy, _) = Make();
        await Assert.ThrowsAsync<RefusedException>(() => handler.RequestAsync(SessionHandler.ApplyProxy, Json(args)));
        Assert.Equal(0, proxy.Writes);
    }

    [Theory]
    [InlineData("{}")]
    [InlineData("""{ "backup": null }""")]
    [InlineData("""{ "backup": { "previous": { "enabled": false, "server": "", "bypass": "" } } }""")]
    [InlineData("""{ "backup": { "desktop": "Gnome", "previous_mode": "none" } }""")]
    [InlineData("""{ "backup": "yes" }""")]
    public async Task ABackupThatIsNotOursIsRefusedAndNothingIsWritten(string args)
    {
        var (handler, proxy, _) = Make(ProxySettings.Local(7890));
        await Assert.ThrowsAsync<RefusedException>(() => handler.RequestAsync(SessionHandler.RestoreProxy, Json(args)));
        Assert.Equal(0, proxy.Writes);
    }

    [Fact]
    public async Task TheLoginEntryIsSetAndRead()
    {
        var (handler, _, login) = Make();
        Assert.False((await handler.RequestAsync(SessionHandler.StartsAfterLogin, Message.EmptyObject)).GetBoolean());
        await handler.RequestAsync(SessionHandler.SetStartAfterLogin, Sample("set-start-after-login-request.json"));
        Assert.True(login.Enabled);
        Assert.True((await handler.RequestAsync(SessionHandler.StartsAfterLogin, Message.EmptyObject)).GetBoolean());
        await handler.RequestAsync(SessionHandler.SetStartAfterLogin, Json("""{ "enabled": false }"""));
        Assert.False(login.Enabled);
    }

    [Theory]
    [InlineData("{}")]
    [InlineData("""{ "enabled": "yes" }""")]
    [InlineData("""{ "enabled": 1 }""")]
    public async Task ALoginSwitchThatIsNotOneIsRefused(string args)
    {
        var (handler, _, login) = Make();
        await Assert.ThrowsAsync<RefusedException>(() => handler.RequestAsync(SessionHandler.SetStartAfterLogin, Json(args)));
        Assert.False(login.Enabled);
    }

    [Theory]
    [InlineData("connect")]
    [InlineData("session.unknown")]
    [InlineData("")]
    public async Task AnythingElseIsRefused(string command)
    {
        var (handler, _, _) = Make();
        await Assert.ThrowsAsync<RefusedException>(() => handler.RequestAsync(command, Message.EmptyObject));
    }

    [Fact]
    public void ANoticeIsPassedOn()
    {
        var (handler, _, _) = Make();
        var notices = new List<Notice>();
        var others = new List<string>();
        handler.Noticed += notices.Add;
        handler.Happened += (name, _) => others.Add(name);
        handler.Event(SessionHandler.NoticeEvent, Sample("notice-event.json"));
        Assert.Equal(new[] { new Notice("Disconnected", "The connection was lost", null) }, notices.ToArray());
        Assert.Empty(others);
    }

    [Fact]
    public void OtherEventsArePassedOnByName()
    {
        var (handler, _, _) = Make();
        var others = new List<string>();
        handler.Happened += (name, _) => others.Add(name);
        handler.Event("snapshot", Message.EmptyObject);
        // A notice without its texts is not shown as one.
        handler.Event(SessionHandler.NoticeEvent, Json("""{ "title": "only a title" }"""));
        Assert.Equal(new[] { "snapshot", SessionHandler.NoticeEvent }, others.ToArray());
    }
}

public sealed class RegistryProxyStoreTests
{
    [Fact]
    public void AKeyThatIsNotThereReadsAsNoProxy()
    {
        using var key = new ScratchKey();
        Assert.Equal(new ProxySettings(false, "", ""), new RegistryProxyStore(key.Path, announce: false).Read());
    }

    [Fact]
    public void WhatIsWrittenIsReadBack()
    {
        using var key = new ScratchKey();
        var store = new RegistryProxyStore(key.Path, announce: false);
        var settings = new ProxySettings(true, "127.0.0.1:7890", "<local>");
        store.Write(settings);
        Assert.Equal(settings, store.Read());
        store.Write(new ProxySettings(false, "", ""));
        Assert.Equal(new ProxySettings(false, "", ""), store.Read());
    }

    [Fact]
    public void TheValuesAreTheOnesWindowsReads()
    {
        using var key = new ScratchKey();
        new RegistryProxyStore(key.Path, announce: false).Write(new ProxySettings(true, "127.0.0.1:7890", "<local>"));
        using var written = Registry.CurrentUser.OpenSubKey(key.Path);
        Assert.NotNull(written);
        Assert.Equal(1, written.GetValue("ProxyEnable"));
        Assert.Equal(RegistryValueKind.DWord, written.GetValueKind("ProxyEnable"));
        Assert.Equal("127.0.0.1:7890", written.GetValue("ProxyServer"));
        Assert.Equal("<local>", written.GetValue("ProxyOverride"));
    }

    /// <summary>Writes the user's real settings back as they are, telling applications: the machine is left as found.</summary>
    [Fact]
    public void TheRealSettingsCanBeReadAndWrittenWithoutAdministratorRights()
    {
        var store = new RegistryProxyStore();
        var current = store.Read();
        store.Write(current);
        Assert.Equal(current, store.Read());
    }

    [Fact]
    public async Task TheHandlerWorksOnTheRegistry()
    {
        using var key = new ScratchKey();
        using var run = new ScratchKey();
        var store = new RegistryProxyStore(key.Path, announce: false);
        var handler = new SessionHandler(store, new RegistryLoginEntry(@"C:\Program Files\AnixOps\Net\AnixOpsNet.exe", run.Path));
        var backup = await handler.RequestAsync(SessionHandler.ApplyProxy, JsonDocument.Parse("""{ "port": 7890 }""").RootElement);
        Assert.Equal(ProxySettings.Local(7890), store.Read());
        await handler.RequestAsync(SessionHandler.RestoreProxy, JsonSerializer.SerializeToElement(new { backup }));
        Assert.Equal(new ProxySettings(false, "", ""), store.Read());
    }
}

public sealed class RegistryLoginEntryTests
{
    private const string Executable = @"C:\Program Files\AnixOps\Net\AnixOpsNet.exe";

    [Fact]
    public void TheEntryIsAddedAndRemoved()
    {
        using var key = new ScratchKey();
        var entry = new RegistryLoginEntry(Executable, key.Path);
        Assert.False(entry.Enabled);
        entry.Set(true);
        Assert.True(entry.Enabled);
        using (var run = Registry.CurrentUser.OpenSubKey(key.Path))
        {
            Assert.Equal($"\"{Executable}\" --start-after-login", run?.GetValue(RegistryLoginEntry.ValueName));
        }
        entry.Set(false);
        Assert.False(entry.Enabled);
        // Removing what is not there is not an error.
        entry.Set(false);
    }

    [Fact]
    public void AnEntryForAnotherCopyIsNotOurs()
    {
        using var key = new ScratchKey();
        new RegistryLoginEntry(@"D:\Old\AnixOpsNet.exe", key.Path).Set(true);
        Assert.False(new RegistryLoginEntry(Executable, key.Path).Enabled);
    }

    [Fact]
    public void ThePathIsComparedWithoutCase()
    {
        using var key = new ScratchKey();
        new RegistryLoginEntry(Executable.ToUpperInvariant(), key.Path).Set(true);
        Assert.True(new RegistryLoginEntry(Executable, key.Path).Enabled);
    }
}
