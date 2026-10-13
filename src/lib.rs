//! printerkeep keeps Klipper printer configuration safe in git, and keeps printers in step.
//!
//! The design is in `docs/`. Start with `docs/problem.md`.

pub mod app;
pub mod backup;
pub mod files;
pub mod git;
pub mod ignore;
pub mod install;
pub mod log;
pub mod merge;
pub mod overrides;
pub mod paths;
pub mod readme;
pub mod repository;
pub mod repository_changes;
pub mod settings;
pub mod shared;
pub mod snapshot;
pub mod state;

/// The printerkeep version: major.minor from `Cargo.toml` (whose patch number is always 0). Releases are
/// tagged `v<major>.<minor>`.
pub const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR")
);
