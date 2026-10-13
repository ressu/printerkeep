//! printerkeep keeps Klipper printer configuration safe in git, and keeps printers in step.
//!
//! The design is in `docs/`. Start with `docs/problem.md`.

pub mod app;
pub mod backup;
pub mod branch_readme;
pub mod files;
pub mod git;
pub mod ignore;
pub mod install;
pub mod log;
pub mod merge;
pub mod overrides;
pub mod paths;
pub mod repository;
pub mod repository_changes;
pub mod settings;
pub mod shared;
pub mod snapshot;
pub mod state;

/// The printerkeep version, from `Cargo.toml`: three numbers, because Fluidd and Mainsail compare versions as
/// semantic versions. Releases are tagged `v<version>`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
