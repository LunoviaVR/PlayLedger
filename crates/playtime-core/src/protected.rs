//! The protected-file format (the same as earlier versions), and the rules for recovering from edited or
//! rolled-back files. The actual encryption (Windows DPAPI) and the generation store (HKCU registry)
//! are supplied by the platform through [`DataProtector`] and [`GenerationStore`].
//!
//! File layout: an 8-byte ASCII header, then a DPAPI blob.
//! - `PTDATA1\n` (older files): the blob decrypts to the JSON document.
//! - `PTDATA2\n`: the blob decrypts to `<generation>\n<json>`. The generation increases with every save and is also
//!   recorded (protected) outside the file, so putting back an older genuine copy is detected.
//!
//! The DPAPI "optional entropy" is `PlaytimeTracker/<purpose>/v1` (not a secret; it only stops one protected file
//! from being passed off as another).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const HEADER_V1: &[u8; 8] = b"PTDATA1\n";
pub const HEADER_V2: &[u8; 8] = b"PTDATA2\n";
/// Years of sessions are a few MB; anything bigger than this isn't ours.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// What each protected file holds. The value is part of the encryption context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Purpose {
    Sessions,
    Settings,
}

impl Purpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sessions => "sessions",
            Self::Settings => "settings",
        }
    }

    pub fn entropy(self) -> Vec<u8> {
        format!("PlaytimeTracker/{}/v1", self.as_str()).into_bytes()
    }

    pub fn generation_entropy(self) -> Vec<u8> {
        format!("PlaytimeTracker/{}/generation/v1", self.as_str()).into_bytes()
    }
}

/// Encrypts/decrypts with integrity protection (DPAPI on Windows). `unprotect` must fail on any tampering.
pub trait DataProtector {
    fn protect(&self, plain: &[u8], entropy: &[u8]) -> Result<Vec<u8>, ProtectError>;
    fn unprotect(&self, blob: &[u8], entropy: &[u8]) -> Result<Vec<u8>, ProtectError>;
}

/// Where the last saved generation of each file is kept (DPAPI-protected under HKCU on Windows).
pub trait GenerationStore {
    fn read(&self, purpose: Purpose) -> Option<u64>;
    fn write(&self, purpose: Purpose, generation: u64);
}

#[derive(Debug, Error)]
#[error("{0}")]
pub struct ProtectError(pub String);

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("{file} failed verification: {reason}")]
    Unverified { file: String, reason: String },
    #[error("{file}: {source}")]
    Io { file: String, source: io::Error },
    #[error("could not protect {file}: {source}")]
    Protect { file: String, source: ProtectError },
}

