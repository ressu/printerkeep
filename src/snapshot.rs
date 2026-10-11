//! Read the config folder: the files that a backup includes.

use crate::ignore::IgnoreRules;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A file that a backup includes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    /// Path relative to the config folder, with `/` between folders.
    pub path: Vec<u8>,
    /// Path on disk.
    pub disk: PathBuf,
    pub executable: bool,
}

/// The result of a scan.
#[derive(Debug, Default)]
pub struct Scan {
    /// Sorted by path.
    pub files: Vec<ScannedFile>,
    /// Symlinks and files that printerkeep does not back up (relative paths, for messages).
    pub skipped: Vec<String>,
}

/// Reads the config folder. Applies the ignore rules and skips symlinks.
pub fn scan(config: &Path, rules: &IgnoreRules) -> std::io::Result<Scan> {
    let mut scan = Scan::default();
    walk(config, Path::new(""), rules, &mut scan)?;
    scan.files.sort_by(|a, b| a.path.cmp(&b.path));
    scan.skipped.sort();
    Ok(scan)
}

fn walk(
    folder: &Path,
    relative: &Path,
    rules: &IgnoreRules,
    scan: &mut Scan,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        let disk = entry.path();
        let relative = relative.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(&disk)?;
        let file_type = metadata.file_type();

        if file_type.is_symlink() {
            if !rules.is_ignored_file(&relative) {
                scan.skipped.push(relative.display().to_string());
            }
        } else if file_type.is_dir() {
            if !rules.is_ignored_folder(&relative) {
                walk(&disk, &relative, rules, scan)?;
            }
        } else if file_type.is_file() {
            if rules.is_ignored_file(&relative) {
                continue;
            }
            let path = relative.as_os_str().as_bytes();
            // git's --stdin-paths reads one path per line.
            if path.contains(&b'\n') || disk.as_os_str().as_bytes().contains(&b'\n') {
                scan.skipped.push(relative.display().to_string());
                continue;
            }
            scan.files.push(ScannedFile {
                path: path.to_vec(),
                disk,
                executable: metadata.permissions().mode() & 0o111 != 0,
            });
        }
        // Sockets, pipes and devices are not configuration.
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_skips_ignored_files_folders_and_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("printer.cfg"), "[printer]\n").unwrap();
        fs::write(root.join("printer-20261011_120000.cfg"), "old\n").unwrap();
        fs::create_dir_all(root.join("macros/.git")).unwrap();
        fs::write(root.join("macros/macros.cfg"), "[gcode_macro A]\n").unwrap();
        fs::write(root.join("macros/.git/HEAD"), "ref\n").unwrap();
        fs::create_dir(root.join("timelapse")).unwrap();
        fs::write(root.join("timelapse/a.cfg"), "x\n").unwrap();
        std::os::unix::fs::symlink("/nowhere", root.join("mainsail.cfg")).unwrap();

        let rules = IgnoreRules::new(["timelapse/"]).unwrap();
        let scan = scan(root, &rules).unwrap();
        let paths: Vec<_> = scan
            .files
            .iter()
            .map(|f| String::from_utf8(f.path.clone()).unwrap())
            .collect();
        assert_eq!(paths, ["macros/macros.cfg", "printer.cfg"]);
        assert_eq!(scan.skipped, ["mainsail.cfg"]);
    }
}
