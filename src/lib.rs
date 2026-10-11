//! printerkeep keeps Klipper printer configuration safe in git, and keeps printers in step.
//!
//! The design is in `docs/`. Start with `docs/problem.md`.

/// The printerkeep version, from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
