using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows.Automation;

namespace AnixOps.Net.App.UiTests;

/// <summary>One run of the built app, seen through UI Automation.</summary>
public sealed class AppSession : IDisposable
{
    /// <summary>The app's executable: named by ANIXOPS_NET_APP_EXE, or the one this solution built.</summary>
    public const string ExecutableVariable = "ANIXOPS_NET_APP_EXE";

    /// <summary>Where screenshots go; without it they are not taken.</summary>
    public const string ScreenshotsVariable = "ANIXOPS_NET_SCREENSHOTS";

    private const string ExecutableName = "AnixOpsNet.exe";
#if DEBUG
    private const string Configuration = "Debug";
#else
    private const string Configuration = "Release";
#endif

    private static readonly TimeSpan StartPatience = TimeSpan.FromSeconds(60);
    private static readonly TimeSpan Patience = TimeSpan.FromSeconds(15);

    private readonly Process process;

    private AppSession(Process process, AutomationElement window)
    {
        this.process = process;
        Window = window;
    }

    /// <summary>The app's main window.</summary>
    public AutomationElement Window { get; }

    /// <summary>Starts the app and waits for its window.</summary>
    public static AppSession Start(params string[] arguments)
    {
        var start = new ProcessStartInfo(Executable()) { UseShellExecute = false };
        foreach (var argument in arguments)
        {
            start.ArgumentList.Add(argument);
        }
        var process = Process.Start(start) ?? throw new InvalidOperationException("the app did not start");
        try
        {
            var deadline = DateTime.UtcNow + StartPatience;
            while (true)
            {
                if (process.HasExited)
                {
                    throw new InvalidOperationException($"the app exited with code {process.ExitCode} before showing a window");
                }
                process.Refresh();
                if (process.MainWindowHandle != IntPtr.Zero)
                {
                    break;
                }
                if (DateTime.UtcNow > deadline)
                {
                    throw new TimeoutException($"the app showed no window within {StartPatience.TotalSeconds:0} s");
                }
                Thread.Sleep(100);
            }
            return new AppSession(process, AutomationElement.FromHandle(process.MainWindowHandle));
        }
        catch
        {
            Stop(process);
            throw;
        }
    }

    /// <summary>The element with this automation id, waiting for it to appear.</summary>
    public AutomationElement Find(string automationId)
    {
        var condition = new PropertyCondition(AutomationElement.AutomationIdProperty, automationId);
        AutomationElement? found = null;
        Wait(() => (found = Window.FindFirst(TreeScope.Descendants, condition)) is not null, $"no element '{automationId}'");
        return found!;
    }

    /// <summary>Selects a sidebar entry the way assistive technology does.</summary>
    public void Select(string automationId)
    {
        var item = Find(automationId);
        if (item.TryGetCurrentPattern(SelectionItemPattern.Pattern, out var selection))
        {
            ((SelectionItemPattern)selection).Select();
        }
        else if (item.TryGetCurrentPattern(InvokePattern.Pattern, out var invoke))
        {
            ((InvokePattern)invoke).Invoke();
        }
        else
        {
            throw new InvalidOperationException($"'{automationId}' can be neither selected nor invoked");
        }
    }

    /// <summary>Waits until the element with this automation id has this name.</summary>
    public void WaitForName(string automationId, string expected)
    {
        var last = "";
        Wait(
            () => (last = Find(automationId).Current.Name) == expected,
            () => $"'{automationId}' reads '{last}', expected '{expected}'");
    }

    /// <summary>Every element under the window, in tree order.</summary>
    public IReadOnlyList<AutomationElement> Elements() =>
        Window.FindAll(TreeScope.Descendants, Condition.TrueCondition).Cast<AutomationElement>().ToList();

