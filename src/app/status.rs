//! `printerkeep status`.

use super::*;

/// The state of the printer, for `printerkeep status`.
#[derive(Debug)]
pub struct Status {
    pub printer_name: String,
    pub config: PathBuf,
    pub last_backup: Option<LastBackup>,
    /// Changes in the config folder since the last backup.
    pub changes: Changes,
    /// The files with only bundled changes, which wait for the next backup, as a sentence.
    pub waiting: Option<String>,
    pub skipped: Vec<String>,
    pub repository: Option<String>,
    /// Backups that are not in the repository yet. `None` without a repository or backups.
    pub not_sent: Option<u64>,
    /// Why the last try to send did not work.
    pub send_problem: Option<String>,
    /// The followed files and their status.
    pub followed: Vec<(String, shared::FileStatus)>,
}

#[derive(Debug)]
pub struct LastBackup {
    pub time: String,
    pub summary: String,
    pub count: u64,
    pub files: usize,
}

/// Reads the state of the printer. Changes nothing and uses no network.
pub fn status(paths: &Paths) -> Result<Status, Error> {
    let settings = settings::load(&paths.settings())?;
    let rules = IgnoreRules::new(settings.ignore.iter().map(String::as_str))?;
    let bundle = BundleRules::new(settings.bundled_changes())?;
    let printer_name = printer_name(&settings)?;
    let readme = settings
        .repository
        .as_deref()
        .map(|url| branch_readme::content(&printer_name, url, &settings.follow));
    let scan = backup_scan(
        paths,
        Files {
            rules: &rules,
            readme: readme.as_deref(),
        },
    )?;

    let git = Git::new(&paths.git_dir);
    let reference = branch(&printer_name);
    let mut not_sent = None;
    let mut waiting = None;
    let (last_backup, changes) = if git.exists() && git.resolve(&reference)?.is_some() {
        let prepared = backup::prepare(&git, &scan, &reference, false, Some(&bundle))?;
        let commit = prepared.parent.as_deref().unwrap_or_default();
        let (time, summary) = git.commit_info(commit)?;
        let last = LastBackup {
            time: crate::log::timestamp(UNIX_EPOCH + Duration::from_secs(time)),
            summary,
            count: git.count_commits(commit)?,
            files: {
                let tree = git.read_tree(commit)?;
                tree.len() - usize::from(branch_readme::in_tree(&git, &tree)?)
            },
        };
        if settings.repository.is_some() {
            not_sent = Some(match git.resolve(&repository::sent_ref(&printer_name))? {
                Some(sent) if git.is_ancestor(&sent, commit)? => {
                    git.count_between(&sent, commit)?
                }
                _ => last.count,
            });
        }
        waiting = prepared.waiting_text();
        (Some(last), prepared.changes)
    } else {
        let changes = Changes {
            added: scan
                .files
                .iter()
                .map(|f| String::from_utf8_lossy(&f.path).into_owned())
                .collect(),
            ..Changes::default()
        };
        (None, changes)
    };

    let mut followed = Vec::new();
    if !settings.follow.is_empty() {
        let state = state::load(&paths.config.join(STATE_FILE))?;
        let tip = if git.exists() {
            git.resolve(SHARED_REF)?
        } else {
            None
        };
        for path in &settings.follow {
            let status = if shared::check_path(path).is_err() {
                shared::FileStatus::NotInShared
            } else {
                shared::file_status(&git, paths, path, &state, tip.as_deref())?
            };
            followed.push((path.clone(), status));
        }
    }

    Ok(Status {
        followed,
        printer_name,
        config: paths.config.clone(),
        last_backup,
        changes,
        waiting,
        skipped: scan.skipped,
        repository: settings.repository,
        not_sent,
        send_problem: std::fs::read_to_string(paths.send_problem())
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty()),
    })
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Printer name: {}", self.printer_name)?;
        writeln!(f, "Config folder: {}", self.config.display())?;
        match &self.last_backup {
            Some(last) => {
                writeln!(f, "Last backup: {} ({})", last.time, last.summary)?;
                writeln!(f, "Backups: {}", last.count)?;
                writeln!(f, "Files in the last backup: {}", last.files)?;
                if let Some(waiting) = &self.waiting {
                    writeln!(f, "Changes since the last backup: {waiting}")?;
                } else if self.changes.is_empty() {
                    writeln!(f, "Changes since the last backup: none")?;
                } else {
                    writeln!(
                        f,
                        "Changes since the last backup: {}. The next backup includes them.",
                        self.changes.summary()
                    )?;
                }
            }
            None => {
                writeln!(
                    f,
                    "Last backup: none yet. To make the first one, run: printerkeep backup"
                )?;
                writeln!(f, "Files to back up: {}", self.changes.added.len())?;
            }
        }
        if !self.skipped.is_empty() {
            writeln!(f, "Not backed up (symlinks): {}", self.skipped.join(", "))?;
        }
        if !self.followed.is_empty() {
            writeln!(f, "Followed files (from shared):")?;
            for (path, status) in &self.followed {
                writeln!(f, "  {path}: {}", status.text())?;
            }
        }
        match &self.repository {
            Some(url) => {
                writeln!(f, "Repository: {url}")?;
                match self.not_sent {
                    Some(0) => {
                        writeln!(f, "All backups are in the repository.")?;
                        match &self.send_problem {
                            Some(problem) => writeln!(f, "The last run had a problem:\n{problem}"),
                            None => Ok(()),
                        }
                    }
                    Some(n) => match &self.send_problem {
                        Some(problem) => writeln!(
                            f,
                            "Backups not in the repository: {n}. The last try to send them didn't \
                             work:\n{problem}"
                        ),
                        None => writeln!(
                            f,
                            "Backups not in the repository: {n}. The next backup sends them."
                        ),
                    },
                    None => Ok(()),
                }
            }
            None => writeln!(
                f,
                "Repository: none. The history is on this printer only. \
                 To connect it to a repository: printerkeep connect <url>"
            ),
        }
    }
}
