using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace AnixOps.Net.App.Pages;

/// <summary>A page of the shell before its real content exists.</summary>
public sealed partial class SectionPage : Page
{
    /// <summary>What the page shows, already in the interface language.</summary>
    public sealed record Model(string Title, string Summary, string Notice);

    public SectionPage()
    {
        InitializeComponent();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        if (e.Parameter is Model content)
        {
            Heading.Text = content.Title;
            Summary.Text = content.Summary;
            Unfinished.Message = content.Notice;
        }
    }
}