    /// <summary>Saves a picture of the window as <paramref name="name"/>.png, if a screenshot directory is set.</summary>
    public string? Screenshot(string name)
    {
        var directory = Environment.GetEnvironmentVariable(ScreenshotsVariable);
        if (string.IsNullOrEmpty(directory))
        {
            return null;
        }
        Directory.CreateDirectory(directory);
        SetForegroundWindow(process.MainWindowHandle);
        // Page transitions and the backdrop settle within a few hundred milliseconds.
        Thread.Sleep(800);
        var bounds = Window.Current.BoundingRectangle;
        using var bitmap = new Bitmap((int)bounds.Width, (int)bounds.Height);
        using (var graphics = Graphics.FromImage(bitmap))
        {
            graphics.CopyFromScreen((int)bounds.Left, (int)bounds.Top, 0, 0, bitmap.Size);
        }
        var path = Path.Combine(directory, name + ".png");
        bitmap.Save(path, ImageFormat.Png);
        return path;
    }

    /// <summary>Types into a text box the way assistive technology does.</summary>
    public void SetText(string automationId, string text)
    {
        var box = Find(automationId);
        if (!box.TryGetCurrentPattern(ValuePattern.Pattern, out var value))
        {
            throw new InvalidOperationException($"'{automationId}' takes no text");
        }
        ((ValuePattern)value).SetValue(text);
    }

    /// <summary>Waits for the app to exit by itself and gives its exit code.</summary>
    public int WaitForExit(TimeSpan patience)
    {
        if (!process.WaitForExit((int)patience.TotalMilliseconds))
        {
            throw new TimeoutException($"the app was still running after {patience.TotalSeconds:0} s");
        }
        return process.ExitCode;
    }

    /// <summary>The directory for screenshots and other things worth keeping from a run, or null.</summary>
    public static string? KeepDirectory()
    {
        var directory = Environment.GetEnvironmentVariable(ScreenshotsVariable);
        if (string.IsNullOrEmpty(directory))
        {
            return null;
        }
        Directory.CreateDirectory(directory);
        return directory;
    }

    public void Dispose() => Stop(process);

    private static void Stop(Process process)
    {
        try
        {
            if (!process.HasExited)
            {
                process.CloseMainWindow();
                if (!process.WaitForExit(5000))
                {
                    process.Kill(entireProcessTree: true);
                    process.WaitForExit(5000);
                }
            }
        }
        catch (InvalidOperationException)
        {
            // Already gone.
        }
        finally
        {
            process.Dispose();
        }
    }

    private static void Wait(Func<bool> done, string failure) => Wait(done, () => failure);

    private static void Wait(Func<bool> done, Func<string> failure)
    {
        var deadline = DateTime.UtcNow + Patience;
        while (!done())
        {
            if (DateTime.UtcNow > deadline)
            {
                throw new TimeoutException(failure());
            }
            Thread.Sleep(50);
        }
    }

    /// <summary>The app's executable: named by ANIXOPS_NET_APP_EXE, or the one this solution built.</summary>
    public static string Executable()
    {
        var named = Environment.GetEnvironmentVariable(ExecutableVariable);
        if (!string.IsNullOrEmpty(named))
        {
            return named;
        }
        // tests/AnixOps.Net.App.UiTests/bin/<configuration>/<framework>/ is four levels below apps/windows-native.
        var root = new DirectoryInfo(AppContext.BaseDirectory);
        while (root is not null && !File.Exists(Path.Combine(root.FullName, "AnixOps.Net.slnx")))
        {
            root = root.Parent;
        }
        if (root is null)
        {
            throw new FileNotFoundException($"set {ExecutableVariable}: the solution directory was not found above {AppContext.BaseDirectory}");
        }
        var output = Path.Combine(root.FullName, "src", "AnixOps.Net.App", "bin");
        var separator = Path.DirectorySeparatorChar;
        var built = Directory.Exists(output)
            ? Directory.EnumerateFiles(output, ExecutableName, SearchOption.AllDirectories)
                .Where(path => path.Contains($"{separator}{Configuration}{separator}", StringComparison.OrdinalIgnoreCase))
                .OrderByDescending(File.GetLastWriteTimeUtc)
                .FirstOrDefault()
            : null;
        return built ?? throw new FileNotFoundException($"set {ExecutableVariable}: no {Configuration} build of {ExecutableName} under {output}");
    }

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetForegroundWindow(IntPtr window);
}
