//! `printerkeep install` and `printerkeep uninstall`.
//!
//! The program goes into `~/printerkeep/` with `release_info.json`, so Moonraker's update_manager can
//! update it. A systemd **user** timer runs the backups. Nothing needs root access (no sudo).

use crate::paths::Paths;
use std::fs;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The GitHub repository of printerkeep. update_manager gets the releases from it.
pub const PROJECT: &str = "ressu/printerkeep";
/// The update_manager section in moonraker.conf.
pub const SECTION: &str = "[update_manager printerkeep]";
const SERVICE: &str = "printerkeep.service";
const TIMER: &str = "printerkeep.timer";
/// The Rust target of this build, from build.rs.
const TARGET: &str = env!("PRINTERKEEP_TARGET");

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error(
        "Run this command as the printer's user, not as root. printerkeep installs for one user and needs \
         no root access."
    )]
    Root,
    #[error("This build of printerkeep is not for a printer (build target {0}).")]
    UnknownTarget(String),
    #[error(
        "The config folder {0} does not exist. Use --printer-data to select the printer_data folder."
    )]
    NoConfigFolder(PathBuf),
    #[error("Cannot use the printer name: {0}")]
    Name(String),
    #[error("The HOME variable is not set.")]
    NoHome,
    #[error(
        "'systemctl --user {command}' did not work: {message}\n\
         printerkeep uses the systemd user manager. Log in over SSH as the printer's user (not with su or \
         sudo), and try again."
    )]
    Systemd { command: String, message: String },
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
}

fn io_error(context: impl Into<String>) -> impl FnOnce(std::io::Error) -> InstallError {
    let context = context.into();
    move |source| InstallError::Io { context, source }
}

/// The name of the release asset for this build, for example `printerkeep-linux-aarch64`.
pub fn asset() -> Option<&'static str> {
    asset_for(TARGET)
}

fn asset_for(target: &str) -> Option<&'static str> {
    if target.starts_with("aarch64-") {
        Some("printerkeep-linux-aarch64")
    } else if target.starts_with("armv7-") {
        Some("printerkeep-linux-armv7")
    } else if target.starts_with("arm-") {
        Some("printerkeep-linux-armv6")
    } else if target.starts_with("x86_64-") {
        Some("printerkeep-linux-x86_64")
    } else {
        None
    }
}

/// Where the installed files go, in the home folder of the user.
#[derive(Debug, Clone)]
pub struct Locations {
    /// `~/printerkeep`: the program and `release_info.json` (update_manager `path`).
    pub program_dir: PathBuf,
    /// `~/.local/bin/printerkeep`: a link to the program, so the command is `printerkeep`.
    pub link: PathBuf,
    /// `~/.config/systemd/user`: the systemd user units.
    pub units: PathBuf,
}

impl Locations {
    pub fn new(home: &Path, config_home: Option<PathBuf>) -> Self {
        let config_home = config_home.unwrap_or_else(|| home.join(".config"));
        Self {
            program_dir: home.join("printerkeep"),
            link: home.join(".local").join("bin").join("printerkeep"),
            units: config_home.join("systemd").join("user"),
        }
    }

    /// From the HOME and XDG_CONFIG_HOME variables.
    pub fn from_env() -> Result<Self, InstallError> {
        let home = std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .ok_or(InstallError::NoHome)?;
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from);
        Ok(Self::new(Path::new(&home), config_home))
    }
}

