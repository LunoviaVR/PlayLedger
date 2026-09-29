//! A small read-only registry wrapper: open a key (optionally in the 32-bit view), read strings, list sub-keys.
//! Handles are closed on drop; every failure is just "not there".

use playtime_core::discovery::{Hive, RegView};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY,
    KEY_WRITE, REG_BINARY, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_BINARY,
    RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
};

/// Longest string value we accept (paths and names); anything bigger isn't launcher metadata.
const MAX_VALUE_BYTES: u32 = 64 * 1024;
/// Registry key names are limited to 255 characters.
const MAX_KEY_NAME: usize = 256;
/// Upper bound on sub-keys read from one key, so a hostile or corrupt hive can't stall discovery.
const MAX_SUBKEYS: u32 = 10_000;

pub struct RegKey(HKEY);

impl RegKey {
    pub fn open(hive: Hive, view: RegView, path: &str) -> Option<Self> {
        let root = match hive {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        };
        let access = match view {
            RegView::Default => KEY_READ,
            RegView::Wow32 => KEY_READ | KEY_WOW64_32KEY,
        };
        let path = HSTRING::from(path);
        let mut key = HKEY::default();
        // SAFETY: `path` outlives the call; on success `key` is a handle we own and close in Drop.
        let status = unsafe { RegOpenKeyExW(root, PCWSTR(path.as_ptr()), None, access, &mut key) };
        (status == ERROR_SUCCESS).then_some(Self(key))
    }

    /// Opens (creating if needed) a key under HKCU for reading and writing.
    pub fn create_current_user(path: &str) -> Option<Self> {
        let path = HSTRING::from(path);
        let mut key = HKEY::default();
        // SAFETY: `path` outlives the call; on success `key` is a handle we own and close in Drop.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_READ | KEY_WRITE,
                None,
                &mut key,
                None,
            )
        };
        (status == ERROR_SUCCESS).then_some(Self(key))
    }

    /// Opens an existing HKCU key for reading and writing.
    pub fn open_current_user_writable(path: &str) -> Option<Self> {
        let path = HSTRING::from(path);
        let mut key = HKEY::default();
        // SAFETY: as in `open`.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                None,
                KEY_READ | KEY_WRITE,
                &mut key,
            )
        };
        (status == ERROR_SUCCESS).then_some(Self(key))
    }

    pub fn set_string(&self, value: &str, data: &str) -> bool {
        let name = HSTRING::from(value);
        let bytes: Vec<u8> = data
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        // SAFETY: `bytes` is a NUL-terminated UTF-16 string that outlives the call.
        let status =
            unsafe { RegSetValueExW(self.0, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&bytes)) };
        status == ERROR_SUCCESS
    }

    pub fn set_binary(&self, value: &str, data: &[u8]) -> bool {
        let name = HSTRING::from(value);
        // SAFETY: `data` outlives the call.
        let status =
            unsafe { RegSetValueExW(self.0, PCWSTR(name.as_ptr()), None, REG_BINARY, Some(data)) };
        status == ERROR_SUCCESS
    }

    /// A REG_BINARY value (up to 1 KB), or `None`.
    pub fn binary(&self, value: &str) -> Option<Vec<u8>> {
        let name = HSTRING::from(value);
        let mut buffer = vec![0u8; 1024];
        let mut size = buffer.len() as u32;
        // SAFETY: `buffer` has `size` bytes; the API writes at most that and updates `size`.
        let status = unsafe {
            RegGetValueW(
                self.0,
                PCWSTR::null(),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_BINARY,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        buffer.truncate(size as usize);
        Some(buffer)
    }

    /// Deletes a value; a missing value is fine.
    pub fn delete_value(&self, value: &str) {
        let name = HSTRING::from(value);
        // SAFETY: plain call with a valid key and name.
        unsafe {
            let _ = RegDeleteValueW(self.0, PCWSTR(name.as_ptr()));
        }
    }

    /// A REG_SZ / REG_EXPAND_SZ value (expanded), or `None`.
    pub fn string(&self, value: &str) -> Option<String> {
        let name = HSTRING::from(value);
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
        let mut size: u32 = 0;
        // SAFETY: size query only; pointers are valid for the call.
        let status = unsafe {
            RegGetValueW(
                self.0,
                PCWSTR::null(),
                PCWSTR(name.as_ptr()),
                flags,
                None,
                None,
                Some(&mut size),
            )
        };
        if status != ERROR_SUCCESS || size == 0 || size > MAX_VALUE_BYTES {
            return None;
        }
        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        let mut bytes = (buffer.len() * 2) as u32;
        // SAFETY: `buffer` holds `bytes` bytes; the API writes at most that and updates `bytes`.
        let status = unsafe {
            RegGetValueW(
                self.0,
                PCWSTR::null(),
                PCWSTR(name.as_ptr()),
                flags,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        buffer.truncate(bytes as usize / 2);
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        String::from_utf16(&buffer[..end]).ok()
    }

    pub fn subkeys(&self) -> Vec<String> {
        let mut names = Vec::new();
        for index in 0..MAX_SUBKEYS {
            let mut buffer = [0u16; MAX_KEY_NAME];
            let mut len = buffer.len() as u32;
            // SAFETY: `buffer` has `len` characters; the API writes the name and its length into them.
            let status = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    Some(PWSTR(buffer.as_mut_ptr())),
                    &mut len,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if status == ERROR_MORE_DATA {
                continue; // not a valid key name length; skip it
            }
            if status != ERROR_SUCCESS {
                break; // ERROR_NO_MORE_ITEMS or an error: either way, done
            }
            if let Ok(name) = String::from_utf16(&buffer[..len as usize]) {
                names.push(name);
            }
        }
        names
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: we own the handle and close it exactly once.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}
