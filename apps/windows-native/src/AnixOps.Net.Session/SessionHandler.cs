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
    public const string RootTrusted = "session.root_trusted";
    public const string TrustRoot = "session.trust_root";
    public const string UntrustRoot = "session.untrust_root";
    public const string NoticeEvent = "notice";

    private static readonly JsonElement Windows = JsonSerializer.SerializeToElement(new { desktop = "Windows" });
    private static readonly JsonElement Null = JsonSerializer.SerializeToElement<object?>(null);

    private readonly IProxyStore proxy;
    private readonly ILoginEntry login;
    private readonly IRootStore? roots;
    private readonly object writing = new();

    /// <param name="proxy">The user's proxy settings.</param>
    /// <param name="login">The entry that starts the window after login.</param>
    /// <param name="roots">The user's trusted roots; without it the window says it keeps no such store.</param>
    public SessionHandler(IProxyStore proxy, ILoginEntry login, IRootStore? roots = null)
    {
        this.proxy = proxy;
        this.login = login;
        this.roots = roots;
    }

    /// <summary>A notification arrived. Raised on a thread of the channel, not the interface's.</summary>
    public event Action<Notice>? Noticed;

    /// <summary>An event other than a notification arrived: its name and payload.</summary>
    public event Action<string, JsonElement>? Happened;

    public Task<JsonElement> RequestAsync(string command, JsonElement args)
    {
        if (command is TrustRoot or UntrustRoot)
        {
            // Windows asks the user to confirm, which takes as long as the user does: off the channel's
            // thread, so that the core's other questions are still answered meanwhile.
            return Task.Run(() => Guarded(command, args));
        }
        // Registry access is quick; answering on the calling thread keeps the order the core asked in.
        try
        {
            return Task.FromResult(Guarded(command, args));
        }
        catch (RefusedException problem)
        {
            return Task.FromException<JsonElement>(problem);
        }
    }

    private JsonElement Guarded(string command, JsonElement args)
    {
        try
        {
            return Answer(command, args);
        }
        catch (Exception problem) when (problem is UnauthorizedAccessException or System.Security.SecurityException or IOException or System.Security.Cryptography.CryptographicException)
        {
            // The reason reaches the core as the answer, and the user as the core's sentence.
            throw new RefusedException(problem.Message);
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
            case RootTrusted:
                return roots is null ? Null : JsonSerializer.SerializeToElement(roots.Trusts(Certificate(command, args)));
            case TrustRoot:
                Roots().Trust(Certificate(command, args));
                return Message.EmptyObject;
            case UntrustRoot:
                Roots().Untrust(Certificate(command, args));
                return Message.EmptyObject;
            default:
                throw new RefusedException($"{command} is not something a window does");
        }
    }

    private IRootStore Roots() => roots ?? throw new RefusedException("this window keeps no certificate store");

    private static string Certificate(string command, JsonElement args) =>
        Text(args, "certificate") is { Length: > 0 } certificate
            ? certificate
            : throw new RefusedException($"{command} needs the certificate");

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
        value.ValueKind == JsonValueKind.Object && value.TryGetProperty(name, out var text) && text.ValueKind == JsonValueKind.String ? text.GetString() : null;
}
