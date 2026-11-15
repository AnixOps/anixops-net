using Microsoft.Win32;

namespace AnixOps.Net.Session;

/// <summary>The entry that starts the app after the user signs in.</summary>
public interface ILoginEntry
{
    bool Enabled { get; }

    void Set(bool enabled);
}

/// <summary>
/// The user's own Run key. The app runs without administrator rights, so Windows starts it from there; the
/// entry is ours only while it names this executable.
/// </summary>
public sealed class RegistryLoginEntry : ILoginEntry
{
    /// <summary>The key Windows reads, under HKEY_CURRENT_USER.</summary>
    public const string Run = @"Software\Microsoft\Windows\CurrentVersion\Run";

    /// <summary>The value's name. Not the web client's, so that the two can be installed side by side.</summary>
    public const string ValueName = "AnixOpsNet";

    /// <summary>Tells the app that the user did not start it by hand.</summary>
    public const string Argument = "--start-after-login";

    private readonly string key;
    private readonly string command;

    /// <param name="executable">The app's executable.</param>
    /// <param name="key">The key under HKEY_CURRENT_USER; tests use one of their own.</param>
    public RegistryLoginEntry(string executable, string key = Run)
    {
        this.key = key;
        command = $"\"{executable}\" {Argument}";
    }

    public bool Enabled
    {
        get
        {
            using var run = Registry.CurrentUser.OpenSubKey(key);
            return run?.GetValue(ValueName) is string value && string.Equals(value, command, StringComparison.OrdinalIgnoreCase);
        }
    }

    public void Set(bool enabled)
    {
        using var run = Registry.CurrentUser.CreateSubKey(key, writable: true);
        if (enabled)
        {
            run.SetValue(ValueName, command, RegistryValueKind.String);
        }
        else
        {
            run.DeleteValue(ValueName, throwOnMissingValue: false);
        }
    }
}
