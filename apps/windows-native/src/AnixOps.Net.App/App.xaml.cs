using System.Globalization;
using AnixOps.Net.Ipc;
using AnixOps.Net.Presentation;
using AnixOps.Net.Session;
using Microsoft.UI.Xaml;

namespace AnixOps.Net.App;

/// <summary>The application: one main window.</summary>
public partial class App : Application
{
    private MainWindow? window;

    public App()
    {
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        var launch = Launch.Parse(Environment.GetCommandLineArgs().Skip(1));
        var strings = Strings.For(launch.Language ?? Strings.Preferred(CultureInfo.CurrentUICulture.Name));
        // What the core asks of this user's session is answered here: the window runs as the user, without
        // administrator rights, and the core runs as a system service.
        var session = new SessionHandler(
            new RegistryProxyStore(),
            new RegistryLoginEntry(Environment.ProcessPath ?? System.IO.Path.Combine(AppContext.BaseDirectory, "AnixOpsNet.exe")));
        var version = typeof(App).Assembly.GetName().Version?.ToString(3) ?? "0.0.0";
        var core = new CoreLink(version, session, launch.Pipe ?? Channel.DefaultPipeName);
        window = new MainWindow(launch, strings, core);
        window.Closed += async (_, _) => await core.DisposeAsync();
        window.Activate();
        core.Start();
    }
}
