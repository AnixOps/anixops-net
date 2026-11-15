using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Security.Cryptography.X509Certificates;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Automation;
using AnixOps.Net.Ipc;
using AnixOps.Net.Session;
using Microsoft.Win32;
using Xunit;

namespace AnixOps.Net.App.UiTests;

/// <summary>
/// Answers the confirmation Windows itself shows before a root certificate is added to or removed from the
/// user's own store, the way the user would: through the dialog's buttons.
/// </summary>
internal static class RootConfirmation
{
    // The dialogs' titles on an English Windows, in case Windows shows them from another process.
    private static readonly string[] Titles = { "Security Warning", "Root Certificate Store" };

    private static IEnumerable<AutomationElement> TopLevel() =>
        AutomationElement.RootElement.FindAll(TreeScope.Children, Condition.TrueCondition).Cast<AutomationElement>();

    private static string Describe(AutomationElement window)
    {
        try
        {
            return $"'{window.Current.Name}' (process {window.Current.ProcessId})";
        }
        catch (ElementNotAvailableException)
        {
            return "(gone)";
        }
    }

    /// <summary>The dialogs whose button was pressed last, for the test's notes.</summary>
    public static string LastPressed { get; private set; } = "";

    private static bool Press(string button)
    {
        foreach (var window in TopLevel())
        {
            try
            {
                if (window.Current.ProcessId != Environment.ProcessId && !Titles.Contains(window.Current.Name))
                {
                    continue;
                }
                var found = window.FindFirst(
                    TreeScope.Descendants,
                    new AndCondition(
                        new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Button),
                        new PropertyCondition(AutomationElement.NameProperty, button)));
                if (found is not null && found.TryGetCurrentPattern(InvokePattern.Pattern, out var pattern))
                {
                    LastPressed = $"'{button}' in {Describe(window)}";
                    ((InvokePattern)pattern).Invoke();
                    return true;
                }
            }
            catch (ElementNotAvailableException)
            {
                // The dialog closed while it was looked at.
            }
        }
        return false;
    }

    /// <summary>
    /// Runs <paramref name="work"/> and presses <paramref name="button"/> ("Yes" or "No") on every
    /// confirmation Windows shows until the work has ended. Answers whether Windows asked at all: it does
    /// on a person's desktop, and does not on the CI machine, whose account is not asked.
    /// </summary>
    public static async Task<bool> AnswerAsync(string button, Func<Task> work)
    {
        LastPressed = "";
        var running = Task.Run(work);
        var deadline = DateTime.UtcNow + TimeSpan.FromSeconds(60);
        var pressed = 0;
        while (!running.IsCompleted)
        {
            if (Press(button))
            {
                pressed++;
                await Task.Delay(500);
                continue;
            }
            Assert.True(
                DateTime.UtcNow < deadline,
                $"no '{button}' button to press after {pressed} presses; the windows on the desktop: {string.Join(", ", TopLevel().Select(Describe))}");
            await Task.Delay(100);
        }
        try
        {
            await running;
        }
        catch (RefusedException) when (pressed == 0)
        {
            // Refused without a dialog to answer: say so, since the caller cannot tell otherwise.
            LastPressed = "(refused without a dialog)";
            throw;
        }
        return pressed > 0;
    }
}

/// <summary>
/// The user's own certificate store, for real: a root added there is accepted by Windows, and what
/// removing the client does (deleting the certificate's key in the user's settings) takes the trust away.
/// Where Windows asks the user before adding a root, the test answers, also with No.
///
/// These show a dialog on the desktop, so they run only where they are asked for
/// (<see cref="Variable"/>), in a CI step of their own.
/// </summary>
public sealed class RootStoreTests
{
    /// <summary>Set to 1 to run these tests.</summary>
    public const string Variable = "ANIXOPS_NET_ROOT_TESTS";

    private const string UserRoots = @"Software\Microsoft\SystemCertificates\Root\Certificates";

    private static bool Asked() => Environment.GetEnvironmentVariable(Variable) == "1";

    private static bool Installed() => Environment.GetEnvironmentVariable(InstalledTests.InstalledVariable) == "1";

