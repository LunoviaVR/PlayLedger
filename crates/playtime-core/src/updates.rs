//! Parsing GitHub's "latest release" response, with the same safety rules as the C# updater:
//! only published, non-prerelease releases with a plain `vX.Y.Z` tag newer than the running version count; the
//! installer must come from this repository's release download URL over HTTPS, and GitHub's recorded size and
//! SHA-256 digest are required before anything can be installed. Downloading and running the installer is the
//! platform layer's job.

use serde::Deserialize;

pub const OWNER: &str = "LunoviaVR";
pub const REPOSITORY: &str = "PlaytimeTracker";
pub const INSTALLER_ASSET: &str = "PlaytimeTrackerSetup.exe";

/// A version as released: major.minor.patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// `v2.1.0` / `2.1.0` / `2.1` → a version. Anything else (suffixes like `-beta`, `latest`, 4 parts) is rejected.
    pub fn parse_tag(tag: &str) -> Option<Self> {
        let text = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
        let parts: Vec<&str> = text.split('.').collect();
        if !(2..=3).contains(&parts.len())
            || parts
                .iter()
                .any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()))
        {
            return None;
        }
        let number = |i: usize| parts.get(i).map_or(Some(0), |p| p.parse::<u32>().ok());
        Some(Self::new(number(0)?, number(1)?, number(2)?))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A newer release that was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: Version,
    pub tag: String,
    pub installer: Option<InstallerAsset>,
}

impl UpdateInfo {
    /// True if the release has everything needed to install it safely from inside the app.
    pub fn can_install(&self) -> bool {
        self.installer.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallerAsset {
    pub url: String,
    pub size: u64,
    /// Lower-case hex SHA-256 as recorded by GitHub.
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseDto {
    tag_name: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<AssetDto>,
}

#[derive(Debug, Deserialize)]
struct AssetDto {
    name: Option<String>,
    #[serde(default)]
    size: u64,
    digest: Option<String>,
    browser_download_url: Option<String>,
}

pub fn releases_page() -> String {
    format!("https://github.com/{OWNER}/{REPOSITORY}/releases/latest")
}

pub fn latest_release_api() -> String {
    format!("https://api.github.com/repos/{OWNER}/{REPOSITORY}/releases/latest")
}

/// Parses the API response. `Ok(None)` if there's nothing newer than `current`.
pub fn parse_latest_release(
    json: &str,
    current: Version,
) -> Result<Option<UpdateInfo>, serde_json::Error> {
    let release: ReleaseDto = serde_json::from_str(json)?;
    let Some(tag) = release
        .tag_name
        .filter(|_| !release.draft && !release.prerelease)
    else {
        return Ok(None);
    };
    let Some(version) = Version::parse_tag(&tag).filter(|v| *v > current) else {
        return Ok(None);
    };
    let prefix = format!("https://github.com/{OWNER}/{REPOSITORY}/releases/download/");
    let installer = release
        .assets
        .into_iter()
        .find(|a| a.name.as_deref() == Some(INSTALLER_ASSET))
        .and_then(|a| {
            let url = a.browser_download_url.filter(|u| u.starts_with(&prefix))?;
            let digest = a.digest?;
            let hex = digest.strip_prefix("sha256:")?;
            let valid = hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit());
            (valid && a.size > 0).then(|| InstallerAsset {
                url,
                size: a.size,
                sha256: hex.to_ascii_lowercase(),
            })
        });
    Ok(Some(UpdateInfo {
        version,
        tag,
        installer,
    }))
}

/// After redirects the installer must still come from GitHub over HTTPS.
pub fn is_trusted_download_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let host = host.rsplit_once('@').map_or(host.as_str(), |(_, h)| h); // no user-info tricks
    let host = host.split(':').next().unwrap_or("");
    host == "github.com" || host.ends_with(".githubusercontent.com")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURRENT: Version = Version::new(2, 1, 0);

    fn release(tag: &str, extra: &str) -> String {
        let hash = "a".repeat(64);
        format!(
            r#"{{"tag_name":"{tag}","draft":false,"prerelease":false{extra},"assets":[{{"name":"PlaytimeTrackerSetup.exe","size":123,
            "digest":"sha256:{hash}","browser_download_url":"https://github.com/LunoviaVR/PlaytimeTracker/releases/download/{tag}/PlaytimeTrackerSetup.exe"}}]}}"#
        )
    }

    #[test]
    fn tags() {
        assert_eq!(Version::parse_tag("v2.1.0"), Some(Version::new(2, 1, 0)));
        assert_eq!(Version::parse_tag("2.1"), Some(Version::new(2, 1, 0)));
        for bad in [
            "latest",
            "v2.1.0-beta",
            "v2",
            "v2.1.0.1",
            "v2..1",
            "v 2.1.0",
            "",
        ] {
            assert_eq!(Version::parse_tag(bad), None, "{bad}");
        }
    }

    #[test]
    fn newer_release_is_installable() {
        let update = parse_latest_release(&release("v2.2.0", ""), CURRENT)
            .expect("ok")
            .expect("newer");
        assert!(update.can_install());
        assert_eq!(update.version, Version::new(2, 2, 0));
    }

    #[test]
    fn same_older_draft_and_prerelease_are_ignored() {
        assert_eq!(
            parse_latest_release(&release("v2.1.0", ""), CURRENT).expect("ok"),
            None
        );
        assert_eq!(
            parse_latest_release(&release("v1.0.1", ""), CURRENT).expect("ok"),
            None
        );
        assert_eq!(
            parse_latest_release(&release("latest", ""), CURRENT).expect("ok"),
            None
        );
        let draft = release("v3.0.0", "").replace(r#""draft":false"#, r#""draft":true"#);
        assert_eq!(parse_latest_release(&draft, CURRENT).expect("ok"), None);
        let pre = release("v3.0.0", "").replace(r#""prerelease":false"#, r#""prerelease":true"#);
        assert_eq!(parse_latest_release(&pre, CURRENT).expect("ok"), None);
    }

    #[test]
    fn unsafe_assets_are_not_installable() {
        let no_digest = release("v3.0.0", "").replace("sha256:", "md5:");
        assert!(!parse_latest_release(&no_digest, CURRENT)
            .expect("ok")
            .expect("newer")
            .can_install());
        let foreign = release("v3.0.0", "").replace(
            "LunoviaVR/PlaytimeTracker/releases",
            "evil/PlaytimeTracker/releases",
        );
        assert!(!parse_latest_release(&foreign, CURRENT)
            .expect("ok")
            .expect("newer")
            .can_install());
        assert!(parse_latest_release("not json", CURRENT).is_err());
    }

    #[test]
    fn trusted_hosts() {
        assert!(is_trusted_download_url(
            "https://release-assets.githubusercontent.com/x?sig=1"
        ));
        assert!(is_trusted_download_url("https://github.com/a"));
        assert!(!is_trusted_download_url("http://github.com/a"));
        assert!(!is_trusted_download_url(
            "https://githubusercontent.com.evil.io/x"
        ));
        assert!(!is_trusted_download_url(
            "https://evilgithubusercontent.com/x"
        ));
        assert!(
            !is_trusted_download_url("https://github.com@evil.io/x"),
            "user-info can't fake the host"
        );
        assert!(!is_trusted_download_url("https://evil.io/github.com"));
    }
}
