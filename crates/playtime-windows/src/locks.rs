//! Holds data files open read-only (sharing read only), so other programs can read them but can't modify,
//! replace or delete them while the app runs. Released briefly around the app's own saves.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use windows::Win32::Storage::FileSystem::FILE_SHARE_READ;

#[derive(Debug, Default)]
pub struct FileLocks {
    held: HashMap<PathBuf, File>,
}

impl FileLocks {
    pub fn hold(&mut self, path: &Path) {
        if self.held.contains_key(path) || !path.exists() {
            return;
        }
        // Another program having it open for writing just means we try again after the next save.
        if let Ok(file) = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0)
            .open(path)
        {
            self.held.insert(path.to_path_buf(), file);
        }
    }

    pub fn release(&mut self, path: &Path) {
        self.held.remove(path);
    }

    pub fn is_held(&self, path: &Path) -> bool {
        self.held.contains_key(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_file_cannot_be_written_by_others() {
        let path = std::env::temp_dir().join(format!("pt-lock-test-{}.dat", std::process::id()));
        std::fs::write(&path, b"data").expect("write");
        let mut locks = FileLocks::default();
        locks.hold(&path);
        assert!(locks.is_held(&path));
        assert!(std::fs::read(&path).is_ok(), "others can still read");
        assert!(std::fs::write(&path, b"changed").is_err(), "but not write");
        assert!(std::fs::remove_file(&path).is_err(), "or delete");
        locks.release(&path);
        std::fs::write(&path, b"ours").expect("writable again after release");
        std::fs::remove_file(&path).expect("cleanup");
    }
}