    /// <summary>A certificate authority of the test's own, as the client's root is one.</summary>
    private static X509Certificate2 NewRoot()
    {
        using var key = ECDsa.Create(ECCurve.NamedCurves.nistP256);
        var request = new CertificateRequest($"CN=AnixOps Net test root {Guid.NewGuid():N}", key, HashAlgorithmName.SHA256);
        request.CertificateExtensions.Add(new X509BasicConstraintsExtension(certificateAuthority: true, hasPathLengthConstraint: false, pathLengthConstraint: 0, critical: true));
        request.CertificateExtensions.Add(new X509KeyUsageExtension(X509KeyUsageFlags.KeyCertSign | X509KeyUsageFlags.CrlSign, critical: true));
        using var withKey = request.CreateSelfSigned(DateTimeOffset.UtcNow.AddMinutes(-5), DateTimeOffset.UtcNow.AddDays(1));
        // Without the private key, as the window gets the root from the core.
        return X509Certificate2.CreateFromPem(withKey.ExportCertificatePem());
    }

    /// <summary>Whether Windows accepts the certificate as a trusted root when it checks a chain.</summary>
    private static bool Accepted(X509Certificate2 certificate)
    {
        using var chain = new X509Chain();
        chain.ChainPolicy.RevocationMode = X509RevocationMode.NoCheck;
        return chain.Build(certificate);
    }

    private static bool KeyExists(string thumbprint)
    {
        using var key = Registry.CurrentUser.OpenSubKey($@"{UserRoots}\{thumbprint}");
        return key is not null;
    }

