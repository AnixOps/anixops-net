using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Management;
using System.Text.Json;
using System.Threading;
using System.Windows.Automation;
using Xunit;
using Xunit.Abstractions;

namespace AnixOps.Net.App.UiTests;

/// <summary>
/// The second pass mark of docs/architecture/desktop-native.md ("过关标准" 2): on the same machine, the native
/// app is usable sooner after starting and holds less memory when idle than the released web client.
/// </summary>
public sealed class StartupTests
{
    /// <summary>The released web client's executable. CI downloads the latest release and sets this.</summary>
    public const string WebClientVariable = "ANIXOPS_NET_WEB_CLIENT_EXE";

    private const int Runs = 6;
    private static readonly TimeSpan StartPatience = TimeSpan.FromSeconds(90);
    private static readonly TimeSpan Idle = TimeSpan.FromSeconds(10);

    private readonly ITestOutputHelper output;

    public StartupTests(ITestOutputHelper output)
    {
        this.output = output;
    }

    /// <summary>One start of a program.</summary>
    /// <param name="StartMilliseconds">From creating the process until its content could be found by UI Automation.</param>
    /// <param name="PrivateBytes">Memory only this program holds, over its whole process tree, after idling.</param>
    /// <param name="WorkingSetBytes">Memory in RAM, over its whole process tree, after idling.</param>
    /// <param name="Processes">How many processes the tree had.</param>
    public sealed record Run(double StartMilliseconds, long PrivateBytes, long WorkingSetBytes, int Processes);

    /// <summary>All starts of one program; the first is kept apart, since it creates caches the later ones reuse.</summary>
    public sealed record Subject(string Name, string Executable, Run First, IReadOnlyList<Run> Later)
    {
        public double MedianStartMilliseconds => Median(Later.Select(run => run.StartMilliseconds));
        public double MedianPrivateBytes => Median(Later.Select(run => (double)run.PrivateBytes));
        public double MedianWorkingSetBytes => Median(Later.Select(run => (double)run.WorkingSetBytes));
    }

    public sealed record Comparison(Subject Native, Subject Web);

    [Fact]
    public void TheNativeAppStartsSoonerAndIdlesLighterThanTheWebClient()
    {
        var web = Environment.GetEnvironmentVariable(WebClientVariable);
        if (string.IsNullOrEmpty(web))
        {
            // Without a released web client beside it there is nothing to compare; CI always provides one.
            Assert.False(
                string.Equals(Environment.GetEnvironmentVariable("CI"), "true", StringComparison.OrdinalIgnoreCase),
                $"{WebClientVariable} is not set, so the comparison did not run");
            return;
        }
        var native = AppSession.Executable();

        // Alternating, so that whatever else the machine is doing falls on both alike.
        var nativeRuns = new List<Run>();
        var webRuns = new List<Run>();
        for (var run = 0; run < Runs; run++)
        {
            nativeRuns.Add(Measure(native, ["--language", "en"], NativeIsReady));
            webRuns.Add(Measure(web, [], WebIsReady));
        }
        var comparison = new Comparison(
            new Subject("native", native, nativeRuns[0], nativeRuns.Skip(1).ToList()),
            new Subject("web", web, webRuns[0], webRuns.Skip(1).ToList()));

        var text = JsonSerializer.Serialize(comparison, new JsonSerializerOptions { WriteIndented = true });
        output.WriteLine(text);
        if (AppSession.KeepDirectory() is { } directory)
        {
            File.WriteAllText(Path.Combine(directory, "startup-measurement.json"), text);
        }

        Assert.True(
            comparison.Native.MedianStartMilliseconds < comparison.Web.MedianStartMilliseconds,
            $"start: native {comparison.Native.MedianStartMilliseconds:0} ms, web {comparison.Web.MedianStartMilliseconds:0} ms");
        Assert.True(
            comparison.Native.MedianPrivateBytes < comparison.Web.MedianPrivateBytes,
            $"idle private memory: native {comparison.Native.MedianPrivateBytes / 1e6:0.0} MB, web {comparison.Web.MedianPrivateBytes / 1e6:0.0} MB");
        Assert.True(
            comparison.Native.MedianWorkingSetBytes < comparison.Web.MedianWorkingSetBytes,
            $"idle working set: native {comparison.Native.MedianWorkingSetBytes / 1e6:0.0} MB, web {comparison.Web.MedianWorkingSetBytes / 1e6:0.0} MB");
    }

