//! Embeds the app icon, version information and application manifest into playtime-tracker.exe, so Windows (Task
//! Manager, Startup apps, the tray) shows "Playtime Tracker" with its icon, and the tray icon and menu are sharp on
//! high-DPI screens. Resources are compiled only when building on Windows for Windows; elsewhere this does nothing.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=tracker.manifest");
    println!("cargo:rerun-if-changed=../../src/GameSessionTracker/app.ico");
    let target_windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    if !cfg!(windows) || !target_windows {
        return;
    }

    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let rc_path = |p: PathBuf| p.display().to_string().replace('\\', "\\\\");
    let icon = rc_path(
        dir.join("..")
            .join("..")
            .join("src")
            .join("GameSessionTracker")
            .join("app.ico"),
    );
    let manifest = rc_path(dir.join("tracker.manifest"));

    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let mut parts = version
        .split(['.', '-', '+'])
        .map(|p| p.parse::<u16>().unwrap_or(0));
    let (major, minor, patch) = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );

    let rc = format!(
        r#"#include <winver.h>
1 ICON "{icon}"
1 24 "{manifest}"
VS_VERSION_INFO VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS VOS_NT_WINDOWS32
FILETYPE VFT_APP
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "Playtime Tracker"
      VALUE "FileDescription", "Playtime Tracker"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "playtime-tracker"
      VALUE "OriginalFilename", "playtime-tracker.exe"
      VALUE "ProductName", "Playtime Tracker"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default()).join("tracker.rc");
    if let Err(e) = std::fs::write(&out, rc) {
        panic!("couldn't write {}: {e}", out.display());
    }
    if let Err(e) = embed_resource::compile(&out, embed_resource::NONE).manifest_required() {
        panic!("couldn't compile the tracker's resources: {e}");
    }
}
