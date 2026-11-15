using System.Text.Json;
using AnixOps.Net.Ipc;

namespace AnixOps.Net.Session;

/// <summary>A notification the core wants shown.</summary>
/// <param name="Title">The headline.</param>
/// <param name="Body">The text under it.</param>
/// <param name="OpenUrl">What clicking it opens, or null.</param>
public sealed record Notice(string Title, string Body, string? OpenUrl);

/// <summary>
/// The window's answers to the core. The core runs in a system service and cannot reach one user's
/// settings, so it asks the window (crates/client-service: the <c>session.*</c> commands; the tests read
/// that crate's samples).
/// </summary>
public sealed class SessionHandler : IWindowHandler
{
    public const string Describe = "session.describe";
    public const string ApplyProxy = "session.apply_proxy";
    public const string RestoreProxy = "session.restore_proxy";
    public const string StartsAfterLogin = "session.starts_after_login";
    public const string SetStartAfterLogin = "session.set_start_after_login";
    public const string NoticeEvent = "notice";

    private static readonly JsonElement Windows = JsonSerializer.SerializeToElement(new { desktop = "Windows" });

    private readonly IProxyStore proxy;
    private readonly ILoginEntry login;
    private readonly object writing = new();

    public SessionHandler(IProxyStore proxy, ILoginEntry login)
    {
        this.proxy = proxy;
        this.login = login;
    }

    /// <summary>A notification arrived. Raised on a thread of the channel, not the interface's.</summary>
    public event Action<Notice>? Noticed;

    /// <summary>An event other than a notification arrived: its name and payload.</summary>
    public event Action<string, JsonElement>? Happened;

    public Task<JsonElement> RequestAsync(string command, JsonElement args)
    {
        // Registry access is quick; answering on the calling thread keeps the order the core asked in.
        try
        {
            return Task.FromResult(Answer(command, args));
        }
        catch (Exception problem) when (problem is RefusedException or UnauthorizedAccessException or System.Security.SecurityException or IOException)
        {
            // The reason reaches the core as the answer, and the user as the core's sentence.
            return Task.FromException<JsonElement>(problem is RefusedException ? problem : new RefusedException(problem.Message));
        }
    }

    public void Event(string name, JsonElement payload)
    {
        if (name == NoticeEvent && payload.ValueKind == JsonValueKind.Object)
        {
            var title = Text(payload, "title");
            var body = Text(payload, "body");
            if (title is not null && body is not null)
            {
                Noticed?.Invoke(new Notice(title, body, Text(payload, "openUrl")));
                return;
            }
        }
        Happened?.Invoke(name, payload);
    }

    private JsonElement Answer(string command, JsonElement args)
    {
        switch (command)
        {
            case Describe:
                return Windows;
            case ApplyProxy:
            {
                if (args.ValueKind != JsonValueKind.Object
                    || !args.TryGetProperty("port", out var value)
                    || value.ValueKind != JsonValueKind.Number
                    || !value.TryGetInt32(out var port)
                    || port is < 1 or > 65535)
                {
                    throw new RefusedException($"{ApplyProxy} needs a port between 1 and 65535");
                }
                lock (writing)
                {
                    var previous = proxy.Read();
                    var applied = ProxySettings.Local(port);
                    proxy.Write(applied);
                    return JsonSerializer.SerializeToElement(new ProxyBackup(previous, applied));
                }
            }
            case RestoreProxy:
            {
                var backup = Backup(args);
                lock (writing)
                {
                    // If the settings are no longer what the client wrote, the user or another program
                    // changed them since, and they are left as they are.
                    if (proxy.Read() == backup.Applied)
                    {
                        proxy.Write(backup.Previous);
                    }
                }
                return Message.EmptyObject;
            }
            case StartsAfterLogin:
                return JsonSerializer.SerializeToElement(login.Enabled);
            case SetStartAfterLogin:
            {
                if (args.ValueKind != JsonValueKind.Object
                    || !args.TryGetProperty("enabled", out var value)
                    || value.ValueKind is not (JsonValueKind.True or JsonValueKind.False))
                {
                    throw new RefusedException($"{SetStartAfterLogin} needs enabled to be true or false");
                }
                login.Set(value.GetBoolean());
                return Message.EmptyObject;
            }
            default:
                throw new RefusedException($"{command} is not something a window does");
        }
    }

    private static ProxyBackup Backup(JsonElement args)
    {
        try
        {
            if (args.ValueKind == JsonValueKind.Object
                && args.TryGetProperty("backup", out var value)
                && value.Deserialize<ProxyBackup>() is { Previous: not null, Applied: not null } backup
                && backup.Previous.Server is not null
                && backup.Previous.Bypass is not null
                && backup.Applied.Server is not null
                && backup.Applied.Bypass is not null)
            {
                return backup;
            }
        }
        catch (JsonException)
        {
        }
        throw new RefusedException($"{RestoreProxy} needs the backup that {ApplyProxy} gave");
    }

    private static string? Text(JsonElement value, string name) =>
        value.TryGetProperty(name, out var text) && text.ValueKind == JsonValueKind.String ? text.GetString() : null;
}