    // The native app is usable when its page heading is there.
    private static bool NativeIsReady(AutomationElement window) =>
        window.FindFirst(TreeScope.Descendants, new PropertyCondition(AutomationElement.AutomationIdProperty, "Page.Title")) is not null;

    // The web client is usable when its page has put a named button inside the web view's document.
    private static bool WebIsReady(AutomationElement window)
    {
        var document = window.FindFirst(TreeScope.Descendants, new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Document));
        if (document is null)
        {
            return false;
        }
        var buttons = document.FindAll(TreeScope.Descendants, new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Button));
        return buttons.Cast<AutomationElement>().Any(button => !string.IsNullOrWhiteSpace(button.Current.Name));
    }

    private static Run Measure(string executable, string[] arguments, Func<AutomationElement, bool> ready)
    {
        var start = new ProcessStartInfo(executable)
        {
            UseShellExecute = false,
            WorkingDirectory = Path.GetDirectoryName(executable) ?? "",
        };
        foreach (var argument in arguments)
        {
            start.ArgumentList.Add(argument);
        }
        var watch = Stopwatch.StartNew();
        using var process = Process.Start(start) ?? throw new InvalidOperationException($"{executable} did not start");
        try
        {
            while (true)
            {
                if (process.HasExited)
                {
                    throw new InvalidOperationException($"{executable} exited with code {process.ExitCode} before it was usable");
                }
                if (watch.Elapsed > StartPatience)
                {
                    throw new TimeoutException($"{executable} was not usable within {StartPatience.TotalSeconds:0} s");
                }
                process.Refresh();
                if (process.MainWindowHandle != IntPtr.Zero && IsReady(process.MainWindowHandle, ready))
                {
                    break;
                }
                Thread.Sleep(10);
            }
            var started = watch.Elapsed.TotalMilliseconds;

            Thread.Sleep(Idle);
            var tree = Tree(process.Id);
            long privateBytes = 0;
            long workingSet = 0;
            var counted = 0;
            foreach (var id in tree)
            {
                try
                {
                    using var member = Process.GetProcessById(id);
                    privateBytes += member.PrivateMemorySize64;
                    workingSet += member.WorkingSet64;
                    counted++;
                }
                catch (ArgumentException)
                {
                    // Gone since the list was read.
                }
                catch (InvalidOperationException)
                {
                }
                catch (System.ComponentModel.Win32Exception)
                {
                    // A process this user may not read.
                }
            }
            return new Run(started, privateBytes, workingSet, counted);
        }
        finally
        {
            try
            {
                if (!process.HasExited)
                {
                    process.Kill(entireProcessTree: true);
                    process.WaitForExit(10_000);
                }
            }
            catch (InvalidOperationException)
            {
            }
            // Let the machine settle before the next program starts.
            Thread.Sleep(1000);
        }
    }

    private static bool IsReady(IntPtr window, Func<AutomationElement, bool> ready)
    {
        try
        {
            return ready(AutomationElement.FromHandle(window));
        }
        catch (ElementNotAvailableException)
        {
            // The window was replaced while it was being read.
            return false;
        }
    }

    /// <summary>The ids of a process and everything it started, directly or not.</summary>
    private static List<int> Tree(int root)
    {
        var children = new Dictionary<int, List<int>>();
        using (var searcher = new ManagementObjectSearcher("SELECT ProcessId, ParentProcessId FROM Win32_Process"))
        using (var all = searcher.Get())
        {
            foreach (var entry in all)
            {
                var id = Convert.ToInt32(entry["ProcessId"]);
                var parent = Convert.ToInt32(entry["ParentProcessId"]);
                if (!children.TryGetValue(parent, out var list))
                {
                    children[parent] = list = [];
                }
                list.Add(id);
                entry.Dispose();
            }
        }
        var tree = new List<int>();
        var pending = new Queue<int>();
        pending.Enqueue(root);
        while (pending.Count > 0)
        {
            var id = pending.Dequeue();
            if (tree.Contains(id))
            {
                continue;
            }
            tree.Add(id);
            if (children.TryGetValue(id, out var list))
            {
                foreach (var child in list)
                {
                    pending.Enqueue(child);
                }
            }
        }
        return tree;
    }

    private static double Median(IEnumerable<double> values)
    {
        var sorted = values.Order().ToArray();
        return sorted.Length % 2 == 1 ? sorted[sorted.Length / 2] : (sorted[sorted.Length / 2 - 1] + sorted[sorted.Length / 2]) / 2;
    }
}
