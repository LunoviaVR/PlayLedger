using System.Diagnostics;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using PlaytimeTracker.Dashboard.Core;
using Windows.Storage.Pickers;

namespace PlaytimeTracker.Dashboard.Pages;

/// <summary>An entry in one of the settings lists (custom games, folders, ignored names).</summary>
public sealed class ListEntry
{
    public ListEntry(string list, string text, string detail = "")
    {
        List = list;
        Text = text;
        Detail = detail;
    }

    public string List { get; }
    public string Text { get; }
    public string Detail { get; }
    public Visibility DetailVisibility => Detail.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
    public string RemoveName => $"Remove {Text}";
}

public sealed partial class SettingsPage : Page
{
    private TrackerSettings? _settings;
    private bool _loading;
    private bool _installed;
    private readonly Microsoft.UI.Dispatching.DispatcherQueueTimer _updatePoll;

    /// <summary>Accent choices: Windows' own accent, the presets, then a custom colour.</summary>
    private static readonly List<(string Key, string Name)> AccentChoices =
        new[] { (Accent.WindowsKey, "Windows accent") }
            .Concat(Accent.Presets.Select(p => (p.Key, p.Name)))
            .Append(("custom", "Custom…"))
            .ToList();
    private readonly Microsoft.UI.Dispatching.DispatcherQueueTimer _saveSoon;

    public SettingsPage()
    {
        InitializeComponent();
        // Number boxes save a moment after the last change rather than on every keystroke.
        _saveSoon = DispatcherQueue.CreateTimer();
        _saveSoon.Interval = TimeSpan.FromMilliseconds(600);
        _saveSoon.IsRepeating = false;
        _saveSoon.Tick += async (_, _) => await SaveAsync();
        // While an update check or download runs, keep its status current.
        _updatePoll = DispatcherQueue.CreateTimer();
        _updatePoll.Interval = TimeSpan.FromSeconds(2);
        _updatePoll.Tick += async (_, _) => await RefreshUpdateStatusAsync();
        AccentChoice.ItemsSource = AccentChoices.Select(c => c.Name).ToList();
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        await LoadAsync();
    }

    protected override async void OnNavigatedFrom(NavigationEventArgs e)
    {
        _updatePoll.Stop();
        if (_saveSoon.IsRunning)
        {
            _saveSoon.Stop();
            await SaveAsync();
        }
    }

    private async Task LoadAsync()
    {
        SettingsResponse response;
        try
        {
            response = await App.State.Client.GetSettingsAsync();
        }
        catch (Exception ex) when (ex is TrackerUnavailableException or TrackerErrorException)
        {
            Body.IsEnabled = false;
            About.Text = ex.Message;
            return;
        }
        _loading = true;
        _settings = response.Settings;
        Theme.SelectedIndex = _settings.ThemeMode switch { "light" => 1, "dark" => 2, _ => 0 };
        Notifications.IsOn = _settings.ShowNotifications;
        WindowsGameList.IsOn = _settings.UseWindowsGameList;
        PollInterval.Value = _settings.PollIntervalSeconds;
        MinimumSession.Value = _settings.MinimumSessionSeconds;
        GracePeriod.Value = _settings.GracePeriodSeconds;
        OnlineArtwork.IsOn = _settings.OnlineArtwork;
        CheckUpdates.IsOn = _settings.CheckForUpdates;
        AutoUpdates.IsOn = _settings.InstallUpdatesAutomatically;
        ShowAccent(_settings.AccentColor);
        _installed = response.IsInstalledCopy;
        StartWithWindows.IsOn = response.StartWithWindows;
        StartWithWindows.IsEnabled = _installed;
        StartupNote.Text = _installed
            ? "You can also change this in Task Manager → Startup apps."
            : "Available once Playtime Tracker is installed.";
        ShowKeyState(response.HasSteamGridDbKey);
        ShowLists();
        var tracker = App.State.Client.TrackerVersion ?? "unknown";
        var dashboard = typeof(SettingsPage).Assembly.GetName().Version?.ToString(3) ?? "unknown";
        About.Text = $"Playtime Tracker {tracker} (dashboard {dashboard}). Your play history stays on this PC.";
        Body.IsEnabled = true;
        _loading = false;
        await RefreshUpdateStatusAsync();
    }

