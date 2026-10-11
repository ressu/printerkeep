use clap::{Parser, Subcommand};
use printerkeep::{app, log, paths::Paths};
use std::path::PathBuf;
use std::process::ExitCode;

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
    /// Make a backup of the config folder, if a file changed.
    Backup,
    /// Show the status of the printer and its backups.
    Status,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let Some(printer_data) = cli.printer_data.or_else(default_printer_data) else {
        eprintln!("printerkeep: cannot find the home folder. Use --printer-data.");
        return ExitCode::FAILURE;
    };
    let paths = Paths::new(printer_data);

    let result = match cli.command {
        Command::Backup => app::backup(&paths).map(|report| {
            let lines = report.lines();
            for line in &lines {
                println!("{line}");
            }
            log_lines(&paths, &lines);
        }),
        Command::Status => app::status(&paths).map(|status| print!("{status}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
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
