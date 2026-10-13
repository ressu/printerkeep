use clap::{Parser, Subcommand};
use printerkeep::{app, install, log, paths::Paths};
use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;
use std::process::{Command as Process, ExitCode, Stdio};

/// Keeps Klipper printer configuration safe in git, and keeps printers in step.
#[derive(Parser)]
#[command(name = "printerkeep", version = printerkeep::VERSION)]
struct Cli {
    /// The printer_data folder. The default is ~/printer_data.
    #[arg(
        long,
        global = true,
        env = "PRINTERKEEP_PRINTER_DATA",
        value_name = "FOLDER"
    )]
    printer_data: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Make a backup of the config folder, if a file changed, and send new backups to the repository.
    Backup,
    /// Show the status of the printer and its backups.
    Status,
    /// Connect this printer to a repository.
    ///
    /// With an https:// URL, printerkeep asks for a personal access token (or reads it from standard input).
    /// With an SSH URL (git@host:path), printerkeep makes an SSH key and shows the public key.
    AddRepository {
        /// The repository URL, without a token.
        url: String,
    },
    /// Replace the personal access token of the repository, for example when it expired.
    ///
    /// printerkeep tests the new token. It replaces the old token only if the repository accepts the new one.
    ChangeToken,
    /// Restore the config folder from the repository, for example after a new install.
    ///
    /// Never removes files. Files that are different from the backup are replaced only with --overwrite.
    Restore {
        /// The repository URL, without a token.
        url: String,
        /// The printer to restore. Necessary if the repository has backups of more printers.
        printer_name: Option<String>,
        /// Replace files in the config folder that are different from the backup.
        #[arg(long)]
        overwrite: bool,
    },
    /// Start to follow a shared file. printerkeep then writes the changes from shared into it.
    ///
    /// If the file is on this printer, its differences from the shared version stay as local changes.
    Follow {
        /// The path in the config folder, for example macros/macros.cfg.
        file: String,
    },
    /// Stop following a shared file. The file stays on this printer.
    StopFollowing {
        /// The path in the config folder.
        file: String,
    },
    /// Make the shared version of a file equal to the version on this printer, and follow the file.
    ///
    /// Other printers that follow the file get the changes at their next backup.
    ///
    /// printerkeep shows the changes that go to shared, and asks first.
    CopyToShared {
        /// The path in the config folder.
        file: String,
        /// Do not ask. Necessary when printerkeep does not run on a terminal.
        #[arg(long)]
        yes: bool,
    },
    /// Finish a file that needs decision, without editing it.
    ///
    /// Usually you edit the file in Fluidd or Mainsail: keep the lines that you want and delete the ####
    /// lines. printerkeep records the decision at the next backup.
    Decide {
        /// The path in the config folder.
        file: String,
        /// Use the shared lines.
        #[arg(
            long,
            conflicts_with = "keep_mine",
            required_unless_present = "keep_mine"
        )]
        use_shared: bool,
        /// Keep the lines of this printer.
        #[arg(long)]
        keep_mine: bool,
    },
    /// Install printerkeep for this user, or update the installation. Needs no root access (no sudo).
    ///
    /// Puts the program in ~/printerkeep (for Moonraker's update manager), links ~/.local/bin/printerkeep,
    /// makes a backup every 5 minutes with a systemd user timer, and adds [update_manager printerkeep] to
    /// moonraker.conf.
    Install {
        /// The printer name: the name of its branch in the repository. Without it: main. Give each printer
        /// its own name when more printers use the same repository.
        #[arg(long)]
        name: Option<String>,
    },
    /// Give the printer a new name. Its history moves to the branch with the new name.
    ///
    /// In the repository, the old branch stays. Remove it in the web pages of the repository if you do not
    /// need it.
    Rename {
        /// The new printer name, for example printer-2.
        name: String,
    },
    /// Remove what install made. Keeps the history, the login for the repository and printerkeep.conf.
    Uninstall,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let Some(printer_data) = cli.printer_data.or_else(default_printer_data) else {
        eprintln!("printerkeep: cannot find the home folder. Use --printer-data.");
        return ExitCode::FAILURE;
    };
    let paths = Paths::new(printer_data);

    // A backup run with no changes is not logged: no write to the SD card.
    let mut quiet = false;
    let result = match &cli.command {
        Command::Backup => {
            // By hand (on a terminal), check the repository now. The timer follows check_repository.
            let check_now = std::io::stdin().is_terminal();
            app::backup(&paths, check_now).map(|report| {
                quiet = report.quiet;
                report.lines()
            })
        }
        Command::Status => app::status(&paths).map(|status| {
            print!("{status}");
            if let Ok(at) = install::Locations::from_env() {
                for problem in install::problems(&at) {
                    println!("{problem}");
                }
            }
            Vec::new()
        }),
        Command::AddRepository { url } => {
            app::add_repository(&paths, url, &read_token).map(|report| report.lines())
        }
        Command::Restore {
            url,
            printer_name,
            overwrite,
        } => app::restore(
            &paths,
            url,
            printer_name.as_deref(),
            *overwrite,
            &read_token,
        )
        .map(|report| report.lines()),
        Command::ChangeToken => app::change_token(&paths, &read_token),
        Command::Follow { file } => app::follow(&paths, file),
        Command::StopFollowing { file } => app::stop_following(&paths, file),
        Command::CopyToShared { file, yes } => {
            let confirm = |preview: &str| confirm_copy(preview, *yes);
            app::copy_to_shared(&paths, file, &confirm)
        }
        Command::Decide {
            file, use_shared, ..
        } => app::decide(&paths, file, *use_shared),
        Command::Rename { name } => app::rename(&paths, name),
        Command::Install { name } => install::Locations::from_env()
            .and_then(|at| install::install(&paths, &at, name.as_deref()))
            .map_err(app::Error::from),
        Command::Uninstall => install::Locations::from_env()
            .and_then(|at| install::uninstall(&paths, &at))
            .map_err(app::Error::from),
    };

    match result {
        Ok(lines) => {
            for line in &lines {
                println!("{line}");
            }
            if !lines.is_empty() && !quiet {
                log_lines(&paths, &lines);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("printerkeep: {error}");
            log_lines(&paths, &[format!("Error: {error}")]);
            ExitCode::FAILURE
        }
    }
}

