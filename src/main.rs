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
    /// Make a backup, get the changes from shared and the repository, and send new backups to the repository.
    ///
    /// The timer runs this every 5 minutes.
    Sync,
    /// Make a backup of the config folder, if a file changed, and send it to the repository.
    ///
    /// Never changes your files: changes from shared and the repository wait for printerkeep sync.
    Backup,
    /// Show the status of the printer and its backups.
    Status,
    /// Connect this printer to a repository.
    ///
    /// With an https:// URL, printerkeep asks for a personal access token (or reads it from standard input).
    /// With an SSH URL (git@host:path), printerkeep makes an SSH key and shows the public key.
    ///
    /// printerkeep shows the printers in the repository. If it does not know which one this printer is, it
    /// asks (on a terminal). Without a terminal it never guesses: use the options below.
    Connect {
        /// The repository URL, without a token.
        url: String,
        /// This printer's name in the repository. If the repository has a branch with this name, printerkeep
        /// continues it (it asks first, if its history is not this printer's); if not, it starts one.
        #[arg(long, value_name = "PRINTER_NAME")]
        printer_name: Option<String>,
        /// Continue the repository's branch also when its history is not this printer's (a new install of
        /// this printer, or its klipper-backup branch). Your files don't change. The backups that printerkeep
        /// made on this printer so far stay here and aren't sent.
        #[arg(long = "continue")]
        continue_history: bool,
        /// For a new branch: send only a backup of the files now, not the backups made on this printer
        /// before (they are set aside).
        #[arg(long)]
        from_now: bool,
    },
    /// Replace the personal access token of the repository, for example when it expired.
    ///
    /// printerkeep tests the new token. It replaces the old token only if the repository accepts the new one.
    ChangeToken,
    /// Put files back from a backup: one file or all files, from the last backup or from a time.
    ///
    /// printerkeep first makes a backup of the files now, so you can always go back. It never removes files.
    ///
    /// On a terminal without --at, printerkeep lists the earlier versions to pick from. With --at, it takes
    /// the backup from that time; if there are several backups on that day, a terminal lists them.
    ///
    /// After a new install, give the repository with --repository: printerkeep connects to it and continues
    /// the printer's history.
    Restore {
        /// The file to restore, a path in the config folder, for example printer.cfg. Without it: all files.
        file: Option<String>,
        /// The time, for example "2 days ago", yesterday, 2026-10-12 or "2026-10-12 14:00".
        #[arg(long, value_name = "TIME")]
        at: Option<String>,
        /// The repository to restore from. On a printer that isn't connected yet, printerkeep connects to it.
        #[arg(long, value_name = "URL")]
        repository: Option<String>,
        /// The printer to restore from (its branch in the repository). After a new install: which printer
        /// this is.
        #[arg(long, value_name = "PRINTER_NAME")]
        printer_name: Option<String>,
    },
    /// Start to follow shared files. printerkeep then writes the changes from shared into them.
    ///
    /// If a file is on this printer, its differences from the shared version stay as local changes.
    Follow {
        /// The paths in the config folder, for example macros/macros.cfg.
        #[arg(required = true, value_name = "FILE")]
        files: Vec<String>,
    },
    /// Stop following shared files. The files stay on this printer.
    Unfollow {
        /// The paths in the config folder.
        #[arg(required = true, value_name = "FILE")]
        files: Vec<String>,
    },
    /// Make the shared version of files equal to the version on this printer, and follow the files.
    ///
    /// Other printers that follow a file get the changes at their next sync.
    ///
    /// printerkeep shows the changes that go to shared, and asks first.
    CopyToShared {
        /// The paths in the config folder. All files go to shared in one copy.
        #[arg(required = true, value_name = "FILE")]
        files: Vec<String>,
        /// Copy all changes without questions. Necessary when printerkeep does not run on a terminal.
        #[arg(long)]
        yes: bool,
    },
    /// Finish files that need a decision, without editing them.
    ///
    /// Usually you edit the file in Fluidd or Mainsail: keep the lines that you want and delete the ####
    /// lines. printerkeep records the decision at the next sync.
    Decide {
        /// The paths in the config folder. The same choice applies to each file.
        #[arg(required = true, value_name = "FILE")]
        files: Vec<String>,
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
        /// its own name when several printers use the same repository.
        #[arg(long, value_name = "PRINTER_NAME")]
        printer_name: Option<String>,
    },
    /// Give this printer a new name. Its history moves to the new name.
    ///
    /// The next send makes a new branch with the new name in the repository, with the whole history.
    /// printerkeep does not rename or remove the old branch in the repository: remove it in the web pages of
    /// the repository if you do not need it. If the repository already has a branch with the new name,
    /// rename stops: it never takes over a branch.
    Rename {
        /// The new printer name, for example printer-2.
        #[arg(value_name = "PRINTER_NAME")]
        printer_name: String,
    },
    /// Remove what install made. Keeps the history, the login for the repository and printerkeep.conf.
    Uninstall,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let Some(printer_data) = cli.printer_data.or_else(default_printer_data) else {
        eprintln!(
            "printerkeep: can't find the home folder. To give the printer_data folder, add --printer-data \
             <folder>."
        );
        return ExitCode::FAILURE;
    };
    let paths = Paths::new(printer_data);

    // A backup run logs only what is new: nothing for a run with no change, and a problem only when it starts,
    // changes or is solved. No write to the SD card for a run that repeats the last one.
    let mut log_only: Option<Vec<String>> = None;
    let result = match &cli.command {
        Command::Sync => {
            // By hand (on a terminal), check the repository now. The timer follows check_repository.
            let check_now = std::io::stdin().is_terminal();
            app::sync(&paths, check_now).map(|report| {
                log_only = Some(report.log_lines());
                report.lines()
            })
        }
        Command::Backup => app::backup(&paths).map(|report| {
            log_only = Some(report.log_lines());
            report.lines()
        }),
        Command::Status => app::status(&paths).map(|status| {
            print!("{status}");
            if let Ok(at) = install::Locations::from_env() {
                for problem in install::problems(&at) {
                    println!("{problem}");
                }
            }
            Vec::new()
        }),
        Command::Connect {
            url,
            printer_name,
            continue_history,
            from_now,
        } => {
            let choices = app::ConnectChoices {
                printer_name: printer_name.clone(),
                continue_history: *continue_history,
                from_now: *from_now,
            };
            app::connect(&paths, url, &choices, &read_token, &Terminal).map(|report| report.lines())
        }
        Command::Restore {
            file,
            at,
            repository,
            printer_name,
        } => {
            let choices = app::RestoreChoices {
                file: file.clone(),
                at: at.clone(),
                repository: repository.clone(),
                printer_name: printer_name.clone(),
            };
            app::restore(&paths, &choices, &read_token, &Terminal).map(|report| report.lines())
        }
        Command::ChangeToken => app::change_token(&paths, &read_token),
        Command::Follow { files } => app::follow(&paths, files),
        Command::Unfollow { files } => app::unfollow(&paths, files),
        Command::CopyToShared { files, yes } => {
            app::copy_to_shared(&paths, files, &CopyQuestions { yes: *yes })
        }
        Command::Decide {
            files, use_shared, ..
        } => app::decide(&paths, files, *use_shared),
        Command::Rename { printer_name } => app::rename(&paths, printer_name),
        Command::Install { printer_name } => install::Locations::from_env()
            .and_then(|at| install::install(&paths, &at, printer_name.as_deref()))
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
            let log = log_only.unwrap_or_else(|| lines.clone());
            if !log.is_empty() {
                log_lines(&paths, &log);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("printerkeep: {error}");
            // A message of more lines: each line gets its own time in the log.
            let text = format!("Error: {error}");
            let lines: Vec<String> = text.lines().map(str::to_string).collect();
            log_lines(&paths, &lines);
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
            "printerkeep: can't write the log {}: {error}",
            paths.log.display()
        );
    }
}

