//! The real machine behind `playtime_core::discovery`: files, the registry and exe version resources.

use crate::reg::RegKey;
use playtime_core::discovery::{DiscoveryHost, Hive, KnownFolders, RegView};
use std::fs;
use std::io::Read;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Storage::FileSystem::{
    GetDriveTypeW, GetFileVersionInfoSizeW, GetFileVersionInfoW, GetLogicalDrives,
    GetLongPathNameW, VerQueryValueW,
};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;

/// Launcher metadata files are small; refuse anything bigger than this.
const MAX_TEXT_BYTES: u64 = 8 * 1024 * 1024;
/// Version resources are a few KB; cap what we'll load for one.
const MAX_VERSION_INFO_BYTES: u32 = 1024 * 1024;
/// `GetDriveTypeW` result for a fixed disk.
const DRIVE_FIXED: u32 = 3;

#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsHost;

impl DiscoveryHost for WindowsHost {
    fn known_folders(&self) -> KnownFolders {
        let env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        KnownFolders {
            program_files: env("ProgramW6432").or_else(|| env("ProgramFiles")),
            program_files_x86: env("ProgramFiles(x86)"),
            program_data: env("ProgramData"),
            fixed_drives: fixed_drives(),
        }
    }

    fn dir_exists(&self, path: &str) -> bool {
        fs::metadata(path).is_ok_and(|m| m.is_dir())
    }

    fn file_exists(&self, path: &str) -> bool {
        fs::metadata(path).is_ok_and(|m| m.is_file())
    }

    fn read_text(&self, path: &str) -> Option<String> {
        let file = fs::File::open(path).ok()?;
        if file.metadata().ok()?.len() > MAX_TEXT_BYTES {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(MAX_TEXT_BYTES).read_to_end(&mut bytes).ok()?;
        // Launchers write UTF-8, sometimes with a BOM; be lenient about stray bytes.
        let text = String::from_utf8_lossy(&bytes);
        Some(text.trim_start_matches('\u{feff}').to_string())
    }

    fn list_files(&self, dir: &str) -> Vec<String> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().into_string().ok())
            .collect()
    }

    fn reg_string(&self, hive: Hive, view: RegView, key: &str, value: &str) -> Option<String> {
        RegKey::open(hive, view, key)?.string(value)
    }

    fn reg_subkeys(&self, hive: Hive, view: RegView, key: &str) -> Vec<String> {
        RegKey::open(hive, view, key)
            .map(|k| k.subkeys())
            .unwrap_or_default()
    }

    fn exe_product_name(&self, exe: &str) -> Option<String> {
        let info = version_info(exe)?;
        ["ProductName", "FileDescription"]
            .iter()
            .filter_map(|field| version_string(&info, field))
            .map(|s| s.trim().to_string())
            .find(|s| !s.is_empty())
    }

    fn expand_env(&self, text: &str) -> String {
        let source = HSTRING::from(text);
        // SAFETY: size query with no buffer.
        let needed = unsafe { ExpandEnvironmentStringsW(PCWSTR(source.as_ptr()), None) };
        if needed == 0 || needed > 32 * 1024 {
            return text.to_string();
        }
        let mut buffer = vec![0u16; needed as usize];
        // SAFETY: `buffer` has room for `needed` characters including the terminator.
        let written =
            unsafe { ExpandEnvironmentStringsW(PCWSTR(source.as_ptr()), Some(&mut buffer)) };
        if written == 0 || written as usize > buffer.len() {
            return text.to_string();
        }
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        let expanded = String::from_utf16(&buffer[..end]).unwrap_or_else(|_| text.to_string());
        long_path(&expanded).unwrap_or(expanded)
    }
}

/// The long form of an existing path (`C:\Users\RUNNER~1\…` → `C:\Users\runneradmin\…`). Windows reports running
/// programs by their long path, so a folder saved in 8.3 short form would otherwise never match.
fn long_path(path: &str) -> Option<String> {
    let source = HSTRING::from(path);
    // SAFETY: size query with no buffer; fails (0) if the path doesn't exist.
    let needed = unsafe { GetLongPathNameW(PCWSTR(source.as_ptr()), None) };
    if needed == 0 || needed > 32 * 1024 {
        return None;
    }
    let mut buffer = vec![0u16; needed as usize];
    // SAFETY: `buffer` has room for `needed` characters including the terminator.
    let written = unsafe { GetLongPathNameW(PCWSTR(source.as_ptr()), Some(&mut buffer)) };
    if written == 0 || written as usize >= buffer.len() {
        return None;
    }
    String::from_utf16(&buffer[..written as usize]).ok()
}

fn fixed_drives() -> Vec<String> {
    // SAFETY: no arguments; returns a bit mask of drive letters.
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|bit| mask & (1 << bit) != 0)
        .map(|bit| format!("{}:\\", char::from(b'A' + bit)))
        .filter(|root| {
            let root = HSTRING::from(root.as_str());
            // SAFETY: `root` is a valid NUL-terminated drive root for the call.
            unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) == DRIVE_FIXED }
        })
        .collect()
}

fn version_info(exe: &str) -> Option<Vec<u8>> {
    let path = HSTRING::from(exe);
    // SAFETY: size query; the path outlives the call.
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None) };
    if size == 0 || size > MAX_VERSION_INFO_BYTES {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    // SAFETY: `data` has `size` bytes, as the API was told.
    unsafe { GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, data.as_mut_ptr().cast()) }
        .ok()?;
    Some(data)
}

/// Reads `\StringFileInfo\<lang><codepage>\<field>`, trying the exe's own translations first, then US English.
fn version_string(info: &[u8], field: &str) -> Option<String> {
    let mut translations: Vec<(u16, u16)> = Vec::new();
    if let Some(pairs) = query(info, r"\VarFileInfo\Translation", 1) {
        translations.extend(pairs.chunks_exact(2).map(|p| (p[0], p[1])));
    }
    translations.extend([(0x0409, 0x04b0), (0x0409, 0x04e4), (0x0000, 0x04b0)]);
    for (language, codepage) in translations {
        let sub_block = format!(r"\StringFileInfo\{language:04x}{codepage:04x}\{field}");
        // For string values the reported length is in characters, not bytes.
        let Some(chars) = query(info, &sub_block, 2) else {
            continue;
        };
        let end = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
        if let Ok(value) = String::from_utf16(&chars[..end]) {
            return Some(value);
        }
    }
    None
}

/// Looks up a value in a version resource and returns it as UTF-16 units. `unit_bytes` is how many bytes one unit
/// of the reported length is (1 for binary values, 2 for strings). The value is copied out of `info` only after
/// checking it lies entirely inside it.
fn query(info: &[u8], sub_block: &str, unit_bytes: usize) -> Option<Vec<u16>> {
    let sub_block = HSTRING::from(sub_block);
    let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
    let mut len: u32 = 0;
    // SAFETY: `info` is a complete version resource from GetFileVersionInfoW; outputs point inside it.
    let found = unsafe {
        VerQueryValueW(
            info.as_ptr().cast(),
            PCWSTR(sub_block.as_ptr()),
            &mut ptr,
            &mut len,
        )
    };
    if !found.as_bool() || ptr.is_null() || len == 0 {
        return None;
    }
    let start = (ptr as usize).checked_sub(info.as_ptr() as usize)?;
    let bytes = (len as usize).checked_mul(unit_bytes)? & !1;
    let value = info.get(start..start.checked_add(bytes)?)?;
    Some(
        value
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect(),
    )
}
