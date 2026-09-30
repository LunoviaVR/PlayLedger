//! Compiles the Slint UI (Fluent style), and on Windows embeds the app icon and version information into the exe,
//! so Windows (Task Manager, the taskbar, Apps) shows "PlayLedger" with its icon.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    slint_build::compile_with_config(
        "ui/app.slint",
        slint_build::CompilerConfiguration::new().with_style("fluent".into()),
    )?;
    println!("cargo:rerun-if-changed=../../assets/app.ico");
    let target_windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    if !cfg!(windows) || !target_windows {
        return Ok(());
    }
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let icon = dir
        .join("..")
        .join("..")
        .join("assets")
        .join("app.ico")
        .display()
        .to_string()
        .replace('\\', "\\\\");
    let version = std::env::var("CARGO_PKG_VERSION")?;
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
      VALUE "CompanyName", "PlayLedger"
      VALUE "FileDescription", "PlayLedger"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "PlaytimeTracker.Dashboard"
      VALUE "OriginalFilename", "PlaytimeTracker.Dashboard.exe"
      VALUE "ProductName", "PlayLedger"
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
    let out = PathBuf::from(std::env::var("OUT_DIR")?).join("dashboard.rc");
    std::fs::write(&out, rc)?;
    embed_resource::compile(&out, embed_resource::NONE).manifest_optional()?;
    Ok(())
}