/// Questions on the terminal. Without a terminal, there are no answers: printerkeep does not guess.
struct Terminal;

impl app::Ask for Terminal {
    fn confirm(&self, question: &str) -> Option<bool> {
        let answer = self.text(&format!("{question} [y/N]"))?;
        Some(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
    }

    fn text(&self, question: &str) -> Option<String> {
        let stdin = std::io::stdin();
        if !stdin.is_terminal() {
            return None;
        }
        eprint!("{question}: ");
        let mut answer = String::new();
        stdin.lock().read_line(&mut answer).ok()?;
        // An empty line after the answer: what follows is a group of its own.
        eprintln!();
        Some(answer.trim().to_string())
    }
}

/// The questions of a copy to shared, on a terminal. With `--yes`, copies all changes without questions.
struct CopyQuestions {
    yes: bool,
}

impl CopyQuestions {
    /// Asks until the answer is one of `answers`. An empty answer is the first one.
    fn ask(question: &str, answers: &[&str]) -> std::io::Result<String> {
        let stdin = std::io::stdin();
        if !stdin.is_terminal() {
            return Err(std::io::Error::other(
                "copy-to-shared asks before it copies. Run it on a terminal, or add --yes",
            ));
        }
        loop {
            eprint!("{question} ");
            let mut answer = String::new();
            if stdin.lock().read_line(&mut answer)? == 0 {
                return Ok(answers[0].to_string());
            }
            let answer = answer.trim().to_lowercase();
            let answer = if answer.is_empty() {
                answers[0]
            } else {
                answer.as_str()
            };
            if answers.contains(&answer) {
                eprintln!();
                return Ok(answer.to_string());
            }
        }
    }
}

impl app::CopyChoice for CopyQuestions {
    fn how(&self, preview: &str, can_pick: bool) -> std::io::Result<app::How> {
        print!("{preview}");
        if !preview.ends_with('\n') {
            println!();
        }
        if self.yes {
            return Ok(app::How::All);
        }
        if !can_pick {
            let answer = Self::ask("Copy to shared? [y/N]", &["n", "no", "y", "yes"])?;
            return Ok(match answer.as_str() {
                "y" | "yes" => app::How::All,
                _ => app::How::Cancel,
            });
        }
        eprintln!();
        eprintln!("  1  Copy all changes");
        eprintln!("  2  Choose change by change");
        eprintln!("  3  Cancel");
        let answer = Self::ask(
            "How do you want to copy to shared? [1/2/3]",
            &["3", "1", "2"],
        )?;
        Ok(match answer.as_str() {
            "1" => app::How::All,
            "2" => app::How::Pick,
            _ => app::How::Cancel,
        })
    }

