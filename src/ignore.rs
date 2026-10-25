//! Ignore rules: the defaults, plus the `ignore:` option in the settings file.
//!
//! The rules work like `.gitignore` rules, without `!`:
//! - A rule without `/` matches a name in any folder (`*.swp`).
//! - A rule with `/` at the start or in the middle matches from the top of the config folder
//!   (`timelapse/*`, `/old.cfg`).
//! - A rule that ends with `/` matches folders only (`.git/`).
//! - `*` does not match `/`. `**` matches any number of folders.

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use std::path::Path;

/// Files that printerkeep never backs up, unless the user changes the code.
pub const DEFAULT_RULES: &[&str] = &[
    // Klipper SAVE_CONFIG keeps a copy of the main config file (`printer-20261011_120000.cfg`) and
    // writes a temporary `printer.cfg_autosave` (klippy/configfile.py).
    "*-[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]_[0-9][0-9][0-9][0-9][0-9][0-9].cfg",
    "*_autosave",
    // Moonraker's copy of its config file: `.moonraker.conf.bkp` (moonraker/confighelper.py).
    ".*.bkp",
    // Secrets must never go to a repository.
    "*.secrets",
    // git data of tools that users clone into the config folder.
    ".git/",
    // Editor and system files.
    "*.swp",
    "*.swo",
    "*~",
    ".DS_Store",
    "__pycache__/",
    "*.pyc",
];

/// A set of ignore rules, ready to match.
#[derive(Debug)]
pub struct IgnoreRules {
    /// Rules that match files (and folders, for rules without a trailing `/`).
    any: GlobSet,
    /// Rules that match folders only.
    folders: GlobSet,
}

#[derive(Debug, thiserror::Error)]
#[error("The {option} rule '{rule}' isn't valid: {message}")]
pub struct RuleError {
    /// The option of the settings file that has the rule.
    pub option: &'static str,
    pub rule: String,
    pub message: String,
}

impl IgnoreRules {
    /// The default rules plus the user's rules.
    pub fn new<'a>(user_rules: impl IntoIterator<Item = &'a str>) -> Result<Self, RuleError> {
        let mut any = GlobSetBuilder::new();
        let mut folders = GlobSetBuilder::new();
        for rule in DEFAULT_RULES.iter().copied().chain(user_rules) {
            let (pattern, folder_only) = translate(rule);
            if pattern.is_empty() {
                continue;
            }
            let glob = GlobBuilder::new(&pattern)
                .literal_separator(true)
                .build()
                .map_err(|e| RuleError {
                    option: "ignore",
                    rule: rule.to_string(),
                    message: e.kind().to_string(),
                })?;
            if folder_only {
                folders.add(glob);
            } else {
                any.add(glob);
            }
        }
        let build = |builder: GlobSetBuilder| {
            builder.build().map_err(|e| RuleError {
                option: "ignore",
                rule: String::new(),
                message: e.to_string(),
            })
        };
        Ok(Self {
            any: build(any)?,
            folders: build(folders)?,
        })
    }

    /// `path` is relative to the config folder.
    pub fn is_ignored_file(&self, path: &Path) -> bool {
        self.any.is_match(path)
    }

    /// `path` is relative to the config folder. An ignored folder is not read at all.
    pub fn is_ignored_folder(&self, path: &Path) -> bool {
        self.any.is_match(path) || self.folders.is_match(path)
    }
}

/// Converts a rule to a glob pattern relative to the config folder.
pub(crate) fn translate(rule: &str) -> (String, bool) {
    let rule = rule.trim();
    let folder_only = rule.ends_with('/');
    let rule = rule.trim_end_matches('/');
    let anchored = rule.starts_with('/') || rule.contains('/');
    let rule = rule.trim_start_matches('/');
    let pattern = if anchored || rule.is_empty() {
        rule.to_string()
    } else {
        format!("**/{rule}")
    };
    (pattern, folder_only)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(user: &[&str]) -> IgnoreRules {
        IgnoreRules::new(user.iter().copied()).unwrap()
    }

    #[test]
    fn defaults_ignore_generated_files() {
        let r = rules(&[]);
        assert!(r.is_ignored_file(Path::new("printer-20261011_120000.cfg")));
        assert!(r.is_ignored_file(Path::new("sub/printer-20261011_120000.cfg")));
        assert!(r.is_ignored_file(Path::new("main-20261011_120000.cfg")));
        assert!(r.is_ignored_file(Path::new("printer.cfg_autosave")));
        assert!(r.is_ignored_file(Path::new(".moonraker.conf.bkp")));
        assert!(r.is_ignored_file(Path::new("moonraker.secrets")));
        assert!(r.is_ignored_file(Path::new("macros/.macros.cfg.swp")));
        assert!(r.is_ignored_folder(Path::new(".git")));
        assert!(r.is_ignored_folder(Path::new("Klipper-Macros/.git")));
    }

    #[test]
    fn defaults_keep_configuration() {
        let r = rules(&[]);
        for path in [
            "printer.cfg",
            "printer-base.cfg",
            "printer-2026.cfg",
            "moonraker.conf",
            "variables.cfg",
            "macros/macros.cfg",
            ".printerkeep-state.json",
            "printerkeep.conf",
        ] {
            assert!(!r.is_ignored_file(Path::new(path)), "{path}");
        }
        assert!(!r.is_ignored_folder(Path::new("macros")));
    }

    #[test]
    fn user_rules() {
        let r = rules(&["timelapse/*", "/old.cfg", "*.bak", "cache/"]);
        assert!(r.is_ignored_file(Path::new("timelapse/a.cfg")));
        assert!(!r.is_ignored_file(Path::new("timelapse.cfg")));
        assert!(r.is_ignored_file(Path::new("old.cfg")));
        assert!(!r.is_ignored_file(Path::new("macros/old.cfg")));
        assert!(r.is_ignored_file(Path::new("a/b/c.bak")));
        assert!(r.is_ignored_folder(Path::new("cache")));
        assert!(r.is_ignored_folder(Path::new("x/cache")));
        assert!(!r.is_ignored_file(Path::new("cache")));
    }

    #[test]
    fn invalid_rule_is_an_error() {
        let error = IgnoreRules::new(["a[b"]).unwrap_err();
        assert_eq!(error.rule, "a[b");
    }
}
