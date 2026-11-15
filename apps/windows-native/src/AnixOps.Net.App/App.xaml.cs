using System.Globalization;
using AnixOps.Net.Presentation;
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
        window = new MainWindow(launch, strings);
        window.Activate();
    }
}
