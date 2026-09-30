//! The last saved generation of each protected file, DPAPI-protected under
//! `HKCU\Software\Playtime Tracker\Integrity` (value name = purpose). Same format as earlier versions:
//! the value is DPAPI(ASCII decimal generation) with entropy `PlaytimeTracker/<purpose>/generation/v1`.

use crate::dpapi::Dpapi;
use playtime_core::protected::{DataProtector, GenerationStore, Purpose};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    KEY_WRITE, REG_BINARY, REG_OPTION_NON_VOLATILE, RRF_RT_REG_BINARY,
};

const KEY_PATH: &str = r"Software\Playtime Tracker\Integrity";

#[derive(Debug, Default, Clone, Copy)]
pub struct RegistryGenerations;

impl GenerationStore for RegistryGenerations {
    fn read(&self, purpose: Purpose) -> Option<u64> {
        let key = HSTRING::from(KEY_PATH);
        let name = HSTRING::from(purpose.as_str());
        let mut size: u32 = 0;
        // SAFETY: first call only asks for the size; pointers are valid for the call.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_BINARY,
                None,
                None,
                Some(&mut size),
            )
        };
        if status != ERROR_SUCCESS || size == 0 || size > 4096 {
            return None;
        }
        let mut buffer = vec![0u8; size as usize];
        // SAFETY: buffer has `size` bytes; the API writes at most that many and updates `size`.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
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
        let plain = Dpapi
            .unprotect(&buffer, &purpose.generation_entropy())
            .ok()?;
        std::str::from_utf8(&plain).ok()?.parse().ok()
    }

    fn write(&self, purpose: Purpose, generation: u64) {
        let Ok(blob) = Dpapi.protect(
            generation.to_string().as_bytes(),
            &purpose.generation_entropy(),
        ) else {
            return;
        };
        let key_path = HSTRING::from(KEY_PATH);
        let name = HSTRING::from(purpose.as_str());
        let mut key = HKEY::default();
        // SAFETY: creates/opens our own key under HKCU; `key` receives the handle, closed below.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key_path.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_READ | KEY_WRITE,
                None,
                &mut key,
                None,
            )
        };
        if status != ERROR_SUCCESS {
            return;
        }
        // SAFETY: `blob` outlives the call; the key handle is valid until closed.
        unsafe {
            let _ = RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_BINARY, Some(&blob));
            let _ = RegCloseKey(key);
        }
    }
}