/// Installs or updates printerkeep for this user. Returns the lines to show.
/// `name`: the printer name (`--printer-name`). Without it, the name stays as it is (`main` by default).
pub fn install(
    paths: &Paths,
    at: &Locations,
    name: Option<&str>,
) -> Result<Vec<String>, InstallError> {
    refuse_root()?;
    let asset = asset().ok_or_else(|| InstallError::UnknownTarget(TARGET.to_string()))?;
    if !paths.config.is_dir() {
        return Err(InstallError::NoConfigFolder(paths.config.clone()));
    }
    // The name first: a wrong name stops the install before it changes anything.
    let renamed = match name {
        Some(name) => crate::app::rename(paths, name)
            .map_err(|error| InstallError::Name(error.to_string()))?,
        None => Vec::new(),
    };
    let printer_data = std::path::absolute(&paths.printer_data)
        .map_err(io_error("Cannot find the printer_data folder"))?;
    let mut lines = vec![format!(
        "Installing printerkeep {} ({asset}):",
        crate::VERSION
    )];

    // The program and release_info.json.
    fs::create_dir_all(&at.program_dir).map_err(io_error(format!(
        "Cannot make {}",
        at.program_dir.display()
    )))?;
    let program = at.program_dir.join(asset);
    let running = std::env::current_exe().map_err(io_error("Cannot find this program"))?;
    if !same_file(&running, &program) {
        copy_program(&running, &program)?;
    }
    write_file(
        &at.program_dir.join("release_info.json"),
        &release_info(asset),
    )?;
    lines.push(format!("- Program: {}", program.display()));

    // The command.
    lines.push(make_link(&program, &at.link)?);

    // The timer.
    let linger = turn_on_linger(paths);
    let layerlock = find_layerlock();
    fs::create_dir_all(&at.units)
        .map_err(io_error(format!("Cannot make {}", at.units.display())))?;
    write_file(
        &at.units.join(SERVICE),
        &service_unit(&program, &printer_data, layerlock.as_deref()),
    )?;
    write_file(&at.units.join(TIMER), TIMER_UNIT)?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", TIMER])?;
    lines.push(
        "- Timer: printerkeep.timer (a systemd user unit) makes a backup every 5 minutes, \
         when a file changed."
            .to_string(),
    );
    lines.push(match &layerlock {
        Some(_) => "- layerlock: no backup starts during a print.".to_string(),
        None => {
            "- layerlock: not installed. Backups can start during a print (with a low priority)."
                .to_string()
        }
    });
    lines.push(linger);

    // Updates through Moonraker.
    lines.extend(add_update_manager(&paths.config.join("moonraker.conf"))?);

    // The first backup.
    lines.push(String::new());
    lines.extend(
        renamed
            .into_iter()
            .filter(|l| !l.starts_with("No files changed")),
    );
    match crate::app::backup(paths, true) {
        Ok(report) => lines.extend(report.lines()),
        Err(error) => lines.push(format!("The first backup did not work: {error}")),
    }
    lines.push(String::new());
    lines.push(format!(
        "printerkeep is installed. The log is {}.",
        paths.log.display()
    ));
    // The saved state: a printerkeep.conf from an earlier installation stays (uninstall keeps it).
    let settings = crate::settings::load(&paths.settings()).unwrap_or_default();
    let name = settings
        .printer_name
        .clone()
        .unwrap_or_else(|| crate::app::DEFAULT_NAME.to_string());
    match &settings.repository {
        Some(url) => {
            lines.push(format!(
                "This printer is connected to {url} as {name} (from printerkeep.conf). To connect it \
                 differently: printerkeep add-repository <url> --printer-name <name>"
            ));
        }
        None => {
            let source = if settings.printer_name.is_some() {
                "from printerkeep.conf"
            } else {
                "the default"
            };
            lines.push(format!("Printer name: {name} ({source})."));
            lines.push(
                "To send the backups to a repository: printerkeep add-repository <url>".to_string(),
            );
        }
    }
    lines.push("To remove printerkeep: printerkeep uninstall".to_string());
    Ok(lines)
}

/// Removes the program, the timer, the link and the update_manager section. Keeps the history, the
/// login for the repository and printerkeep.conf.
pub fn uninstall(paths: &Paths, at: &Locations) -> Result<Vec<String>, InstallError> {
    refuse_root()?;
    let mut lines = Vec::new();

    let units = [at.units.join(SERVICE), at.units.join(TIMER)];
    if units.iter().any(|unit| unit.exists()) {
        // The timer can be stopped already. Removing the files is what counts.
        let _ = systemctl(&["disable", "--now", TIMER]);
        for unit in &units {
            remove_file(unit)?;
        }
        let _ = systemctl(&["daemon-reload"]);
        lines.push("Removed printerkeep.service and printerkeep.timer.".to_string());
    }

    if fs::read_link(&at.link).is_ok_and(|target| target.starts_with(&at.program_dir)) {
        remove_file(&at.link)?;
        lines.push(format!("Removed {}.", at.link.display()));
    }

    let moonraker = paths.config.join("moonraker.conf");
    if let Ok(text) = fs::read_to_string(&moonraker)
        && let Some(new_text) = remove_section(&text)
    {
        replace_file(&moonraker, &new_text)?;
        lines.push(format!(
            "Removed {SECTION} from moonraker.conf. Restart Moonraker to remove printerkeep from the \
             update panel."
        ));
    }

    if at.program_dir.exists() {
        fs::remove_dir_all(&at.program_dir).map_err(io_error(format!(
            "Cannot remove {}",
            at.program_dir.display()
        )))?;
        lines.push(format!("Removed {}.", at.program_dir.display()));
    }

    let marker = linger_marker(paths);
    if marker.exists() {
        lines.push(format!(
            "printerkeep install turned on lingering for user {}. If nothing else needs it, turn it \
             off with: loginctl disable-linger",
            user_name()
        ));
        remove_file(&marker)?;
    }

    lines.push("printerkeep is removed.".to_string());
    for kept in [paths.data.clone(), paths.settings()] {
        if kept.exists() {
            lines.push(format!("Kept: {}", kept.display()));
        }
    }
    Ok(lines)
}

