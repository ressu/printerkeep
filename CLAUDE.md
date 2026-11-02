# CLAUDE.md

Guidance for Claude Code in this repository.

## Read first

- `docs/problem.md`: what printerkeep must do, and the design principles.
- `docs/solution-options.md`: the decided design (option A + common core C1–C8).
- `docs/naming.md`: **vocabulary and writing rule**, file and command names.
- `docs/implementation-plan.md`: the code layout (Architecture), the milestones (all done), and later decisions.

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

Integration tests use `tests/common/mod.rs`: `repository()` makes a local bare repository, which stands in for
GitHub. `Printer::connected(name, url)` makes a printer, and `web_edit`, `show` and `git` read or change the
repository directly.

Try a change on this computer, on a snapshot of a printer's config folder (all files go to `target/try-local`;
the printer is only read, and git may use only the local test repository). Use this for development passes:

```sh
scripts/try-local.sh --from <ssh host>    # take a snapshot, and start again with an empty test repository
scripts/try-local.sh [printerkeep command and arguments...]   # default: sync; edit the files between runs
scripts/try-local.sh --reset              # start again from the snapshot
```

Try a build on a real printer, on a copy of its config folder (all files go to `~/printerkeep-test`). Only for
a last check, for example of a release, and only when the user asks:

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
2. In `CHANGELOG.md`, rename `## Unreleased` to `## <version> (<date>)`. Check it against the commits with
   `git cliff --unreleased` (`cliff.toml`): each `feat:` and `fix:` commit has a note. `scripts/release-notes.sh
   <version>` shows the text of the GitHub release.
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
- **Commit subjects are the release notes.** A change that users notice is a `feat:` or `fix:` commit, and its
  subject says what users get, not how it's built ("copy-to-shared asks before it copies"). A subject that
  starts with "remove" goes under Removed, with "rename" under Changed. When users must do something after
  the update, mark it `feat!:` and say what in a `BREAKING CHANGE:` footer; it goes under "Before you update".
  Other types (`docs:`, `test:`, `refactor:`, `chore:`, `release:`) stay out of the notes.
- **Release notes as you go.** Each `feat:` or `fix:` commit also adds its note under `## Unreleased` in
  `CHANGELOG.md`, in the same commit: what users get, and what they notice after the update.
- **Mixed versions share one repository.** Before changing a file that printerkeep writes there (a README,
  the state file), check what the last release does with the new format. The change must be one-way, with a
  test for it. Printers can also use other URLs for the same repository (SSH and HTTPS), so text written to
  `shared` must not change only because of the URL.
- **Git:** run the system `git` command (`src/git.rs`). No git library. Never run `git merge`, `git pull` or
  `git checkout` in the config folder; printerkeep writes a file there only when it has a final, valid result.
- **No root access.** Nothing in printerkeep or its installer uses sudo. Install into the user's home folder.
- **Keep dependencies few.** No async, no HTTP client, no crypto crates.
- **No personal names in the repository.** In examples and test fixtures, call printers "printer 1–4".
- **Behavior first:** for a behavior change or a bug, first write the test that shows the wanted behavior and
  see it fail; then change the code.
