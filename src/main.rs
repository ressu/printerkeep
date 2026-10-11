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
    /// Install printerkeep for this user, or update the installation. Needs no root access (no sudo).
    ///
    /// Puts the program in ~/printerkeep (for Moonraker's update manager), links ~/.local/bin/printerkeep,
    /// makes a backup every 5 minutes with a systemd user timer, and adds [update_manager printerkeep] to
    /// moonraker.conf.
    Install,
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

    let result = match &cli.command {
        Command::Backup => app::backup(&paths).map(|report| report.lines()),
        Command::Status => app::status(&paths).map(|status| {
            print!("{status}");
            Vec::new()
        }),
        Command::AddRepository { url } => {
            app::add_repository(&paths, url, &|| read_token(url)).map(|report| report.lines())
        }
        Command::Restore {
            url,
            printer_name,
            overwrite,
        } => app::restore(&paths, url, printer_name.as_deref(), *overwrite, &|| {
            read_token(url)
        })
        .map(|report| report.lines()),
        Command::Install => install::Locations::from_env()
            .and_then(|at| install::install(&paths, &at))
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
            if !lines.is_empty() {
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

/// Reads a personal access token. On a terminal, asks for it and does not show it.
fn read_token(url: &str) -> std::io::Result<String> {
    let stdin = std::io::stdin();
    let mut line = String::new();
    if !stdin.is_terminal() {
        stdin.lock().read_line(&mut line)?;
        return Ok(line);
    }

    eprintln!("printerkeep needs a personal access token for {url}.");
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