fn default_printer_data() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("printer_data"))
}

fn log_lines(paths: &Paths, lines: &[String]) {
    if let Err(error) = log::append(&paths.log, lines) {
        eprintln!(
            "printerkeep: cannot write the log {}: {error}",
            paths.log.display()
        );
    }
}

/// Shows the changes of a copy to shared, and asks on a terminal.
fn confirm_copy(preview: &str, yes: bool) -> std::io::Result<bool> {
    print!("{preview}");
    if !preview.ends_with('\n') {
        println!();
    }
    if yes {
        return Ok(true);
    }
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return Err(std::io::Error::other(
            "copy-to-shared asks before it copies. Run it on a terminal, or add --yes",
        ));
    }
    eprint!("Copy to shared? [y/N] ");
    let mut answer = String::new();
    stdin.lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}

/// Reads a personal access token. On a terminal, asks for it and does not show it.
fn read_token(request: app::TokenRequest, url: &str) -> std::io::Result<String> {
    let stdin = std::io::stdin();
    let mut line = String::new();
    if !stdin.is_terminal() {
        stdin.lock().read_line(&mut line)?;
        return Ok(line);
    }

    match request {
        app::TokenRequest::New => {
            eprintln!("printerkeep needs a personal access token for {url}.")
        }
        app::TokenRequest::StoredRefused => eprintln!(
            "The repository did not accept the stored token. It can be expired or removed.\n\
             printerkeep needs a new personal access token for {url}."
        ),
        app::TokenRequest::Replace => eprintln!(
            "Give the new personal access token for {url}. printerkeep tests it before it replaces the \
             old token."
        ),
    }
    if url.contains("github.com") {
        eprintln!(
            "To make one on GitHub: Settings > Developer settings > Personal access tokens > \
             Fine-grained tokens > Generate new token.\n\
             Repository access: \"Only select repositories\", select this repository.\n\
             Permissions: \"Contents\", \"Read and write\"."
        );
    }
    eprint!("Token (it is not shown on the screen): ");
    let echo_off = stty("-echo");
    let result = stdin.lock().read_line(&mut line);
    if echo_off {
        stty("echo");
        eprintln!();
    }
    result.map(|_| line)
}

fn stty(setting: &str) -> bool {
    Process::new("stty")
        .arg(setting)
        .stdin(Stdio::inherit())
        .status()
        .is_ok_and(|s| s.success())
}
