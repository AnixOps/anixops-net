using System;
using System.Diagnostics;
using System.IO;

namespace AnixOps.Net.Testing;

/// <summary>
/// The real core service (apps/windows-service), run in the foreground on a pipe and a data folder of the
/// test's own, so that it neither meets an installed service nor touches the machine's data.
/// </summary>
public sealed class ServiceProcess : IDisposable
{
    /// <summary>The service's executable, <c>anixops-net-service.exe</c>. CI builds it and sets this.</summary>
    public const string ExecutableVariable = "ANIXOPS_NET_SERVICE_EXE";

    private readonly Process process;
    private readonly bool ownsDirectory;

    private ServiceProcess(Process process, string pipeName, string directory, bool ownsDirectory)
    {
        this.process = process;
        PipeName = pipeName;
        Directory = directory;
        this.ownsDirectory = ownsDirectory;
    }

    /// <summary>The pipe's name without <c>\\.\pipe\</c>, as a window is given it.</summary>
    public string PipeName { get; }

    /// <summary>Where this service keeps its data.</summary>
    public string Directory { get; }

    /// <summary>The service's process id.</summary>
    public int Id => process.Id;

    /// <summary>The executable to test against, or null when none was given.</summary>
    public static string? Executable()
    {
        var named = Environment.GetEnvironmentVariable(ExecutableVariable);
        return string.IsNullOrEmpty(named) ? null : named;
    }

    /// <summary>
    /// Whether the tests against the real service can run. In CI they must: a missing executable there is a
    /// failure, never a silent pass.
    /// </summary>
    public static bool Available()
    {
        if (Executable() is not null)
        {
            return true;
        }
        if (string.Equals(Environment.GetEnvironmentVariable("CI"), "true", StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException($"{ExecutableVariable} is not set, so the tests against the real service did not run");
        }
        return false;
    }

    /// <summary>Starts the service. Give the pipe and folder of an earlier one to start it again as it was.</summary>
    public static ServiceProcess Start(string? pipeName = null, string? directory = null)
    {
        var executable = Executable() ?? throw new InvalidOperationException($"{ExecutableVariable} is not set");
        pipeName ??= "AnixOpsNet.Test." + Guid.NewGuid().ToString("N");
        var owns = directory is null;
        directory ??= Path.Combine(Path.GetTempPath(), "anixops-net-service-" + Guid.NewGuid().ToString("N"));
        var start = new ProcessStartInfo(executable)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            WorkingDirectory = Path.GetDirectoryName(executable) ?? "",
        };
        start.ArgumentList.Add("--console");
        start.ArgumentList.Add("--dir");
        start.ArgumentList.Add(directory);
        start.ArgumentList.Add("--pipe");
        start.ArgumentList.Add(@"\\.\pipe\" + pipeName);
        var process = Process.Start(start) ?? throw new InvalidOperationException($"{executable} did not start");
        return new ServiceProcess(process, pipeName, directory, owns);
    }

    /// <summary>Whether the service has ended, and with what code.</summary>
    public bool HasExited(out int code)
    {
        code = process.HasExited ? process.ExitCode : 0;
        return process.HasExited;
    }

    /// <summary>Ends the service the hard way, as a crash or a forced stop would.</summary>
    public void Stop()
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
            // Already gone.
        }
    }

    public void Dispose()
    {
        Stop();
        process.Dispose();
        if (ownsDirectory)
        {
            try
            {
                System.IO.Directory.Delete(Directory, recursive: true);
            }
            catch (Exception error) when (error is IOException or UnauthorizedAccessException)
            {
                // The service closes its folder to other users; what a test leaves in the temporary folder
                // of a CI machine is thrown away with the machine.
            }
        }
    }
}
