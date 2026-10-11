use clap::Parser;

/// Keeps Klipper printer configuration safe in git, and keeps printers in step.
#[derive(Parser)]
#[command(name = "printerkeep", version = printerkeep::VERSION)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
}
