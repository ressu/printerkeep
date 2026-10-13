//! Writing a file in one step: a temporary file next to it, then a rename. Klipper, Moonraker or an editor
//! never see a half-written file, and a symlink at the path is replaced instead of written through.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// Replaces (or makes) `path` with `content`, and makes its folder. `mode`: the permissions of the file;
/// `None` keeps the permissions of the old file (0644 for a new file).
pub fn replace(path: &Path, content: &[u8], mode: Option<u32>) -> io::Result<()> {
    let folder = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no folder"))?;
    std::fs::create_dir_all(folder)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = folder.join(format!(".{name}.printerkeep-new"));
    let mode = mode
        .or_else(|| {
            std::fs::metadata(path)
                .ok()
                .map(|m| m.permissions().mode() & 0o7777)
        })
        .unwrap_or(0o644);
    std::fs::write(&temporary, content)?;
    std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(mode))?;
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_keeps_the_mode_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b.cfg");
        replace(&path, b"one", None).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        replace(&path, b"two", None).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        replace(&path, b"three", Some(0o755)).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert_eq!(std::fs::read_dir(dir.path().join("a")).unwrap().count(), 1);
    }
}