    /// <summary>What removing the client does for every signed-in user (apps/windows-service, cleanup.rs).</summary>
    private static void DeleteAsTheRemovalDoes(string thumbprint)
    {
        var reg = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System), "reg.exe");
        var start = new ProcessStartInfo(reg) { UseShellExecute = false, CreateNoWindow = true };
        start.ArgumentList.Add("delete");
        start.ArgumentList.Add($@"HKCU\{UserRoots}\{thumbprint}");
        start.ArgumentList.Add("/f");
        using var process = Process.Start(start) ?? throw new InvalidOperationException("reg did not start");
        process.WaitForExit();
    }

    /// <summary>Writes down what this machine did, for the CI log (beside the screenshots).</summary>
    private static void Note(string line)
    {
        var folder = Environment.GetEnvironmentVariable(AppSession.ScreenshotsVariable);
        if (string.IsNullOrEmpty(folder))
        {
            return;
        }
        Directory.CreateDirectory(folder);
        File.AppendAllText(Path.Combine(folder, "root-confirmation.txt"), line + Environment.NewLine);
    }

    private static void Eventually(Func<bool> done, string failure)
    {
        var deadline = DateTime.UtcNow + TimeSpan.FromSeconds(60);
        while (!done())
        {
            Assert.True(DateTime.UtcNow < deadline, failure);
            Thread.Sleep(200);
        }
    }

    [Fact]
    public async Task WindowsAsksTheUserAndTheRemovalTakesTheTrustAway()
    {
        if (!Asked() || Installed())
        {
            return;
        }
        var store = new UserRootStore();
        using var root = NewRoot();
        var pem = root.ExportCertificatePem();
        try
        {
            Assert.False(store.Trusts(pem));
            Assert.False(Accepted(root));

            // The user says Yes (where Windows asks): trusted for this user, kept where the removal looks.
            var asked = await RootConfirmation.AnswerAsync("Yes", () => Task.Run(() => store.Trust(pem)));
            Note($"Windows asked before adding the root: {asked} {RootConfirmation.LastPressed}");
            Assert.True(store.Trusts(pem));
            Assert.True(KeyExists(root.Thumbprint));
            Eventually(() => Accepted(root), "Windows does not accept the root although it is in the user's store");

            // What removing the client does needs no dialog and takes the trust away.
            DeleteAsTheRemovalDoes(root.Thumbprint);
            Assert.False(KeyExists(root.Thumbprint));
            Eventually(() => !store.Trusts(pem), "the user's store still lists the root after its key was deleted");
            Eventually(() => !Accepted(root), "Windows still accepts the root after its key was deleted");

            // The user says No to a root Windows has not been asked about before: nothing is trusted, and
            // the reason is one the core can pass on. (Windows does not ask again about a root the user
            // already agreed to, so this takes a root of its own.)
            using var other = NewRoot();
            var otherPem = other.ExportCertificatePem();
            try
            {
                var answeredNo = false;
                string outcome;
                try
                {
                    answeredNo = await RootConfirmation.AnswerAsync("No", () => Task.Run(() => store.Trust(otherPem)));
                    outcome = "trusted";
                }
                catch (RefusedException refused)
                {
                    outcome = $"refused: {refused.Message}";
                    Assert.Contains("answered with No", refused.Message, StringComparison.Ordinal);
                }
                Note($"Answering No for a new root: pressed {answeredNo} {RootConfirmation.LastPressed}, outcome {outcome}");
                // A root the user refused must never end up trusted.
                Assert.False(answeredNo, $"the root is trusted although No was pressed ({RootConfirmation.LastPressed})");
                if (outcome != "trusted")
                {
                    Assert.False(store.Trusts(otherPem));
                    Assert.False(KeyExists(other.Thumbprint));
                }
            }
            finally
            {
                DeleteAsTheRemovalDoes(other.Thumbprint);
            }

            // The user's own request to stop trusting it.
            var again = await RootConfirmation.AnswerAsync("Yes", () => Task.Run(() => store.Trust(pem)));
            Note($"Windows asked before adding the root a second time: {again} {RootConfirmation.LastPressed}");
            Assert.True(store.Trusts(pem));
            var removing = await RootConfirmation.AnswerAsync("Yes", () => Task.Run(() => store.Untrust(pem)));
            Note($"Windows asked before removing the root: {removing} {RootConfirmation.LastPressed}");
            Assert.False(store.Trusts(pem));
            Assert.False(KeyExists(root.Thumbprint));
            Eventually(() => !Accepted(root), "Windows still accepts the root after the user stopped trusting it");
        }
        finally
        {
            DeleteAsTheRemovalDoes(root.Thumbprint);
        }
    }

    /// <summary>
    /// Run by the installer's test (.github/scripts/native-msi-lifecycle.ps1): the installed service has
    /// this user's window trust its root. The root stays trusted, for the removal to take out.
    /// </summary>
    [Fact]
    public async Task TheInstalledServiceHasItsRootTrustedInTheUsersStore()
    {
        if (!Asked() || !Installed())
        {
            return;
        }
        var scratch = $@"Software\AnixOps\Net\Tests\{Guid.NewGuid():N}";
        try
        {
            var handler = new SessionHandler(
                new RegistryProxyStore($@"{scratch}\Proxy", announce: false),
                new RegistryLoginEntry(@"C:\Program Files\AnixOps\Net\AnixOpsNet.exe", $@"{scratch}\Run"),
                new UserRootStore());
            await using var link = new CoreLink("0.3.0", handler, Channel.DefaultPipeName, TimeSpan.FromMilliseconds(100));
            link.Start();
            var patience = TimeSpan.FromSeconds(120);
            var deadline = DateTime.UtcNow + TimeSpan.FromSeconds(30);
            while (true)
            {
                // Once this is answered the service has taken the window in and can ask it.
                try
                {
                    await link.AskAsync("save_preferences", JsonDocument.Parse("""{ "preferences": { "startAfterLogin": false, "autoConnect": false } }""").RootElement.Clone());
                    break;
                }
                catch (Exception problem) when (problem is RefusedException or ChannelClosedException)
                {
                    Assert.True(DateTime.UtcNow < deadline, $"the installed service did not take the window in: {problem.Message}; the link is {link.Status}");
                    await Task.Delay(100);
                }
            }

            Assert.False((await link.AskAsync("mitm_root_status")).GetProperty("trusted").GetBoolean());
            await RootConfirmation.AnswerAsync("Yes", () => link.AskAsync("open_mitm_root", patience: patience));
            var status = await link.AskAsync("mitm_root_status");
            Assert.True(status.GetProperty("generated").GetBoolean());
            Assert.True(status.GetProperty("trusted").GetBoolean());
        }
        finally
        {
            Registry.CurrentUser.DeleteSubKeyTree(scratch, throwOnMissingSubKey: false);
        }
    }
}
