using System.Diagnostics;
using System.Net.Http.Headers;
using System.Reflection;
using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;
using Microsoft.Win32;

namespace GameSessionTracker;

/// <summary>A newer release found on GitHub.</summary>
internal sealed record UpdateInfo(Version Version, string Tag, Uri? InstallerUrl, long InstallerSize, string? Sha256)
{
    /// <summary>True if it can be installed from inside the app (the release has the installer and GitHub's checksum).</summary>
    public bool CanInstall => InstallerUrl is not null && Sha256 is not null && InstallerSize > 0;
}

/// <summary>
/// Checks GitHub releases for a newer version and installs it.
/// Safety: only the latest published (non-draft, non-prerelease) release of this repository is considered; the installer
/// is downloaded over HTTPS from GitHub's own hosts only, must match the size and SHA-256 that GitHub records for the
/// release asset, and is run only when this copy was installed by the installer (so it updates in place).
/// </summary>
internal sealed class Updater : IDisposable
{
    public const string Owner = "LunoviaVR";
    public const string Repository = "PlaytimeTracker";
    // Releases carry one installer, "Setup.exe" (the small online installer; it fetches .NET 8 if needed).
    private const string InstallerAssetName = "Setup.exe";
    private const string InstallKey = @"Software\Microsoft\Windows\CurrentVersion\Uninstall\PlaytimeTracker";
    private const long MaxInstallerBytes = 300L * 1024 * 1024;
    private const int MaxApiResponseBytes = 2 * 1024 * 1024;

    private static readonly Uri LatestReleaseApi = new($"https://api.github.com/repos/{Owner}/{Repository}/releases/latest");
    private static readonly string DownloadPrefix = $"https://github.com/{Owner}/{Repository}/releases/download/";

    /// <summary>Page shown when an update can't be installed from inside the app. Built from constants, never from the API.</summary>
    public static readonly Uri ReleasesPage = new($"https://github.com/{Owner}/{Repository}/releases/latest");

    private readonly HttpClient _http;

    public Updater()
    {
        // Default handler: certificate validation on, and .NET never follows a redirect from HTTPS to HTTP.
        _http = new HttpClient(new HttpClientHandler { AllowAutoRedirect = true, MaxAutomaticRedirections = 5 })
        {
            Timeout = TimeSpan.FromMinutes(10),
        };
        _http.DefaultRequestHeaders.UserAgent.Add(new ProductInfoHeaderValue(Repository, CurrentVersion.ToString(3)));
    }

    public static Version CurrentVersion
    {
        get
        {
            var v = Assembly.GetExecutingAssembly().GetName().Version ?? new Version(0, 0, 0);
            return new Version(v.Major, v.Minor, Math.Max(0, v.Build));
        }
    }

    /// <summary>The newer release found by the last check, if any.</summary>
    public UpdateInfo? Available { get; private set; }

    public DateTimeOffset? LastChecked { get; private set; }
    public string? LastError { get; private set; }
    public bool Busy { get; private set; }

    /// <summary>Download progress 0–1 while installing, otherwise null.</summary>
    public double? Progress { get; private set; }

    /// <summary>Raised on the UI thread whenever the state above changes.</summary>
    public event Action? StateChanged;

    /// <summary>
    /// True if this copy was installed by the installer (the Apps entry points at this exe's folder), so running a newer
    /// installer updates it in place. A copy run from elsewhere (e.g. Downloads) is never replaced behind your back.
    /// </summary>
    public static bool IsInstalledCopy
    {
        get
        {
            try
            {
                using var key = Registry.CurrentUser.OpenSubKey(InstallKey);
                var location = key?.GetValue("InstallLocation") as string;
                var exeDir = Path.GetDirectoryName(Environment.ProcessPath);
                return location is not null && exeDir is not null &&
                       string.Equals(Path.GetFullPath(location).TrimEnd('\\'), Path.GetFullPath(exeDir).TrimEnd('\\'), StringComparison.OrdinalIgnoreCase);
            }
            catch
            {
                return false;
            }
        }
    }

