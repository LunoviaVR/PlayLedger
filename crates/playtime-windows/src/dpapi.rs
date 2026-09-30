//! Windows DPAPI (`CryptProtectData` / `CryptUnprotectData`) for the current user.

use playtime_core::protected::{DataProtector, ProtectError};
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

/// DPAPI with `CurrentUser` scope, as earlier versions used it, so their protected files still open.
#[derive(Debug, Default, Clone, Copy)]
pub struct Dpapi;

fn blob(data: &[u8]) -> Result<CRYPT_INTEGER_BLOB, ProtectError> {
    let len = u32::try_from(data.len()).map_err(|_| ProtectError("data too large".into()))?;
    // DPAPI only reads the input buffers; the cast to *mut is what the API signature requires.
    Ok(CRYPT_INTEGER_BLOB {
        cbData: len,
        pbData: data.as_ptr().cast_mut(),
    })
}

/// Copies an output blob and frees it with `LocalFree`, as DPAPI requires.
fn take(output: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if output.pbData.is_null() {
        return Vec::new();
    }
    // SAFETY: on success DPAPI returns a buffer of exactly cbData bytes that we own and must LocalFree.
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    // SAFETY: pbData was allocated by DPAPI with LocalAlloc and is freed exactly once here.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
    }
    bytes
}

impl DataProtector for Dpapi {
    fn protect(&self, plain: &[u8], entropy: &[u8]) -> Result<Vec<u8>, ProtectError> {
        let input = blob(plain)?;
        let entropy = blob(entropy)?;
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: input/entropy point at live slices for the duration of the call; output is written by DPAPI.
        unsafe {
            CryptProtectData(
                &input,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .map_err(|e| ProtectError(format!("CryptProtectData failed: {e}")))?;
        Ok(take(output))
    }

    fn unprotect(&self, protected: &[u8], entropy: &[u8]) -> Result<Vec<u8>, ProtectError> {
        let input = blob(protected)?;
        let entropy = blob(entropy)?;
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as above.
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .map_err(|e| ProtectError(format!("verification failed: {e}")))?;
        Ok(take(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use playtime_core::protected::{decode, encode, Purpose};

    #[test]
    fn round_trip_and_tamper_detection() {
        let bytes = encode(&Dpapi, Purpose::Sessions, 5, r#"{"version":1}"#).expect("protects");
        assert_eq!(
            decode(&Dpapi, Purpose::Sessions, &bytes, "t").expect("verifies"),
            (r#"{"version":1}"#.into(), 5)
        );
        assert!(
            decode(&Dpapi, Purpose::Settings, &bytes, "t").is_err(),
            "wrong purpose"
        );
        let mut tampered = bytes;
        let middle = tampered.len() / 2;
        tampered[middle] ^= 0x01;
        assert!(decode(&Dpapi, Purpose::Sessions, &tampered, "t").is_err());
    }
}