fn refuse_root() -> Result<(), InstallError> {
    // /proc/self belongs to the user that runs this process.
    match fs::metadata("/proc/self") {
        Ok(meta) if meta.uid() == 0 => Err(InstallError::Root),
        _ => Ok(()),
    }
}

fn user_name() -> String {
    std::env::var("USER").unwrap_or_else(|_| "of this program".to_string())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// Copies the program in one step: a running program can be replaced, and there is never a half copy.
fn copy_program(from: &Path, to: &Path) -> Result<(), InstallError> {
    let context = format!("Cannot copy the program to {}", to.display());
    let program = fs::read(from).map_err(io_error(context.clone()))?;
    crate::files::replace(to, &program, Some(0o755)).map_err(io_error(context))
}

fn release_info(asset: &str) -> String {
    let (owner, project) = PROJECT.split_once('/').expect("PROJECT is owner/name");
    format!(
        "{{\n  \"project_name\": \"{project}\",\n  \"project_owner\": \"{owner}\",\n  \
         \"version\": \"v{}\",\n  \"asset_name\": \"{asset}\"\n}}\n",
        crate::VERSION
    )
}

/// Links `~/.local/bin/printerkeep` to the program. Never replaces a file that is not a link.
fn make_link(program: &Path, link: &Path) -> Result<String, InstallError> {
    if let Ok(meta) = fs::symlink_metadata(link) {
        if !meta.file_type().is_symlink() {
            return Ok(format!(
                "- Command: not changed, because {} is not a link. Use {}.",
                link.display(),
                program.display()
            ));
        }
        remove_file(link)?;
    }
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent)
            .map_err(io_error(format!("Cannot make {}", parent.display())))?;
    }
    std::os::unix::fs::symlink(program, link)
        .map_err(io_error(format!("Cannot make the link {}", link.display())))?;
    let in_path = link.parent().is_some_and(|dir| {
        std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|p| p == dir))
    });
    Ok(if in_path {
        format!("- Command: printerkeep ({})", link.display())
    } else {
        format!(
            "- Command: printerkeep ({}). Log in again if the shell does not find the command.",
            link.display()
        )
    })
}

fn find_layerlock() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain([PathBuf::from("/usr/local/bin")])
        .map(|dir| dir.join("layerlock"))
        .find(|file| {
            fs::metadata(file).is_ok_and(|meta| meta.is_file() && meta.mode() & 0o111 != 0)
        })
}

/// Problems with the installation that the user must know about, for `printerkeep status`. systemd skips
/// a backup when layerlock cannot run, so printerkeep cannot tell it in the log.
pub fn problems(at: &Locations) -> Vec<String> {
    let Ok(service) = fs::read_to_string(at.units.join(SERVICE)) else {
        // Not installed with printerkeep install (for example a test copy).
        return Vec::new();
    };
    let condition = service
        .lines()
        .find_map(|line| line.strip_prefix("ExecCondition="))
        .map(unit_unquote);
    match (condition, find_layerlock()) {
        (Some(layerlock), _) if !layerlock.exists() => vec![format!(
            "Problem: the backup service uses layerlock at {}, but that file does not exist. systemd \
             skips every backup. Run: printerkeep install",
            layerlock.display()
        )],
        (None, Some(layerlock)) => vec![format!(
            "Note: layerlock is installed ({}), but the backup service does not use it. Backups can start \
             during a print. Run: printerkeep install",
            layerlock.display()
        )],
        _ => Vec::new(),
    }
}

/// The first word of a systemd command line, as written by `unit_quote`.
fn unit_unquote(value: &str) -> PathBuf {
    let Some(rest) = value.strip_prefix('"') else {
        return PathBuf::from(value.split_whitespace().next().unwrap_or_default());
    };
    let mut text = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => text.extend(chars.next()),
            '"' => break,
            c => text.push(c),
        }
    }
    PathBuf::from(text.replace("%%", "%"))
}

/// Quotes a path for a systemd unit file.
fn unit_quote(path: &Path) -> String {
    let text = path
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("\"{text}\"")
}

