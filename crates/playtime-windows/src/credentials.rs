//! API keys the user enters in Settings (e.g. SteamGridDB), kept in Windows Credential Manager for the current
//! user on this PC. They're never written to the data folder, settings file, logs or the repository.

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
    CRED_TYPE_GENERIC,
};

/// Credential Manager entry for the SteamGridDB API key.
pub const STEAMGRIDDB_TARGET: &str = "Playtime Tracker/SteamGridDB API key";

/// Keys are short; refuse anything that isn't.
const MAX_SECRET_BYTES: usize = 1024;

/// A secret read from Credential Manager. Its `Debug` output never shows the value.
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

pub fn read(target: &str) -> Option<Secret> {
    let target = HSTRING::from(target);
    let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: on success `credential` points to a block owned by the system, freed with CredFree below.
    unsafe {
        CredReadW(
            PCWSTR(target.as_ptr()),
            CRED_TYPE_GENERIC,
            None,
            &mut credential,
        )
    }
    .ok()?;
    if credential.is_null() {
        return None;
    }
    // SAFETY: `credential` is valid until CredFree; the blob is `CredentialBlobSize` bytes.
    let value = unsafe {
        let c = &*credential;
        let size = c.CredentialBlobSize as usize;
        let value = if c.CredentialBlob.is_null() || size == 0 || size > MAX_SECRET_BYTES {
            None
        } else {
            String::from_utf8(std::slice::from_raw_parts(c.CredentialBlob, size).to_vec()).ok()
        };
        CredFree(credential.cast());
        value
    };
    value.map(Secret)
}

pub fn write(target: &str, secret: &str) -> windows::core::Result<()> {
    let secret = secret.trim();
    if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
        return Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::E_INVALIDARG,
        ));
    }
    let mut target_w: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
    let mut user: Vec<u16> = "Playtime Tracker".encode_utf16().chain(Some(0)).collect();
    let mut blob = secret.as_bytes().to_vec();
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target_w.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user.as_mut_ptr()),
        ..Default::default()
    };
    // SAFETY: every pointer in `credential` refers to a buffer that outlives the call.
    let result = unsafe { CredWriteW(&credential, 0) };
    blob.fill(0);
    result
}

pub fn delete(target: &str) {
    let target = HSTRING::from(target);
    // SAFETY: plain call with a valid string; a missing entry is fine.
    unsafe {
        let _ = CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None);
    }
}
