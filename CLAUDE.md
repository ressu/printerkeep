# CLAUDE.md

Guidance for Claude Code in this repository.

## Read first

- `docs/problem.md`: what printerkeep must do, and the design principles.
- `docs/solution-options.md`: the decided design (option A + common core C1–C8).
- `docs/naming.md`: **vocabulary and writing rule**, file and command names.
- `docs/implementation-plan.md`: milestones (M0–M6) and their "done when" criteria.

## Commands

```sh
nix develop                     # optional devShell with everything below (direnv loads it via .envrc)
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
shellcheck install.sh scripts/*.sh
# Klipper's and Kalico's parsers ignore printerkeep's markers (CI pins the firmware commits)
python3 -I tests/klipper/check_markers.py <klipper or kalico checkout> tests/klipper/cases
# Release build for one target (links with rust-lld, see .cargo/config.toml), and run it (ARM needs qemu)
cargo build --release --target aarch64-unknown-linux-musl
qemu-aarch64 target/aarch64-unknown-linux-musl/release/printerkeep --version
```

Try a build on a real printer, on a copy of its config folder (all files go to `~/printerkeep-test`):

```sh
scripts/try-on-printer.sh [--reset] [--copy] <ssh host> [printerkeep command and arguments...]
scripts/try-on-printer.sh --disconnect <ssh host>   # the SSH connection otherwise stays open for 10 minutes
```

Try `printerkeep install` on a printer with a local build (before a release exists). It changes the real
printer: units, `moonraker.conf`, backups of the real config folder. No sudo.

```sh
cargo build --release --target aarch64-unknown-linux-musl
scp target/aarch64-unknown-linux-musl/release/printerkeep <ssh host>:
ssh <ssh host> './printerkeep install && rm printerkeep'     # remove with: printerkeep uninstall
```

Make a release (no push; push when the user asks):

1. Set `version` in `Cargo.toml`, and run `cargo check` to update `Cargo.lock`.
2. In `CHANGELOG.md`, rename `## Unreleased` to `## <version> (<date>)`, and add an empty `## Unreleased` above it.
   `scripts/release-notes.sh <version>` shows the text of the GitHub release.
3. Commit as `release: <version>`, and tag `v<version>`. The tag starts the release workflow.

Release targets: `aarch64-unknown-linux-musl`, `armv7-unknown-linux-musleabihf`,
`arm-unknown-linux-musleabihf` (armv6, test with `qemu-arm -cpu arm1176`), `x86_64-unknown-linux-musl`.

## Rules

- **User-facing text** (CLI output, questions, errors, log, docs for users): plain, natural English that you
  would say out loud; main point first, stated positively, short active sentences with "you" and common
  contractions; a question alone on its last line, an error with the exact fix as a command on its own line.
  Terms only from `docs/naming.md` (never "share" as a verb; say "copy to shared"). The `technical-writing`
  skill has the full rules, where it is installed. In the docs, a placeholder in a command
  is in double quotes (`printerkeep connect "<url>"`), so that a command pasted unchanged can't turn `<` and `>`
  into redirections. CLI output keeps the usual `<url>`.
- **Changelog:** a change that users notice (commands, output, behavior, install, update) adds a line under
  `## Unreleased` in `CHANGELOG.md`, in the same commit. Write it for a user who decides whether to update:
  what they get, not how it's built. Groups, in this order, only when used: `### Before you update` (what the
  user must do), `### New`, `### Changed`, `### Fixed`, `### Removed`. The release fails without notes.
- **Git:** run the system `git` command (`src/git.rs`). No git library. Never run `git merge`, `git pull` or
  `git checkout` in the config folder; printerkeep writes a file there only when it has a final, valid result.
- **No root access.** Nothing in printerkeep or its installer uses sudo. Install into the user's home folder.
- **Keep dependencies few.** No async, no HTTP client, no crypto crates.
- **No personal names in the repository.** In examples and test fixtures, call printers "printer 1–4".
- **Behavior first:** for a behavior change or a bug, first write the test that shows the wanted behavior and
  see it fail; then change the code.