fn service_unit(program: &Path, printer_data: &Path, layerlock: Option<&Path>) -> String {
    let condition = match layerlock {
        // --fail-open: only a print (printing or paused) skips a backup. When Moonraker does not answer,
        // the backup runs.
        Some(layerlock) => format!("ExecCondition={} --fail-open\n", unit_quote(layerlock)),
        None => String::new(),
    };
    format!(
        "# Made by printerkeep install. printerkeep install replaces this file.\n\
         [Unit]\n\
         Description=printerkeep backup of the printer configuration\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         {condition}\
         ExecStart={} --printer-data {} backup\n\
         Nice=10\n\
         IOSchedulingClass=idle\n",
        unit_quote(program),
        unit_quote(printer_data)
    )
}

const TIMER_UNIT: &str = "# Made by printerkeep install. printerkeep install replaces this file.
[Unit]
Description=printerkeep backup every 5 minutes

[Timer]
OnCalendar=*:0/5
RandomizedDelaySec=30

[Install]
WantedBy=timers.target
";

fn systemctl(args: &[&str]) -> Result<(), InstallError> {
    let command = args.join(" ");
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .map_err(|e| InstallError::Systemd {
            command: command.clone(),
            message: e.to_string(),
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(InstallError::Systemd {
            command,
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

/// Without lingering, systemd stops the user's timers when the user logs out. A user can turn on
/// lingering for themselves on most systems. If that is not permitted, printerkeep tells the user.
fn turn_on_linger(paths: &Paths) -> String {
    let user = user_name();
    let uid = fs::metadata("/proc/self").map(|m| m.uid().to_string());
    let state = uid.ok().and_then(|uid| {
        Command::new("loginctl")
            .args(["show-user", &uid, "--property=Linger", "--value"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    });
    if state.as_deref() == Some("yes") {
        return format!(
            "- Lingering: on for user {user}. Backups run also when nobody is logged in."
        );
    }
    let turned_on = Command::new("loginctl")
        .arg("enable-linger")
        .output()
        .is_ok_and(|o| o.status.success());
    if turned_on {
        // Remember it, so that uninstall can tell the user.
        let _ = fs::create_dir_all(&paths.data).and_then(|()| fs::write(linger_marker(paths), ""));
        format!(
            "- Lingering: turned on for user {user}, so backups run also when nobody is logged in."
        )
    } else {
        format!(
            "- Lingering: off. Backups run only while user {user} is logged in. To run them always, \
             an administrator can run: sudo loginctl enable-linger {user}"
        )
    }
}

fn linger_marker(paths: &Paths) -> PathBuf {
    paths.data.join("linger-turned-on")
}

fn update_manager_section() -> String {
    format!(
        "{SECTION}\n\
         type: executable\n\
         channel: stable\n\
         repo: {PROJECT}\n\
         path: ~/printerkeep\n\
         # printerkeep runs from a timer. An update does not need to restart a service.\n\
         is_system_service: False\n"
    )
}

fn has_section(text: &str) -> bool {
    text.lines().any(|line| line.trim_end() == SECTION)
}

/// Adds the update_manager section to moonraker.conf, if it is not there.
fn add_update_manager(moonraker: &Path) -> Result<Vec<String>, InstallError> {
    let text = match fs::read_to_string(moonraker) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut lines = vec![format!(
                "- Updates: no moonraker.conf in the config folder. For updates in Fluidd and Mainsail, \
                 add this to your Moonraker configuration:"
            )];
            lines.extend(update_manager_section().lines().map(|l| format!("    {l}")));
            return Ok(lines);
        }
        Err(source) => {
            return Err(InstallError::Io {
                context: format!("Cannot read {}", moonraker.display()),
                source,
            });
        }
    };
    if has_section(&text) {
        return Ok(vec![format!(
            "- Updates: moonraker.conf already has {SECTION}."
        )]);
    }
    let mut addition = String::new();
    if !text.is_empty() && !text.ends_with('\n') {
        addition.push('\n');
    }
    if !text.is_empty() {
        addition.push('\n');
    }
    addition.push_str(&update_manager_section());
    // Append, so the rest of the file and its permissions stay the same.
    fs::OpenOptions::new()
        .append(true)
        .open(moonraker)
        .and_then(|mut file| file.write_all(addition.as_bytes()))
        .map_err(io_error(format!("Cannot write {}", moonraker.display())))?;
    Ok(vec![format!(
        "- Updates: added {SECTION} to moonraker.conf. Restart Moonraker to show printerkeep in the \
         update panel of Fluidd or Mainsail."
    )])
}

/// moonraker.conf without the update_manager section, or None if it has no such section. The section
/// goes from its header to the next section. Empty lines before the header go with it.
fn remove_section(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let header = lines.iter().position(|l| l.trim_end() == SECTION)?;
    let mut end = lines[header + 1..]
        .iter()
        .position(|l| l.starts_with('['))
        .map_or(lines.len(), |i| header + 1 + i);
    let mut start = header;
    if end < lines.len() {
        // Keep the empty lines between this section and the next one.
        while end > header + 1 && lines[end - 1].trim().is_empty() {
            end -= 1;
        }
    }
    while start > 0 && lines[start - 1].trim().is_empty() {
        start -= 1;
    }
    let mut result: String = lines[..start].concat();
    if end < lines.len() && start == 0 {
        // The section was the first one: no empty lines at the top.
        while lines.get(end).is_some_and(|l| l.trim().is_empty()) {
            end += 1;
        }
    }
    result.push_str(&lines[end..].concat());
    Some(result)
}

fn write_file(path: &Path, text: &str) -> Result<(), InstallError> {
    fs::write(path, text).map_err(io_error(format!("Cannot write {}", path.display())))
}

/// Replaces a file in one step and keeps its permissions.
fn replace_file(path: &Path, text: &str) -> Result<(), InstallError> {
    crate::files::replace(path, text.as_bytes(), None)
        .map_err(io_error(format!("Cannot write {}", path.display())))
}

fn remove_file(path: &Path) -> Result<(), InstallError> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(InstallError::Io {
            context: format!("Cannot remove {}", path.display()),
            source: e,
        }),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_for_the_release_targets() {
        assert_eq!(
            asset_for("aarch64-unknown-linux-musl"),
            Some("printerkeep-linux-aarch64")
        );
        assert_eq!(
            asset_for("armv7-unknown-linux-musleabihf"),
            Some("printerkeep-linux-armv7")
        );
        assert_eq!(
            asset_for("arm-unknown-linux-musleabihf"),
            Some("printerkeep-linux-armv6")
        );
        assert_eq!(
            asset_for("x86_64-unknown-linux-musl"),
            Some("printerkeep-linux-x86_64")
        );
        assert_eq!(asset_for("mips-unknown-linux-musl"), None);
    }

    #[test]
    fn remove_section_gives_back_the_file_before_install() {
        let section = update_manager_section();
        for before in [
            "[server]\nhost: 0.0.0.0\n",
            "[server]\nhost: 0.0.0.0",
            "[server]\nhost: 0.0.0.0\n\n",
            "",
        ] {
            let mut text = before.to_string();
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&section);
            let removed = remove_section(&text).unwrap();
            assert_eq!(removed.trim_end(), before.trim_end(), "file: {before:?}");
        }
    }

    #[test]
    fn remove_section_keeps_the_next_section() {
        let text = format!(
            "[server]\nhost: 0.0.0.0\n\n{}\n[update_manager fluidd]\ntype: web\n",
            update_manager_section()
        );
        assert_eq!(
            remove_section(&text).unwrap(),
            "[server]\nhost: 0.0.0.0\n\n[update_manager fluidd]\ntype: web\n"
        );
        let first = format!(
            "{}\n[update_manager fluidd]\ntype: web\n",
            update_manager_section()
        );
        assert_eq!(
            remove_section(&first).unwrap(),
            "[update_manager fluidd]\ntype: web\n"
        );
        assert_eq!(remove_section("[server]\n"), None);
    }

    #[test]
    fn unit_paths_are_quoted() {
        let unit = service_unit(
            Path::new("/home/pi/printerkeep/printerkeep-linux-aarch64"),
            Path::new("/home/pi/printer 50%"),
            None,
        );
        assert!(unit.contains(
            "ExecStart=\"/home/pi/printerkeep/printerkeep-linux-aarch64\" --printer-data \
             \"/home/pi/printer 50%%\" backup\n"
        ));
        assert!(!unit.contains("ExecCondition"));
    }

    #[test]
    fn unit_paths_are_read_back() {
        for path in [
            "/usr/local/bin/layerlock",
            "/home/pi/my \"bin\" 50%/layerlock",
        ] {
            let quoted = unit_quote(Path::new(path));
            assert_eq!(
                unit_unquote(&format!("{quoted} --fail-open")),
                PathBuf::from(path)
            );
        }
        assert_eq!(
            unit_unquote("/usr/bin/layerlock --fail-open"),
            PathBuf::from("/usr/bin/layerlock")
        );
    }
}