    private void ShowAccent(string accent)
    {
        var key = accent.Trim().ToLowerInvariant();
        var index = AccentChoices.FindIndex(c => c.Key == key);
        var custom = index < 0;
        AccentChoice.SelectedIndex = custom ? AccentChoices.Count - 1 : index;
        CustomAccentPanel.Visibility = custom ? Visibility.Visible : Visibility.Collapsed;
        if (Accent.Resolve(key) is { } color)
            CustomAccent.Color = Microsoft.UI.ColorHelper.FromArgb(255, color.R, color.G, color.B);
    }

    private async void Accent_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_settings is null || _loading || AccentChoice.SelectedIndex < 0)
            return;
        var key = AccentChoices[AccentChoice.SelectedIndex].Key;
        if (key == "custom")
        {
            CustomAccentPanel.Visibility = Visibility.Visible;
            return;
        }
        CustomAccentPanel.Visibility = Visibility.Collapsed;
        _settings.AccentColor = key;
        MainWindow.SetAccent(this, key);
        await SaveAsync();
    }

    private async void ApplyCustomAccent_Click(object sender, RoutedEventArgs e)
    {
        if (_settings is null)
            return;
        var c = CustomAccent.Color;
        var hex = new Rgb(c.R, c.G, c.B).ToHex();
        _settings.AccentColor = hex;
        MainWindow.SetAccent(this, hex);
        await SaveAsync();
    }

    private async void StartWithWindows_Toggled(object sender, RoutedEventArgs e)
    {
        if (_loading || !_installed)
            return;
        if (!await App.State.RunAsync(c => c.SetStartWithWindowsAsync(StartWithWindows.IsOn)))
        {
            _loading = true;
            StartWithWindows.IsOn = !StartWithWindows.IsOn;
            _loading = false;
        }
    }

    private async Task RefreshUpdateStatusAsync()
    {
        try
        {
            ShowUpdateStatus(await App.State.Client.GetUpdateStatusAsync());
        }
        catch (Exception ex) when (ex is TrackerUnavailableException or TrackerErrorException)
        {
            _updatePoll.Stop();
        }
    }

    private void ShowUpdateStatus(UpdateStatusResponse status)
    {
        var lines = new List<string>();
        if (status.Busy)
            lines.Add("Working…");
        else if (status.AvailableVersion is { } version)
            lines.Add($"Version {version} is available (you have {status.CurrentVersion}).");
        else if (status.LastChecked is not null)
            lines.Add($"You're up to date ({status.CurrentVersion}).");
        else
            lines.Add($"Version {status.CurrentVersion}.");
        if (status.LastChecked is { } checkedAt)
            lines.Add($"Last checked {Format.Day(checkedAt, DateTimeOffset.Now)} at {Format.Time(checkedAt)}.");
        if (status.LastError is { } error)
            lines.Add(error);
        UpdateStatus.Text = string.Join(" ", lines);
        InstallNow.Visibility = status.CanInstall && !status.Busy ? Visibility.Visible : Visibility.Collapsed;
        CheckNow.IsEnabled = !status.Busy;
        if (status.Busy)
            _updatePoll.Start();
        else
            _updatePoll.Stop();
    }

    private async void CheckNow_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            ShowUpdateStatus(await App.State.Client.CheckForUpdatesAsync());
            _updatePoll.Start();
        }
        catch (Exception ex) when (ex is TrackerUnavailableException or TrackerErrorException)
        {
            App.State.ReportError(ex.Message);
        }
    }

    private async void InstallNow_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            ShowUpdateStatus(await App.State.Client.InstallUpdateAsync());
            _updatePoll.Start();
        }
        catch (Exception ex) when (ex is TrackerUnavailableException or TrackerErrorException)
        {
            App.State.ReportError(ex.Message);
        }
    }

    private void ShowKeyState(bool hasKey)
    {
        KeyStatus.Text = hasKey ? "A key is saved." : "No key saved.";
        RemoveKey.IsEnabled = hasKey;
    }

    private void ShowLists()
    {
        if (_settings is null)
            return;
        CustomGames.ItemsSource = _settings.CustomGames.Select(g => new ListEntry("custom", g.Name, g.Executable)).ToList();
        Folders.ItemsSource = _settings.ExtraGameFolders.Select(f => new ListEntry("folders", f)).ToList();
        IgnoredGames.ItemsSource = _settings.IgnoredGames.Select(g => new ListEntry("games", g)).ToList();
        IgnoredPrograms.ItemsSource = _settings.IgnoredExecutables.Select(p => new ListEntry("programs", p)).ToList();
    }

    private async Task SaveAsync()
    {
        if (_settings is null || _loading)
            return;
        if (await App.State.RunAsync(c => c.UpdateSettingsAsync(_settings)))
            _ = App.State.RefreshAsync();
    }

    private async void Theme_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_settings is null || _loading)
            return;
        _settings.ThemeMode = Theme.SelectedIndex switch { 1 => "light", 2 => "dark", _ => "system" };
        MainWindow.SetTheme(this, _settings.ThemeMode);
        await SaveAsync();
    }

    private async void Toggle_Changed(object sender, RoutedEventArgs e)
    {
        if (_settings is null || _loading)
            return;
        _settings.ShowNotifications = Notifications.IsOn;
        _settings.UseWindowsGameList = WindowsGameList.IsOn;
        _settings.OnlineArtwork = OnlineArtwork.IsOn;
        _settings.CheckForUpdates = CheckUpdates.IsOn;
        _settings.InstallUpdatesAutomatically = AutoUpdates.IsOn;
        await SaveAsync();
    }

    private void Number_Changed(NumberBox sender, NumberBoxValueChangedEventArgs args)
    {
        if (_settings is null || _loading || double.IsNaN(args.NewValue))
            return;
        _settings.PollIntervalSeconds = (int)PollInterval.Value;
        _settings.MinimumSessionSeconds = (int)MinimumSession.Value;
        _settings.GracePeriodSeconds = (int)GracePeriod.Value;
        _saveSoon.Stop();
        _saveSoon.Start();
    }

    private async void SaveKey_Click(object sender, RoutedEventArgs e)
    {
        var key = SteamGridDbKey.Password.Trim();
        if (key.Length == 0)
            return;
        if (await App.State.RunAsync(c => c.SetSteamGridDbKeyAsync(key)))
        {
            SteamGridDbKey.Password = "";
            ShowKeyState(true);
        }
    }

    private async void RemoveKey_Click(object sender, RoutedEventArgs e)
    {
        if (await App.State.RunAsync(c => c.SetSteamGridDbKeyAsync(null)))
            ShowKeyState(false);
    }

    private async void RemoveEntry_Click(object sender, RoutedEventArgs e)
    {
        if (_settings is null || (sender as FrameworkElement)?.Tag is not ListEntry entry)
            return;
        switch (entry.List)
        {
            case "custom":
                _settings.CustomGames = _settings.CustomGames.Where(g => g.Name != entry.Text || g.Executable != entry.Detail).ToList();
                break;
            case "folders":
                _settings.ExtraGameFolders = _settings.ExtraGameFolders.Where(f => f != entry.Text).ToList();
                break;
            case "games":
                _settings.IgnoredGames = _settings.IgnoredGames.Where(g => g != entry.Text).ToList();
                break;
            case "programs":
                _settings.IgnoredExecutables = _settings.IgnoredExecutables.Where(p => p != entry.Text).ToList();
                break;
        }
        ShowLists();
        await SaveAsync();
    }

    private static void InitializePicker(object picker)
    {
        if (App.CurrentWindow is { } window)
            WinRT.Interop.InitializeWithWindow.Initialize(picker, WinRT.Interop.WindowNative.GetWindowHandle(window));
    }

    private async void AddCustomGame_Click(object sender, RoutedEventArgs e)
    {
        if (_settings is null)
            return;
        var picker = new FileOpenPicker { SuggestedStartLocation = PickerLocationId.ComputerFolder };
        picker.FileTypeFilter.Add(".exe");
        InitializePicker(picker);
        var file = await picker.PickSingleFileAsync();
        if (file is null)
            return;

        var name = new TextBox { Text = Path.GetFileNameWithoutExtension(file.Name), Header = "Name to show" };
        var dialog = new ContentDialog
        {
            XamlRoot = XamlRoot,
            Title = "Add a game",
            Content = name,
            PrimaryButtonText = "Add",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
        };
        if (await dialog.ShowAsync() != ContentDialogResult.Primary || name.Text.Trim().Length == 0)
            return;
        _settings.CustomGames = _settings.CustomGames.Append(new CustomGame(name.Text.Trim(), file.Path)).ToList();
        ShowLists();
        await SaveAsync();
    }

    private async void AddFolder_Click(object sender, RoutedEventArgs e)
    {
        if (_settings is null)
            return;
        var picker = new FolderPicker { SuggestedStartLocation = PickerLocationId.ComputerFolder };
        picker.FileTypeFilter.Add("*");
        InitializePicker(picker);
        var folder = await picker.PickSingleFolderAsync();
        if (folder is null || _settings.ExtraGameFolders.Contains(folder.Path, StringComparer.OrdinalIgnoreCase))
            return;
        _settings.ExtraGameFolders = _settings.ExtraGameFolders.Append(folder.Path).ToList();
        ShowLists();
        await SaveAsync();
    }

    private async void AddIgnoredGame_Click(object sender, RoutedEventArgs e)
    {
        var name = NewIgnoredGame.Text.Trim();
        if (_settings is null || name.Length == 0 || _settings.IgnoredGames.Contains(name, StringComparer.OrdinalIgnoreCase))
            return;
        _settings.IgnoredGames = _settings.IgnoredGames.Append(name).ToList();
        NewIgnoredGame.Text = "";
        ShowLists();
        await SaveAsync();
    }

    private async void AddIgnoredProgram_Click(object sender, RoutedEventArgs e)
    {
        var name = NewIgnoredProgram.Text.Trim();
        if (_settings is null || name.Length == 0)
            return;
        if (!name.EndsWith(".exe", StringComparison.OrdinalIgnoreCase))
            name += ".exe";
        if (_settings.IgnoredExecutables.Contains(name, StringComparer.OrdinalIgnoreCase))
            return;
        _settings.IgnoredExecutables = _settings.IgnoredExecutables.Append(name).ToList();
        NewIgnoredProgram.Text = "";
        ShowLists();
        await SaveAsync();
    }

    private async void Rescan_Click(object sender, RoutedEventArgs e)
    {
        if (await App.State.RunAsync(c => c.RescanGamesAsync()))
            await Dialogs.InformAsync(XamlRoot, "Games rescanned", "Newly installed games will be recognised from now on.");
    }

    private async void Export_Click(object sender, RoutedEventArgs e)
    {
        string csv;
        try
        {
            csv = await App.State.Client.ExportCsvAsync();
        }
        catch (Exception ex) when (ex is TrackerUnavailableException or TrackerErrorException)
        {
            App.State.ReportError(ex.Message);
            return;
        }
        var picker = new FileSavePicker { SuggestedFileName = $"Playtime sessions {DateTime.Now:yyyy-MM-dd}" };
        picker.FileTypeChoices.Add("CSV spreadsheet", new List<string> { ".csv" });
        InitializePicker(picker);
        var file = await picker.PickSaveFileAsync();
        if (file is null)
            return;
        // The tracker's CSV already starts with a byte-order mark for Excel.
        await File.WriteAllTextAsync(file.Path, csv, new System.Text.UTF8Encoding(encoderShouldEmitUTF8Identifier: false));
    }

    private void OpenReport_Click(object sender, RoutedEventArgs e)
    {
        var report = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "Playtime Tracker", "Game Stats.txt");
        if (File.Exists(report))
            Process.Start(new ProcessStartInfo(report) { UseShellExecute = true })?.Dispose();
    }

    private void OpenFolder_Click(object sender, RoutedEventArgs e)
    {
        var folder = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "Playtime Tracker");
        if (Directory.Exists(folder))
            Process.Start(new ProcessStartInfo("explorer.exe") { ArgumentList = { folder }, UseShellExecute = false })?.Dispose();
    }
}
