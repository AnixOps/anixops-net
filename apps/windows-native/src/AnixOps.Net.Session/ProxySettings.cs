using System.Runtime.InteropServices;
using System.Text.Json.Serialization;
using Microsoft.Win32;

namespace AnixOps.Net.Session;

/// <summary>The user's proxy settings (WinINet: what browsers and most applications read).</summary>
/// <param name="Enabled">Whether a proxy is used.</param>
/// <param name="Server">The proxy, such as <c>127.0.0.1:7890</c>.</param>
/// <param name="Bypass">Hosts that never go through it, separated by semicolons.</param>
public sealed record ProxySettings(
    [property: JsonPropertyName("enabled")] bool Enabled,
    [property: JsonPropertyName("server")] string Server,
    [property: JsonPropertyName("bypass")] string Bypass)
{
    /// <summary>Names without a dot, which are on the local network, stay direct.</summary>
    public const string LocalBypass = "<local>";

    /// <summary>The settings that send the user's applications to the local proxy on a port.</summary>
    public static ProxySettings Local(int port) => new(true, $"127.0.0.1:{port}", LocalBypass);
}

/// <summary>The settings before the client changed them, and what it wrote.</summary>
public sealed record ProxyBackup(
    [property: JsonPropertyName("previous")] ProxySettings Previous,
    [property: JsonPropertyName("applied")] ProxySettings Applied);

/// <summary>Where the proxy settings are kept.</summary>
public interface IProxyStore
{
    ProxySettings Read();

    void Write(ProxySettings settings);
}

/// <summary>
/// The signed-in user's own settings in the registry. Writing them needs no administrator rights. The
/// machine-wide WinHTTP proxy that services read is not touched here.
/// </summary>
public sealed class RegistryProxyStore : IProxyStore
{
    /// <summary>The key Windows reads, under HKEY_CURRENT_USER.</summary>
    public const string InternetSettings = @"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

    private readonly string key;
    private readonly bool announce;

    /// <param name="key">The key under HKEY_CURRENT_USER; tests use one of their own.</param>
    /// <param name="announce">Whether running applications are told to read the settings again.</param>
    public RegistryProxyStore(string key = InternetSettings, bool announce = true)
    {
        this.key = key;
        this.announce = announce;
    }

    public ProxySettings Read()
    {
        using var settings = Registry.CurrentUser.OpenSubKey(key);
        if (settings is null)
        {
            return new ProxySettings(false, "", "");
        }
        var enabled = settings.GetValue("ProxyEnable") is int value && value != 0;
        return new ProxySettings(
            enabled,
            settings.GetValue("ProxyServer") as string ?? "",
            settings.GetValue("ProxyOverride") as string ?? "");
    }

    public void Write(ProxySettings settings)
    {
        using (var target = Registry.CurrentUser.CreateSubKey(key, writable: true))
        {
            target.SetValue("ProxyEnable", settings.Enabled ? 1 : 0, RegistryValueKind.DWord);
            target.SetValue("ProxyServer", settings.Server, RegistryValueKind.String);
            target.SetValue("ProxyOverride", settings.Bypass, RegistryValueKind.String);
        }
        if (announce)
        {
            // Without these, applications that are already running keep the settings they read at start.
            InternetSetOption(IntPtr.Zero, SettingsChanged, IntPtr.Zero, 0);
            InternetSetOption(IntPtr.Zero, Refresh, IntPtr.Zero, 0);
        }
    }

    private const int Refresh = 37;
    private const int SettingsChanged = 39;

    [DllImport("wininet.dll", EntryPoint = "InternetSetOptionW", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool InternetSetOption(IntPtr internet, int option, IntPtr buffer, int length);
}
