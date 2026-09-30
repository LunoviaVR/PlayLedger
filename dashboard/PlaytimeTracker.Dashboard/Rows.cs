using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Shapes;

namespace PlaytimeTracker.Dashboard;

/// <summary>
/// List rows built in code: a title (with a green dot while playing), a caption under it and a value on the right.
/// Used where an x:Bind template trips the XAML compiler (App.xaml resources, the Overview's games list). The row's
/// Tag holds its item, so ItemClick handlers read it from there.
/// </summary>
public static class Rows
{
    public static Grid Create(object tag, string title, string caption, string value, string automationName, bool live = false, bool strong = false)
    {
        var grid = new Grid { Padding = new Thickness(0, 8, 0, 8), ColumnSpacing = 16, Tag = tag };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        AutomationProperties.SetName(grid, automationName);

        var heading = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        var name = new TextBlock { Text = title, TextTrimming = TextTrimming.CharacterEllipsis };
        if (strong)
            name.Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"];
        heading.Children.Add(name);
        if (live)
        {
            heading.Children.Add(new Ellipse
            {
                Width = 8,
                Height = 8,
                VerticalAlignment = VerticalAlignment.Center,
                Fill = (Microsoft.UI.Xaml.Media.Brush)Application.Current.Resources["SystemFillColorSuccessBrush"],
            });
        }
        var names = new StackPanel();
        names.Children.Add(heading);
        names.Children.Add(new TextBlock { Text = caption, Style = (Style)Application.Current.Resources["CaptionStyle"] });

        var right = new TextBlock { Text = value, VerticalAlignment = VerticalAlignment.Center };
        Grid.SetColumn(right, 1);
        grid.Children.Add(names);
        grid.Children.Add(right);
        return grid;
    }

    /// <summary>The item a clicked row carries.</summary>
    public static T? ItemOf<T>(object? clicked) where T : class => (clicked as FrameworkElement)?.Tag as T;
}
