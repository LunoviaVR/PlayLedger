using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using PlaytimeTracker.Dashboard.ViewModels;

namespace PlaytimeTracker.Dashboard.Pages;

public sealed partial class GamesPage : Page
{
    private List<GameItem> _items = new();
    // Rebuilt only when the data changed, so artwork doesn't flicker on the periodic refresh.
    private long _shownRevision = -1;

    public GamesPage()
    {
        InitializeComponent();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        _shownRevision = -1;
        App.State.SnapshotChanged += Rebuild;
        App.State.ArtworkChanged += OnArtworkChanged;
        Rebuild();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        App.State.SnapshotChanged -= Rebuild;
        App.State.ArtworkChanged -= OnArtworkChanged;
    }

    private void Rebuild()
    {
        if (App.State.Snapshot is not { } s || s.Revision == _shownRevision)
            return;
        _shownRevision = s.Revision;
        _items = s.Games.Select(g => new GameItem(g, s)).ToList();
        Show();
        foreach (var item in _items)
            _ = item.LoadArtworkAsync();
    }

    private void Show()
    {
        var query = Search.Text.Trim();
        IEnumerable<GameItem> visible = _items.Where(i => query.Length == 0 || i.Name.Contains(query, StringComparison.CurrentCultureIgnoreCase));
        visible = Sort.SelectedIndex switch
        {
            1 => visible.OrderByDescending(i => i.Game.LastPlayed),
            2 => visible.OrderBy(i => i.Name, StringComparer.CurrentCultureIgnoreCase),
            _ => visible.OrderByDescending(i => i.Game.TotalSeconds),
        };
        var list = visible.ToList();
        var live = list.Where(i => i.Game.IsLive).ToList();
        LiveGames.ItemsSource = live;
        LivePanel.Visibility = live.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        AllGames.ItemsSource = list.Where(i => !i.Game.IsLive).ToList();
        EmptyText.Visibility = _items.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    private void OnArtworkChanged(string game, string kind)
    {
        var item = _items.FirstOrDefault(i => string.Equals(i.Name, game, StringComparison.OrdinalIgnoreCase));
        if (item is not null)
            _ = item.LoadArtworkAsync();
    }

    private void Search_TextChanged(AutoSuggestBox sender, AutoSuggestBoxTextChangedEventArgs args) => Show();

    private void Sort_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (IsLoaded)
            Show();
    }

    private async void Game_Click(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is GameItem item && App.State.Snapshot is { } s)
            await Dialogs.ShowGameAsync(XamlRoot, item.Game, s);
    }
}
