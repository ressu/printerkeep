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
# Release build for one target, and run it (ARM needs qemu)
cargo zigbuild --release --target aarch64-unknown-linux-musl
qemu-aarch64 target/aarch64-unknown-linux-musl/release/printerkeep --version
```

Release targets: `aarch64-unknown-linux-musl`, `armv7-unknown-linux-musleabihf`,
`arm-unknown-linux-musleabihf` (armv6, test with `qemu-arm -cpu arm1176`), `x86_64-unknown-linux-musl`.

## Rules

- **User-facing text** (CLI output, log, docs for users) uses only the terms in `docs/naming.md` and
  follows its writing rule: one word, one meaning, simple verbs, short sentences. Never use "share" as a verb;
  say "copy to shared".
- **Git:** run the system `git` command (`src/git.rs`). No git library. Never run `git merge`, `git pull` or
  `git checkout` in the config folder; printerkeep writes a file there only when it has a final, valid result.
- **Keep dependencies few.** No async, no HTTP client, no crypto crates.
- **No personal names in the repository.** In examples and test fixtures, call printers "printer 1–4".
- **Version control is jj** (colocated with git). Start a new change with `jj new` before you edit files.