    fn change(&self, question: &app::ChangeQuestion) -> std::io::Result<app::Pick> {
        let place = match &question.section {
            Some(section) => format!("{}, [{section}]", question.path),
            None => question.path.to_string(),
        };
        eprintln!(
            "Change {} of {} in {place}:\n{}",
            question.number, question.total, question.lines
        );
        eprintln!("  y  copy this change to shared");
        if question.can_keep_always {
            eprintln!("  n  keep it on this printer this time");
            eprintln!("  o  keep it on this printer always (adds # printerkeep: override)");
            let answer = Self::ask("Copy this change? [y/N/o]", &["n", "y", "o"])?;
            return Ok(match answer.as_str() {
                "y" => app::Pick::Send,
                "o" => app::Pick::KeepAlways,
                _ => app::Pick::Keep,
            });
        }
        eprintln!("  n  keep it on this printer (you're asked again at the next copy)");
        let answer = Self::ask("Copy this change? [y/N]", &["n", "y"])?;
        Ok(match answer.as_str() {
            "y" => app::Pick::Send,
            _ => app::Pick::Keep,
        })
    }
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
            "The repository refused the stored token. It may have expired or been removed.\n\
             printerkeep needs a new personal access token for {url}."
        ),
        app::TokenRequest::Replace => eprintln!(
            "Give the new personal access token for {url}. printerkeep tests it before it replaces the \
             old token."
        ),
    }
    if url.contains("github.com") {
        eprintln!(
            "To make one on GitHub, go to Settings > Developer settings > Personal access tokens > \
             Fine-grained tokens > Generate new token.\n\
             Under \"Repository access\", select \"Only select repositories\" and then this repository.\n\
             Under \"Permissions\", give \"Contents\" the access \"Read and write\"."
        );
    }
    eprint!("Token (it stays hidden while you type): ");
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