impl StoreError {
    pub fn is_unverified(&self) -> bool {
        matches!(self, Self::Unverified { .. })
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Builds the bytes of a v2 protected file.
pub fn encode(
    protector: &dyn DataProtector,
    purpose: Purpose,
    generation: u64,
    contents: &str,
) -> Result<Vec<u8>, ProtectError> {
    let plain = format!("{generation}\n{contents}");
    let blob = protector.protect(plain.as_bytes(), &purpose.entropy())?;
    let mut bytes = Vec::with_capacity(HEADER_V2.len() + blob.len());
    bytes.extend_from_slice(HEADER_V2);
    bytes.extend_from_slice(&blob);
    Ok(bytes)
}

/// Verifies and decodes a protected file's bytes into (contents, generation). v1 files are generation 0.
pub fn decode(
    protector: &dyn DataProtector,
    purpose: Purpose,
    bytes: &[u8],
    file: &str,
) -> Result<(String, u64), StoreError> {
    let unverified = |reason: &str| StoreError::Unverified {
        file: file.to_string(),
        reason: reason.to_string(),
    };
    let v2 = bytes.starts_with(HEADER_V2);
    if !v2 && !bytes.starts_with(HEADER_V1) {
        return Err(unverified("not in Playtime Tracker's protected format"));
    }
    let plain = protector
        .unprotect(&bytes[HEADER_V2.len()..], &purpose.entropy())
        .map_err(|e| unverified(&e.to_string()))?;
    let text = String::from_utf8(plain).map_err(|_| unverified("contents aren't valid UTF-8"))?;
    if !v2 {
        return Ok((text, 0));
    }
    let (generation, contents) = text
        .split_once('\n')
        .ok_or_else(|| unverified("missing generation"))?;
    let generation = generation
        .parse::<u64>()
        .map_err(|_| unverified("invalid generation"))?;
    Ok((contents.to_string(), generation))
}

/// Reads and verifies one protected file.
pub fn read_file(
    protector: &dyn DataProtector,
    purpose: Purpose,
    path: &Path,
) -> Result<(String, u64), StoreError> {
    let file = display_name(path);
    let io_err = |source| StoreError::Io {
        file: file.clone(),
        source,
    };
    let length = fs::metadata(path).map_err(io_err)?.len();
    if length > MAX_FILE_BYTES {
        return Err(StoreError::Unverified {
            file,
            reason: "larger than expected".into(),
        });
    }
    let bytes = fs::read(path).map_err(io_err)?;
    decode(protector, purpose, &bytes, &file)
}

/// What [`load_with_recovery`] found, for the UI to report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    /// The main file verified and is the newest copy.
    None,
    /// The main file failed verification; it was kept under `set_aside` and the backup was used.
    RestoredFromBackup { set_aside: PathBuf },
    /// The main file failed verification and there was no usable backup; a new one should be started.
    StartedFresh { set_aside: Option<PathBuf> },
    /// An older genuine copy had been put back over a newer one; the newer backup was used.
    RolledBackCopyReplaced { set_aside: PathBuf },
    /// The newest verified copy is older than the last save that was recorded: newer data is missing.
    NewerDataMissing { found: u64, expected: u64 },
}

/// A loaded protected file.
#[derive(Debug)]
pub struct Loaded<T> {
    pub value: Option<T>,
    pub generation: u64,
    pub recovery: Recovery,
}

/// Loads the newest verified copy among `path` and `path.bak`, following these rules:
/// a file that fails verification (or that `parse` rejects) is renamed aside (never deleted); an older genuine
/// copy put back over a newer one is detected by generation; and the recorded generation catches a rollback of
/// both copies. Returns `value: None` if there's nothing usable.
pub fn load_with_recovery<T, E: std::fmt::Display>(
    protector: &dyn DataProtector,
    generations: &dyn GenerationStore,
    purpose: Purpose,
    path: &Path,
    parse: impl Fn(&str) -> Result<T, E>,
    now_stamp: &str,
) -> Result<Loaded<T>, StoreError> {
    let backup = backup_path(path);
    let try_load = |p: &Path| -> Option<Result<(T, u64), String>> {
        if !p.exists() {
            return None;
        }
        Some(match read_file(protector, purpose, p) {
            Ok((contents, generation)) => parse(&contents)
                .map(|v| (v, generation))
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        })
    };
    let recorded = generations.read(purpose);

    let mut main = try_load(path);
    let bak = try_load(&backup).and_then(Result::ok);

    let mut set_aside_main = None;
    if let Some(Err(_)) = &main {
        set_aside_main = Some(set_aside(path, "unverified", now_stamp)?);
        main = None;
    }
    let main = main.and_then(Result::ok);

    let use_backup = match (&main, &bak) {
        (None, Some(_)) => true,
        (Some((_, m)), Some((_, b))) => b > m,
        _ => false,
    };

    let (value, generation, mut recovery) = if use_backup {
        let Some((value, generation)) = bak else {
            return Err(StoreError::Io {
                file: display_name(path),
                source: io::Error::other("backup vanished"),
            });
        };
        let recovery = if main.is_some() {
            Recovery::RolledBackCopyReplaced {
                set_aside: set_aside(path, "older", now_stamp)?,
            }
        } else {
            Recovery::RestoredFromBackup {
                set_aside: set_aside_main.unwrap_or_default(),
            }
        };
        fs::copy(&backup, path).map_err(|source| StoreError::Io {
            file: display_name(path),
            source,
        })?;
        (Some(value), generation, recovery)
    } else if let Some((value, generation)) = main {
        (Some(value), generation, Recovery::None)
    } else {
        let recovery = if set_aside_main.is_some() || backup.exists() {
            Recovery::StartedFresh {
                set_aside: set_aside_main,
            }
        } else {
            Recovery::None
        };
        return Ok(Loaded {
            value: None,
            generation: recorded.unwrap_or(0),
            recovery,
        });
    };

    // Restoring the backup already explains a one-save gap, so only report missing data when nothing else happened.
    if let Some(expected) = recorded {
        if generation < expected && recovery == Recovery::None {
            recovery = Recovery::NewerDataMissing {
                found: generation,
                expected,
            };
        }
    }
    Ok(Loaded {
        value,
        generation: generation.max(recorded.unwrap_or(0)),
        recovery,
    })
}

/// Writes a new version: generation = max(known, recorded) + 1, previous file kept as `.bak`, written atomically.
/// Returns the generation written.
pub fn save(
    protector: &dyn DataProtector,
    generations: &dyn GenerationStore,
    purpose: Purpose,
    path: &Path,
    known_generation: u64,
    contents: &str,
) -> Result<u64, StoreError> {
    let generation = known_generation.max(generations.read(purpose).unwrap_or(0)) + 1;
    let bytes =
        encode(protector, purpose, generation, contents).map_err(|source| StoreError::Protect {
            file: display_name(path),
            source,
        })?;
    write_atomic(path, &bytes, Some(&backup_path(path))).map_err(|source| StoreError::Io {
        file: display_name(path),
        source,
    })?;
    generations.write(purpose, generation);
    Ok(generation)
}

pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".bak");
    PathBuf::from(name)
}

