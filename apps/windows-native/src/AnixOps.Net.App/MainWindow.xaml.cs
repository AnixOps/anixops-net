using System.Runtime.InteropServices;
using AnixOps.Net.App.Pages;
using AnixOps.Net.Presentation;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Animation;

namespace AnixOps.Net.App;

/// <summary>The main window: the app's own title bar over a sidebar and the current page.</summary>
public sealed partial class MainWindow : Window
{
    // In effective pixels; the window is sized for the monitor's scale when it opens.
    private const int InitialWidth = 1180;
    private const int InitialHeight = 760;

    private readonly Strings strings;
    private readonly Launch launch;
    private IReadOnlyList<RequestRow>? demoRequests;

    public MainWindow(Launch launch, Strings strings)
    {
        this.strings = strings;
        this.launch = launch;
        InitializeComponent();

        Title = strings["app.name"];
        AppName.Text = strings["app.name"];
        AppStage.Text = strings["app.preview"];
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);

        Root.RequestedTheme = launch.Appearance switch
        {
            Appearance.Light => ElementTheme.Light,
            Appearance.Dark => ElementTheme.Dark,
            _ => ElementTheme.Default,
        };
        // The caption buttons are drawn by the system and do not follow the content's theme on their own.
        AppWindow.TitleBar.PreferredTheme = launch.Appearance switch
        {
            Appearance.Light => TitleBarTheme.Light,
            Appearance.Dark => TitleBarTheme.Dark,
            _ => TitleBarTheme.UseDefaultAppMode,
        };

        var scale = GetDpiForWindow(WinRT.Interop.WindowNative.GetWindowHandle(this)) / 96.0;
        AppWindow.Resize(new Windows.Graphics.SizeInt32((int)(InitialWidth * scale), (int)(InitialHeight * scale)));

        AutomationProperties.SetName(Navigation, strings["nav.label"]);
        NavigationViewItem? first = null;
        foreach (var section in Sections.All)
        {
            var item = new NavigationViewItem
            {
                Content = strings[section.Title],
                Tag = section.Id,
                Icon = new FontIcon { Glyph = section.Glyph },
            };
            AutomationProperties.SetAutomationId(item, section.AutomationId);
            (section.Footer ? Navigation.FooterMenuItems : Navigation.MenuItems).Add(item);
            if (section.Id == launch.Page)
            {
                first = item;
            }
        }
        Navigation.SelectedItem = first;
    }

    private void OnSelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (args.SelectedItem is not NavigationViewItem { Tag: string id } || Sections.Find(id) is not { } section)
        {
            return;
        }
        var transition = args.RecommendedNavigationTransitionInfo ?? new EntranceNavigationTransitionInfo();
        if (section.Id == Sections.Activity)
        {
            demoRequests ??= DemoRequests.Make(launch.DemoRows);
            PageFrame.Navigate(
                typeof(RequestsPage),
                new RequestsPage.Model(strings, demoRequests, Demo: launch.DemoRows > 0, launch.Measure),
                transition);
        }
        else
        {
            PageFrame.Navigate(
                typeof(SectionPage),
                new SectionPage.Model(strings[section.Title], strings[section.Summary], strings["page.unfinished"]),
                transition);
        }
    }

    [DllImport("user32.dll")]
    private static extern uint GetDpiForWindow(IntPtr window);
}