    /// <summary>Asks GitHub for the latest release. Never throws; failures end up in <see cref="LastError"/>.</summary>
    public async Task<UpdateInfo?> CheckAsync(CancellationToken cancel = default)
    {
        if (Busy)
            return Available;
        SetBusy(true);
        try
        {
            using var request = new HttpRequestMessage(HttpMethod.Get, LatestReleaseApi);
            request.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("application/vnd.github+json"));
            request.Headers.Add("X-GitHub-Api-Version", "2022-11-28");
            using var response = await _http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancel);
            if (response.StatusCode == System.Net.HttpStatusCode.NotFound)
            {
                Available = null; // no published release yet
            }
            else
            {
                response.EnsureSuccessStatusCode();
                var body = await ReadLimitedAsync(response, MaxApiResponseBytes, cancel);
                Available = Parse(JsonSerializer.Deserialize<ReleaseDto>(body));
            }
            LastError = null;
        }
        catch (Exception ex) when (ex is HttpRequestException or TaskCanceledException or JsonException or InvalidDataException or IOException)
        {
            // Offline, rate-limited, etc. Try again at the next check.
            LastError = "Couldn't reach GitHub to check for updates.";
            ErrorLog.Write("Update check failed", ex);
        }
        finally
        {
            LastChecked = DateTimeOffset.Now;
            SetBusy(false);
        }
        return Available;
    }

    /// <summary>
    /// Downloads and verifies the installer, then starts it silently; it closes this app, updates it in place and starts
    /// the new version. Returns false (with <see cref="LastError"/> set) if anything doesn't check out.
    /// </summary>
    public async Task<bool> DownloadAndStartInstallerAsync(UpdateInfo update, CancellationToken cancel = default)
    {
        if (Busy || !update.CanInstall || !IsInstalledCopy)
            return false;
        SetBusy(true);
        var folder = Path.Combine(Path.GetTempPath(), "PlaytimeTracker-update-" + Guid.NewGuid().ToString("N"));
        try
        {
            Directory.CreateDirectory(folder);
            var installer = Path.Combine(folder, InstallerAssetName);
            await DownloadAsync(update, installer, cancel);

            // /S = silent, /relaunch = start the updated app when done (see installer/PlaytimeTracker.nsi).
            Process.Start(new ProcessStartInfo(installer, "/S /relaunch") { UseShellExecute = false, WorkingDirectory = folder });
            LastError = null;
            return true;
        }
        catch (Exception ex) when (ex is HttpRequestException or TaskCanceledException or IOException or InvalidDataException or
                                       UnauthorizedAccessException or System.ComponentModel.Win32Exception)
        {
            LastError = ex is InvalidDataException ? ex.Message : "The update couldn't be downloaded. Try again later.";
            ErrorLog.Write("Update install failed", ex);
            TryDelete(folder);
            return false;
        }
        finally
        {
            Progress = null;
            SetBusy(false);
        }
    }

    private async Task DownloadAsync(UpdateInfo update, string destination, CancellationToken cancel)
    {
        using var response = await _http.GetAsync(update.InstallerUrl!, HttpCompletionOption.ResponseHeadersRead, cancel);
        response.EnsureSuccessStatusCode();
        // After GitHub's redirect the file must still come from GitHub over HTTPS.
        var finalUri = response.RequestMessage?.RequestUri;
        if (finalUri is null || !IsTrustedDownloadHost(finalUri))
            throw new InvalidDataException("The update was served from an unexpected address, so it wasn't installed.");

        await using (var output = new FileStream(destination, FileMode.CreateNew, FileAccess.Write, FileShare.None))
        await using (var input = await response.Content.ReadAsStreamAsync(cancel))
        {
            using var sha = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
            var buffer = new byte[81920];
            long total = 0;
            int read;
            while ((read = await input.ReadAsync(buffer, cancel)) > 0)
            {
                total += read;
                if (total > update.InstallerSize || total > MaxInstallerBytes)
                    throw new InvalidDataException("The update download was larger than expected, so it wasn't installed.");
                sha.AppendData(buffer, 0, read);
                await output.WriteAsync(buffer.AsMemory(0, read), cancel);
                SetProgress(total / (double)update.InstallerSize);
            }
            if (total != update.InstallerSize)
                throw new InvalidDataException("The update download was incomplete, so it wasn't installed.");
            var actual = Convert.ToHexString(sha.GetHashAndReset());
            if (!string.Equals(actual, update.Sha256, StringComparison.OrdinalIgnoreCase))
                throw new InvalidDataException("The update didn't match GitHub's checksum, so it wasn't installed.");
        }
    }

    // ---------- Parsing ----------

    internal static UpdateInfo? Parse(ReleaseDto? release)
    {
        if (release is null || release.Draft || release.Prerelease || release.TagName is null)
            return null;
        if (!TryParseTag(release.TagName, out var version) || version <= CurrentVersion)
            return null;

        var asset = release.Assets?.FirstOrDefault(a => string.Equals(a.Name, InstallerAssetName, StringComparison.Ordinal));
        Uri? url = null;
        string? sha256 = null;
        long size = 0;
        if (asset is not null &&
            Uri.TryCreate(asset.BrowserDownloadUrl, UriKind.Absolute, out var candidate) &&
            candidate.Scheme == Uri.UriSchemeHttps &&
            candidate.AbsoluteUri.StartsWith(DownloadPrefix, StringComparison.Ordinal))
        {
            url = candidate;
            size = asset.Size;
            // GitHub computes this itself when the file is uploaded: "sha256:<64 hex chars>".
            if (asset.Digest is { } digest && digest.StartsWith("sha256:", StringComparison.OrdinalIgnoreCase) &&
                digest.Length == 7 + 64 && digest[7..].All(Uri.IsHexDigit))
                sha256 = digest[7..];
        }
        return new UpdateInfo(version, release.TagName, url, size, sha256);
    }

    /// <summary>"v2.1.0" or "2.1.0" → 2.1.0. Anything else (including pre-release suffixes) is rejected.</summary>
    internal static bool TryParseTag(string tag, out Version version)
    {
        var text = tag.StartsWith('v') || tag.StartsWith('V') ? tag[1..] : tag;
        if (Version.TryParse(text, out var parsed) && parsed.Build >= 0 && parsed.Revision < 0 && text.All(c => char.IsDigit(c) || c == '.'))
        {
            version = parsed;
            return true;
        }
        version = new Version(0, 0, 0);
        return false;
    }

    internal static bool IsTrustedDownloadHost(Uri uri) =>
        uri.Scheme == Uri.UriSchemeHttps &&
        (uri.Host.Equals("github.com", StringComparison.OrdinalIgnoreCase) ||
         uri.Host.EndsWith(".githubusercontent.com", StringComparison.OrdinalIgnoreCase));

    private static async Task<byte[]> ReadLimitedAsync(HttpResponseMessage response, int maxBytes, CancellationToken cancel)
    {
        if (response.Content.Headers.ContentLength > maxBytes)
            throw new InvalidDataException("The release information was larger than expected.");
        await using var stream = await response.Content.ReadAsStreamAsync(cancel);
        using var memory = new MemoryStream();
        var buffer = new byte[16384];
        int read;
        while ((read = await stream.ReadAsync(buffer, cancel)) > 0)
        {
            if (memory.Length + read > maxBytes)
                throw new InvalidDataException("The release information was larger than expected.");
            memory.Write(buffer, 0, read);
        }
        return memory.ToArray();
    }

    private void SetBusy(bool busy)
    {
        Busy = busy;
        StateChanged?.Invoke();
    }

    private void SetProgress(double value)
    {
        var rounded = Math.Round(Math.Clamp(value, 0, 1), 2);
        if (Progress == rounded)
            return;
        Progress = rounded;
        StateChanged?.Invoke();
    }

    private static void TryDelete(string folder)
    {
        try
        {
            Directory.Delete(folder, recursive: true);
        }
        catch
        {
            // Temp files; Windows cleans them up eventually.
        }
    }

    public void Dispose() => _http.Dispose();

    // ---------- GitHub API shapes (only the fields we use) ----------

    internal sealed class ReleaseDto
    {
        [JsonPropertyName("tag_name")] public string? TagName { get; set; }
        [JsonPropertyName("draft")] public bool Draft { get; set; }
        [JsonPropertyName("prerelease")] public bool Prerelease { get; set; }
        [JsonPropertyName("assets")] public List<AssetDto>? Assets { get; set; }
    }

    internal sealed class AssetDto
    {
        [JsonPropertyName("name")] public string? Name { get; set; }
        [JsonPropertyName("size")] public long Size { get; set; }
        [JsonPropertyName("digest")] public string? Digest { get; set; }
        [JsonPropertyName("browser_download_url")] public string? BrowserDownloadUrl { get; set; }
    }
}