/// Renames a file aside (`<name>.<reason>-<stamp>`) so it's kept but never read again.
pub fn set_aside(path: &Path, reason: &str, stamp: &str) -> Result<PathBuf, StoreError> {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{reason}-{stamp}"));
    let destination = PathBuf::from(name);
    fs::rename(path, &destination).map_err(|source| StoreError::Io {
        file: display_name(path),
        source,
    })?;
    Ok(destination)
}

/// Temp file + rename, keeping the previous version as `backup`, so a crash never leaves a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8], backup: Option<&Path>) -> io::Result<()> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    fs::write(&temp, bytes)?;
    if let Some(backup) = backup {
        if path.exists() {
            fs::copy(path, backup)?;
        }
    }
    fs::rename(&temp, path)
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A stand-in for DPAPI: XOR "encryption" plus a checksum, so any edit fails verification.
    pub struct FakeProtector;

    fn checksum(data: &[u8], entropy: &[u8]) -> u32 {
        data.iter().chain(entropy).fold(2_166_136_261_u32, |h, b| {
            (h ^ u32::from(*b)).wrapping_mul(16_777_619)
        })
    }

    impl DataProtector for FakeProtector {
        fn protect(&self, plain: &[u8], entropy: &[u8]) -> Result<Vec<u8>, ProtectError> {
            let mut out: Vec<u8> = plain.iter().map(|b| b ^ 0x5a).collect();
            out.extend_from_slice(&checksum(plain, entropy).to_le_bytes());
            Ok(out)
        }

        fn unprotect(&self, blob: &[u8], entropy: &[u8]) -> Result<Vec<u8>, ProtectError> {
            if blob.len() < 4 {
                return Err(ProtectError("too short".into()));
            }
            let (data, sum) = blob.split_at(blob.len() - 4);
            let plain: Vec<u8> = data.iter().map(|b| b ^ 0x5a).collect();
            if checksum(&plain, entropy).to_le_bytes() != sum {
                return Err(ProtectError("integrity check failed".into()));
            }
            Ok(plain)
        }
    }

    #[derive(Default)]
    pub struct MemoryGenerations(pub RefCell<HashMap<&'static str, u64>>);

    impl GenerationStore for MemoryGenerations {
        fn read(&self, purpose: Purpose) -> Option<u64> {
            self.0.borrow().get(purpose.as_str()).copied()
        }

        fn write(&self, purpose: Purpose, generation: u64) {
            self.0.borrow_mut().insert(purpose.as_str(), generation);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("pt-core-test-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn parse(text: &str) -> Result<String, String> {
        if text.starts_with('{') {
            Ok(text.to_string())
        } else {
            Err("not json".into())
        }
    }

    fn load(dir: &TempDir, gens: &MemoryGenerations) -> Loaded<String> {
        load_with_recovery(
            &FakeProtector,
            gens,
            Purpose::Sessions,
            &dir.0.join("sessions.dat"),
            parse,
            "20260929-120000",
        )
        .expect("loads")
    }

    #[test]
    fn encode_decode_round_trip_and_v1_compat() {
        let bytes = encode(&FakeProtector, Purpose::Sessions, 7, "{\"a\":1}").expect("encodes");
        assert_eq!(
            decode(&FakeProtector, Purpose::Sessions, &bytes, "f").expect("decodes"),
            ("{\"a\":1}".into(), 7)
        );
        // v1: header + blob of the bare JSON.
        let mut v1 = HEADER_V1.to_vec();
        v1.extend(
            FakeProtector
                .protect(b"{}", &Purpose::Sessions.entropy())
                .expect("protects"),
        );
        assert_eq!(
            decode(&FakeProtector, Purpose::Sessions, &v1, "f").expect("decodes"),
            ("{}".into(), 0)
        );
    }

    #[test]
    fn tampering_and_wrong_purpose_fail() {
        let mut bytes = encode(&FakeProtector, Purpose::Sessions, 1, "{}").expect("encodes");
        assert!(
            decode(&FakeProtector, Purpose::Settings, &bytes, "f").is_err(),
            "settings can't pose as sessions"
        );
        let last = bytes.len() - 6;
        bytes[last] ^= 1;
        assert!(decode(&FakeProtector, Purpose::Sessions, &bytes, "f")
            .expect_err("tampered")
            .is_unverified());
        assert!(decode(&FakeProtector, Purpose::Sessions, b"{\"plain\":true}", "f").is_err());
    }

    #[test]
    fn save_increments_generation_and_keeps_backup() {
        let dir = TempDir::new("save");
        let gens = MemoryGenerations::default();
        let path = dir.0.join("sessions.dat");
        assert_eq!(
            save(&FakeProtector, &gens, Purpose::Sessions, &path, 0, "{1}").expect("saves"),
            1
        );
        assert_eq!(
            save(&FakeProtector, &gens, Purpose::Sessions, &path, 1, "{2}").expect("saves"),
            2
        );
        let loaded = load(&dir, &gens);
        assert_eq!(
            (loaded.value.as_deref(), loaded.generation, loaded.recovery),
            (Some("{2}"), 2, Recovery::None)
        );
        assert_eq!(
            read_file(&FakeProtector, Purpose::Sessions, &backup_path(&path))
                .expect("bak")
                .1,
            1
        );
    }

    #[test]
    fn edited_file_is_set_aside_and_backup_restored() {
        let dir = TempDir::new("edit");
        let gens = MemoryGenerations::default();
        let path = dir.0.join("sessions.dat");
        save(&FakeProtector, &gens, Purpose::Sessions, &path, 0, "{1}").expect("saves");
        save(&FakeProtector, &gens, Purpose::Sessions, &path, 1, "{2}").expect("saves");
        fs::write(&path, b"PTDATA2\nhacked").expect("edit");
        let loaded = load(&dir, &gens);
        assert_eq!(loaded.value.as_deref(), Some("{1}"));
        assert!(matches!(
            loaded.recovery,
            Recovery::RestoredFromBackup { .. }
        ));
        assert!(
            dir.0
                .join("sessions.dat.unverified-20260929-120000")
                .exists(),
            "kept, not deleted"
        );
    }

    #[test]
    fn older_copy_put_back_is_detected() {
        let dir = TempDir::new("rollback");
        let gens = MemoryGenerations::default();
        let path = dir.0.join("sessions.dat");
        save(&FakeProtector, &gens, Purpose::Sessions, &path, 0, "{1}").expect("saves");
        let old_copy = fs::read(&path).expect("copy");
        save(&FakeProtector, &gens, Purpose::Sessions, &path, 1, "{2}").expect("saves");
        save(&FakeProtector, &gens, Purpose::Sessions, &path, 2, "{3}").expect("saves");
        fs::write(&path, old_copy).expect("put old copy back");
        let loaded = load(&dir, &gens);
        assert_eq!(
            loaded.value.as_deref(),
            Some("{2}"),
            "newest verified copy (the backup) wins"
        );
        assert!(matches!(
            loaded.recovery,
            Recovery::RolledBackCopyReplaced { .. }
        ));
    }

    #[test]
    fn rollback_of_both_copies_is_reported() {
        let dir = TempDir::new("rollback-both");
        let gens = MemoryGenerations::default();
        let path = dir.0.join("sessions.dat");
        save(&FakeProtector, &gens, Purpose::Sessions, &path, 0, "{1}").expect("saves");
        let old = fs::read(&path).expect("copy");
        save(&FakeProtector, &gens, Purpose::Sessions, &path, 1, "{2}").expect("saves");
        save(&FakeProtector, &gens, Purpose::Sessions, &path, 2, "{3}").expect("saves");
        fs::write(&path, &old).expect("old main");
        fs::write(backup_path(&path), &old).expect("old bak");
        let loaded = load(&dir, &gens);
        assert_eq!(
            loaded.recovery,
            Recovery::NewerDataMissing {
                found: 1,
                expected: 3
            }
        );
        assert_eq!(
            loaded.generation, 3,
            "next save still goes above the recorded generation"
        );
    }

    #[test]
    fn nothing_usable_starts_fresh() {
        let dir = TempDir::new("fresh");
        let gens = MemoryGenerations::default();
        fs::write(dir.0.join("sessions.dat"), b"garbage").expect("write");
        let loaded = load(&dir, &gens);
        assert!(loaded.value.is_none());
        assert!(matches!(
            loaded.recovery,
            Recovery::StartedFresh { set_aside: Some(_) }
        ));
        let empty = TempDir::new("empty");
        assert_eq!(load(&empty, &gens).recovery, Recovery::None);
    }
}
