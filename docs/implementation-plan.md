# Implementation plan

Status: draft (2026-10-11). Based on [problem.md](problem.md), [solution-options.md](solution-options.md),
[naming.md](naming.md) and [language.md](language.md).

## Main milestones

| Milestone | Content |
|---|---|
| **One printer** | Local history, repository, restore, installer, update_manager, klipper-backup history. A replacement for klipper-backup; the authors move their own printers to printerkeep. |
| **More printers** | Follow, changes from shared, needs decision, override, copy to shared, status. Changes in the repository get to the printer (M6). |

Both use the same repository format (one branch per printer, files at the top), so the second needs no
migration. Release numbers are not planned ahead: a release happens when its work is ready (see
"Versions").

## Versions

Three numbers, `<major>.<minor>.<patch>`, tagged `v<version>` (for example `v0.2.0`). No pre-releases.
Release numbers are not planned ahead.

History: on 2026-10-13 we chose `major.minor` (`v0.1`), because Moonraker's update_manager only checks if the
installed version is the same as the release title. v0.1 was released that way. Then Fluidd did not offer
the update: Fluidd and Mainsail offer an update only when the installed version and the release title are
both valid semantic versions (`semver.valid(…, { loose: true })`, which needs three numbers) and the release
is greater. Thus, from the release after v0.1, versions have three numbers again. The release workflow
refuses a version that is not three numbers, and a tag that does not match. A printer with v0.1 must be
updated by hand once (its installed version is not a semantic version).

Future (2026-10-13): simpler releases. The 0.2.0 release changed six places by hand: the version in
`Cargo.toml` (and `Cargo.lock`), four examples in the README (three URLs and `VERSION=`), and two comments in
`install.sh`. Ideas to decide later:

- Docs without a version: the README examples use `<version>` (with a link to the release page), or the
  `releases/latest/download/` URLs where a fixed release is not needed. Then only `Cargo.toml` changes.
- A release script (`scripts/release.sh <version>`): set the version in `Cargo.toml`, update `Cargo.lock`,
  run the checks, make the jj change and the tag. It prints the push commands, and does not push.
- The version from the tag: the release workflow sets the version from the tag (for example with
  `cargo set-version`), so that `Cargo.toml` does not change for a release. Local builds then show the last
  version in `Cargo.toml`, which can be wrong.

## Project setup

- **License:** MIT (the same as layerlock).
- **Rust:** edition 2024. `rust-toolchain.toml` pins the toolchain for every contributor. An optional
  `flake.nix` gives a Nix devShell with the toolchain, `git`, Python 3 (for the Klipper
  parser tests) and `jj`.
- **One crate** with a library and a binary (`src/lib.rs`, `src/main.rs`). Split into a workspace
  (`printerkeep-*`) only if a real need appears.
- **CLAUDE.md**: short. It points to the docs, the vocabulary and the writing rule in
  [naming.md](naming.md).
- **GitHub repository** `printerkeep`: create it when the user decides (it is public).

### Dependencies (keep few)

| Need | Crate |
|---|---|
| Command line | `clap` (derive) |
| State file | `serde`, `serde_json` |
| Errors | `thiserror` (library), `anyhow` (binary) |
| Ignore patterns | `globset` |
| Tests | `tempfile`, `proptest`, `assert_cmd` |

No async, no HTTP client, no git library, no crypto. printerkeep runs `git` for repository work and
`ssh-keygen` for keys.

## Architecture

```
src/
  main.rs               command line (clap), token and confirm prompts, log lines
  app/                  the commands: errors, settings, printer name, lock (mod.rs)
    run.rs              the backup run: backup, changes from shared and the repository, send
    connection.rs       connect, change-token, restore, rename
    shared_files.rs     follow, stop-following, copy-to-shared, decide
    status.rs           printerkeep status
  settings.rs           printerkeep.conf: strict Moonraker-style subset (C3)
  paths.rs              config folder, printer_data, git data, log file
  files.rs              write a file in one step (temporary file, rename)
  git.rs                thin wrapper around the git command (GIT_DIR, GIT_INDEX_FILE, GIT_SSH_COMMAND)
  ignore.rs             default and user ignore rules
  snapshot.rs           read the config folder: ignore rules, no symlinks (C2)
  backup.rs             commit only on change, commit messages (C2)
  repository.rs         URLs, tokens, keys, send, continue a history, restore (C8)
  repository_changes.rs changes on the printer's own branch in the repository (M6)
  branch_readme.rs      README.md on each printer branch and on shared
  log.rs                printer_data/logs/printerkeep.log
  install.rs            install and uninstall (systemd user units, update_manager)
  state.rs              .printerkeep-state.json: base per followed file (C3)
  merge.rs              three-way merge per file, needs-decision block (C4, C5)
  overrides.rs          override lines (C6)
  shared.rs             the shared branch, changes from shared, status of followed files
```

### Git commands that printerkeep uses

All are old, stable commands: `hash-object -w`, `update-index` (with a separate `GIT_INDEX_FILE`),
`write-tree`, `commit-tree`, `update-ref`, `rev-parse`, `cat-file`, `ls-tree`, `fetch`, `push`,
`merge-file`. printerkeep never runs `git merge`, `git pull` or `git checkout` in the config folder (C1).

### Merge engine

1. Apply the override rule: for each override line in the printer's file, find the line with the same
   `[section]` and option name in the base and the shared version, and replace it with the printer's line.
   Thus, the merge cannot change an override line. Overrides apply to `option: value` lines only (decided
   2026-10-11). Lines in `gcode:` blocks have no stable key.
2. Run `git merge-file --diff3 -p --marker-size=<large>` on printer / base / shared.
3. Read git's output as a list of parts: merged lines and conflict parts (printer, base, shared).
4. Write each conflict part as a `#### printerkeep: needs decision` block (C5).

`merge.rs` hides git behind one interface, so a merge in Rust can replace it later.

## Milestones

Each milestone ends with working, tested software.

### M0. Skeleton and release pipeline

- Cargo project, license, `rust-toolchain.toml`, `flake.nix`, CLAUDE.md, README stub.
- CI: `cargo fmt --check`, `clippy -D warnings`, tests.
- Release workflow on a tag: build the four musl targets (since 2026-10-13 with `cargo build` and `rust-lld`,
  see `.cargo/config.toml`; before, with `cargo-zigbuild`), publish
  `printerkeep-linux-{aarch64,armv7,armv6,x86_64}` and a SHA-256 checksum file. (Pre-releases were removed later, see "Versions".)
- **Done when:** a tag makes a release with five files, and each binary runs `printerkeep --version` (CI runs
  the ARM binaries with qemu-user).

### M1. Local backup (S1, S6)

- Settings parser (strict subset, clear error messages with line numbers).
- Default ignore rules. First, list what Moonraker, KlipperScreen, Crowsnest, timelapse, Spoolman and
  Mobileraker write into the config folder (open question 3).
- Snapshot: ignore rules, skip symlinks, compare without line-ending differences, store files as they are.
- Commit only on change. Message lists the changed files. Detect "only the `SAVE_CONFIG` block changed".
- Log file with the status.
- Commands: `backup`, `status`.
- **Done when:** integration tests with real git in temporary folders show: no commit without a change, no
  commit for a line-ending-only change, ignored files and symlinks are not in the tree, `SAVE_CONFIG`
  message is correct.

### M2. Repository and restore (S2, S3)

- `connect <url>`, two kinds of authentication (the URL selects):
  - `https://`: personal access token, from a prompt or standard input. Stored with
    `git -c credential.helper="store --file=printer_data/printerkeep/credentials"` (mode 600). Show the
    GitHub steps for a fine-grained token (one repository, "Contents: read and write").
  - SSH: make an ed25519 key with `ssh-keygen` in `printer_data/printerkeep/`, use it with
    `GIT_SSH_COMMAND`, show the public key and the deploy-key steps.

  Both: test the connection, write `repository:` (the URL only, never a token) to `printerkeep.conf`.
  Authentication errors (expired token, removed key) give a clear status.
- **Existing printer branch:** if the branch exists in the repository, printerkeep continues its history.
  The next backup is a child of the branch's last commit. This is needed to move from klipper-backup.
- Push after each backup. If the repository has commits that the printer does not have, printerkeep does
  **not** force-push. It keeps its backups locally and writes a clear status ("the repository has changes
  that this printer does not have"). M6 merges them.
- `restore <url> [--printer-name <name>]`: fetch the branch and write its files into the config folder. If the
  folder has files that are different from the branch, printerkeep stops and lists them, unless the user
  adds `--overwrite`. After a restore, printerkeep tells the user to restart Moonraker and Klipper. (Changed
  2026-10-14: `restore --repository <url>`, no `--overwrite`; see "Restore from the history" below M7.)
- Printer name: `printer_name:` in the settings file, default the host name. Not `shared`. (Changed
  2026-10-13: default `main`, see "Printer names" below M6.)
- **Done when:** integration tests with a local bare repository cover: first push, continued history on an
  existing branch, no force-push when the repository is ahead, restore to an empty folder, refusal and
  `--overwrite` on a non-empty folder.

**Decisions made in M2 (2026-10-11):**

- No git remote is configured. Each network command gets the URL and the login. A token uses
  `-c credential.helper=` (removes the user's own helpers) and git's `store` helper with printerkeep's file.
  SSH uses printerkeep's own key and `known_hosts` file.
- **Tokens (changed 2026-10-11, after the first live test):**
  - The token is stored per `printer_data` folder (`printer_data/printerkeep/credentials`), not per user. A
    test copy in another folder has its own token.
  - `connect` uses the stored token. It asks for a token only if there is none, or if the repository
    does not accept it.
  - `change-token` replaces the token. A new token goes to `credentials.new` first. It replaces the old token
    only after the repository accepts it, so a wrong token never breaks a working login.
  - git's `store` helper removes a token when the repository refuses it once. printerkeep's helper answers
    `get` only, so a short failure on the server cannot remove a token that works. Only printerkeep changes
    the file.
  - The tests use a local HTTPS git server (`tests/support/https_git_server.py`, python3 and openssl) that
    accepts one token, as GitHub does.
- A folder or `file://` URL is a repository without login (a network share, and the tests).
- printerkeep remembers the last backup that it sent (`refs/printerkeep/sent/<printer name>`). When nothing
  changed, a run makes no network connection.
- When the repository already has the printer's branch, printerkeep continues it. Backups that were only on
  the printer are kept as `refs/printerkeep/kept/<printer name>-<time>`. They are not sent and not removed.
- `backup` exits with 0 when the backup worked, also when sending failed. The output, the log and `status`
  show the problem. A printer that is offline for a short time is not a failed service.
- A URL with a token in it is refused, because printerkeep.conf (which is backed up) would contain it.
- `connect` and `restore` write `printer_name` and `repository` to printerkeep.conf, so a new host
  name does not start a new history.

### M3. Installer and updates (S4, S5)

- Spike first: confirm the `executable` type details (`is_system_service`, managed services,
  `moonraker.asvc`) in the Moonraker source.
- **No root access.** Everything goes in the home folder of the printer's user. No step uses `sudo`.
- `printerkeep install` (in Rust, with tests): copy the program to `~/printerkeep/`, write
  `release_info.json`, link `~/.local/bin/printerkeep`, install `printerkeep.service` and `printerkeep.timer`
  as systemd **user** units (default every 5 minutes), add `ExecCondition=` when layerlock is installed,
  turn on lingering, add `[update_manager printerkeep]` to `moonraker.conf` if it is not there.
- `printerkeep uninstall`: remove units, link, program folder and update_manager entry. Keep the git data,
  the login and `printerkeep.conf`.
- `install.sh`, based on layerlock's installer: map `uname -m` to the asset (with a separate armv6 asset),
  check the SHA-256 checksum (`BINARY_SHA256` or the checksum file), the `VERSION` variable,
  then run `printerkeep install`. The README shows three ways to install: the script in one command, the
  script read and checked against the release page, and no script.
- **Done when:** on a real printer, one command installs printerkeep, the timer makes backups, and Fluidd's
  update panel updates printerkeep from one release to the next.

**Spike results (2026-10-11, from the Moonraker source, update_manager `net_deploy.py` and `app_deploy.py`):**

- update_manager saves the downloaded asset with its asset name: `~/printerkeep/printerkeep-linux-<cpu>`.
  The program in the program folder has that name. `printerkeep install` links `~/.local/bin/printerkeep` to it, so
  the command is `printerkeep`. The systemd service runs the file in the program folder.
- update_manager compares the **title** of the GitHub release with `version` in `release_info.json`. An
  update happens when they are different. The release workflow sets the title to the tag.
- `channel: beta` takes the newest release, also a pre-release. `channel: stable` takes the latest release.
- `is_system_service` is true by default. Then Moonraker restarts the service after an update, and it must be
  in `moonraker.asvc`, or Moonraker shows a warning. printerkeep runs from a timer, so the entry has
  `is_system_service: False`.
- The program folder must not be in a git repository (the home folder is permitted).

**Decisions made in M3 (2026-10-11):**

- **No sudo.** The user is careful with root access, and printerkeep does not need it. The program, the link
  and the units are in the home folder. This has costs, which we accept:
  - systemd user units stop when the user logs out, unless lingering is on. `printerkeep install` runs
    `loginctl enable-linger` (a user can do this for themselves on most systems). If the system refuses,
    printerkeep shows the command for an administrator. `uninstall` does not turn lingering off, because other
    user units can need it. It tells the user if printerkeep turned it on.
  - Moonraker manages system services only. Thus, printerkeep is not in the services menu of Fluidd and
    Mainsail, and the installer does not change `moonraker.asvc`. The manual start is `printerkeep backup`.
    The `PRINTERKEEP_BACKUP` macro is not built yet (Klipper has no standard way to run a command).
  - printerkeep does not restart Moonraker. It tells the user to restart Moonraker, so that the update panel
    shows printerkeep.
- **The install steps are in the program**, not in the script: they have tests, and the way without a script
  is short (download, compare the checksum with the release page, run `install`). `install.sh` only
  downloads and checks. It is a release asset, so each release has a matching script.
- A checksum file from the same place as the program finds only a damaged download. The README says so. For a
  real check, `BINARY_SHA256` (or the way without a script) uses the checksum on the GitHub release page.
- The release tag must be `v<version in Cargo.toml>`, because `install` writes that version to
  `release_info.json`. The release workflow checks it.
- The timer runs at every fifth minute (`OnCalendar=*:0/5`), with a random delay of up to 30 seconds. A
  calendar timer runs also after a skipped run (layerlock).
- The service runs with `Nice=10` and `IOSchedulingClass=idle`, so a backup does not slow Klipper.
- layerlock runs with `--fail-open`: only a print (printing or paused) skips a backup. When Moonraker does not
  answer, the backup runs. `install` finds layerlock in `PATH` or `/usr/local/bin` and writes its path into the
  service. When layerlock cannot run, systemd skips every backup and printerkeep cannot log it. Thus,
  `status` tells the user when the service uses a layerlock that does not exist, or when layerlock is installed
  but the service does not use it. In both cases, `printerkeep install` fixes the service.
- The update channel follows the installed version: a pre-release (`0.1.0-rc.1`) gets `channel: beta`.
  (Removed later: no pre-releases, always `channel: stable`. See "Versions".)
- `install` makes the first backup, after it changed `moonraker.conf`.

### M4. First release

- User README: install, first backup, connect a repository, restore.
- People who move from klipper-backup use the same repository: `connect` continues the
  klipper-backup history (see "Move from klipper-backup").
- **Done when:** the authors' printers run the first release.

**Decisions made in M4 (2026-10-12):**

- `status` shows why the last try to send did not work (for example an expired token), because `status`
  uses no network. `backup` writes it to `printer_data/printerkeep/send-problem`, and removes it after a send
  that works.
- No README in the printer branch, as klipper-backup has: a printer branch is the config folder, and restore
  writes all its files. (Changed in M5 part 6: a README.md that is not in the config folder.)

### M5. More printers

- Spike first: a CI job that loads test configurations with Klipper's own parser (`klippy/configfile.py`, a
  pinned Klipper checkout, Python 3). It proves that `#### printerkeep` blocks, commented-out lines and
  override comments have no effect, also in multi-line `gcode:` values (open question 2).
- State file, `follow`, `stop-following`, the `shared` branch.
- Merge engine (above), changes from shared (C4), needs decision and its detection (C5), `decide`.
- Override rule in merges and in `copy-to-shared` (C6). `copy-to-shared` shows the changes it sends and the
  override lines it keeps back.
- Changes in the repository on the printer's own branch (M6): the same per-file merge, with the printer's
  branch in the repository as the other side.
- `README.md` on `shared`: static (see part 7).
- Status per followed file in the log and in `status`.
- **Done when:** property tests show the merge rules (an override line never changes; when changes touch
  different lines, the result has both; a needs-decision file is always valid configuration), and
  integration tests with three printers cover M1–M9.

**Decisions made in M5 (2026-10-12):**

- Spike result: Klipper and Kalico delete all text from the first `#` on each line before their parser reads
  it, also in `gcode:` values. Thus, a marker line or a commented-out line is an empty line, and an empty line
  in a gcode template does nothing. `tests/klipper/check_markers.py` loads test files with the real
  `configfile.py` of both (pinned in CI) and checks: a file with blocks gives the same configuration as
  without them, and "keep mine" and "use shared", done as the block tells, give the expected configuration.
  Cases: an option value, lines in `gcode:`, no active lines, a block over a section header, a file with
  `[include]` and a SAVE_CONFIG part.
- A shared line in a block is `#` + the shared line, with no space. With `# `, removing the `#` leaves a space
  at the start, and the parser adds the line to the value of the option above it. There is not always an
  error (tested). The block text says "remove the #", so the result must then be exactly the shared line.
- Marker lines start at the start of the line, also in `gcode:` values. Their indentation does not matter
  to the parser.
- When this printer has no lines at a place, the block says "Active now (this printer): no lines." and only
  "remove the # from the next lines".

Part 1 (2026-10-12): state, follow, stop-following, copy-to-shared, changes from shared.

- **Order in each run:** a backup of the config folder, then changes from shared, then a second backup with
  what printerkeep did ("Changes from shared: macros.cfg", "Needs decision: …"), then send. Thus, the
  printer's own version is always in the history before printerkeep writes a file.
- Shared files need a repository. printerkeep gets the `shared` branch at each run only when the printer
  follows a file. If the repository cannot be reached, printerkeep uses its last copy of `shared`.
- A file in `follow:` in `printerkeep.conf` (for example added in Fluidd) works the same as
  `printerkeep follow`. A file that is not in shared gets a line in the log, nothing more.
- No base (a new follow, or shared lost the base commit): the current shared version becomes the base.
  The differences on the printer are local changes.
- A followed file that is not in the config folder any more is not written again. The log says how to stop
  following it.
- `copy-to-shared` first does the normal run, so the printer has all changes from shared. It refuses a file
  that needs decision. If another printer changed shared at the same time, it tries again on the new
  version, at most three times. For a file that the printer does not follow, it replaces the shared version:
  that is what "copy to shared" means.
- Needs decision is found by the lines that start with `#### printerkeep:`, not by all `####` lines: many
  configurations use `####` in headings.
- The state file is JSON with a small parser and writer in `state.rs`, not a JSON crate (few dependencies).
- The merge works on LF line ends. The result gets the line ends of the printer's file.
- Block text for two more cases: "Shared changed lines that this printer removed." and "Shared version.
  No lines. To use it, delete the line(s) above."
- Not in part 1: the override rule (part 2), `decide`, the status of each followed file, M6, the overview.

Part 2 (2026-10-12): override lines (C6), in `overrides.rs`.

- Before the merge, each line in the base and in the shared version that has the key (`[section]` and
  option) of an override line on the printer is replaced by the printer's line. The merge then sees no
  change there. This also applies to the "no local changes" case, so printerkeep never writes the shared
  value over an override line.
- `copy-to-shared` replaces each override line with shared's line for that key. If shared has no line for
  it (the first copy, or a new option), shared gets `option: CHANGE_ME   # printerkeep: override`. A
  printer that starts to follow the file gets a clear Klipper error, never the pins of a different printer.
  Leaving the line out instead could make a printer use a default value without an error. The output lists
  each override line that was not copied.
- After `follow` writes a file with `CHANGE_ME` lines, the output tells the user to set the values.
- The mark is found in any `#` comment of the line; spaces and capitals do not matter
  (`#printerkeep:override` works).
- A clean merge whose result is the printer's file writes nothing and gives no line in the log.

Part 3 (2026-10-12): `decide` and the status of each followed file.

- `decide <file> --use-shared|--keep-mine` does the same steps as the block text tells, for all blocks
  of the file. It refuses a file with an incomplete block, and tells the user to edit the file.
- `status` shows each followed file with one status word (up to date, local changes, changes waiting,
  needs decision), or "not in the config folder" / "not in shared". It uses printerkeep's last copy of
  shared, so it uses no network. Thus, changes in shared after the last backup show only after the next one.
- Override lines do not count as local changes for "up to date".
- The log gets lines only when something happens to a followed file (and a "needs decision" line at each
  run while a file needs decision). Not a full status at each run: that is a line per file every 5 minutes.

Part 4 (2026-10-12): changes in the repository on the printer's own branch (M6), in
`repository_changes.rs`.

- When the repository refuses a send because its branch has commits that the printer does not have (for
  example, an edit in the web pages), printerkeep merges each changed file as in C4, writes the results, and
  makes a merge commit with both histories. Then it sends again. Nothing in the repository is overwritten.
- The block text names the source: "The repository changed this line too." and "Repository version.".
  `decide` and the parser check accept both.
- printerkeep never removes a file from the config folder. A file that was removed in the repository gets a
  line in the log, and the next backup has it again. A file that the printer removed is not written again.
- printerkeep never writes its state file from the repository, and never writes files that the ignore rules
  skip. `printerkeep.conf` merges like any other file.
- A followed file with any needs-decision block gets no changes from shared until the user decides.
- **Lines next to each other:** `git merge-file` marks changes on lines next to each other as a conflict. In
  configuration files that is common (two options, one under the other). printerkeep uses `--diff3` and looks
  into each conflict part again: lines that are the same at the start and the end of both sides are merged,
  and when no lines were added or removed, each line is merged alone. Only lines that both sides changed get
  a block. This applies to shared and to the repository. (`--zdiff3` does a part of this, but needs git 2.35;
  older printer systems have older git.)

Part 5 (2026-10-13): no SD writes when nothing changed, and `check_repository`.

- Found: a run with no changes wrote a log line, the lock file, `FETCH_HEAD` (each fetch), and a new time on
  each stored file (git "freshens" objects that `hash-object -w` stores again). Now:
  - the log gets a run only when something happened (a backup, a change, a send, a problem). A status that
    only repeats (a file that still needs decision, a file not in shared) is shown on the terminal only;
  - the lock file is opened without emptying it, and the data folder mode is changed only when it is wrong;
  - printerkeep hashes all files without storing them, and stores only new and changed files;
  - the repository check is one `git ls-remote` for `shared` and the printer's branch. printerkeep fetches
    a branch only when it moved.
  A test checks that a run with no changes changes no file time in `printer_data`.
- `check_repository: off | <n>m | <n>h | <n>d` (default 5m). The time of the last check is in
  `$XDG_RUNTIME_DIR/printerkeep/` (RAM). After a reboot, the first run checks. Without
  `XDG_RUNTIME_DIR`, each run checks. A run by hand (on a terminal) and the commands (`follow`,
  `copy-to-shared`, …) always check. A send that the repository refuses always leads to a merge (part 4).
- A web edit on a printer that has no new backups continues the repository's history (no merge commit).

Part 6 (2026-10-13): README.md on each printer branch (moved up: a visible README with the restore steps is
important also without shared files).

- `README.md` at the top of the printer branch: what the branch is, that printerkeep made it, how to restore
  the printer (with the URL and the printer name), and the followed files. All git hosts show a README at the
  top. Only with a repository.
- It is added to each backup, but it is not in the config folder. Its first line is a hidden marker. Restore
  and changes from the repository skip a README.md with the marker. printerkeep replaces changes to it.
- If the config folder has its own `README.md`, that file is backed up, and printerkeep adds none.
- `status` does not count it in "Files in the last backup".
- `shared` gets its own README.md (part 7: static). With more printers, `shared` is
  the main branch: the guide tells the user to make it the default branch on the git host (git cannot set
  that).

Part 7 (2026-10-13): the README of `shared`. First built as an overview with a status section per
printer; then simplified (decided with the user): the status is too complex for the need.

- The status is not in the repository. `printerkeep status` on each printer shows it. A page with all
  printers needs coordination between printers (each writes its section, races, retries, a commit on
  `shared` per status change). Revisit if users need a dashboard.
- `README.md` on `shared` is static: what the repository is, that each printer has its own branch (find it
  with the branch list of the git host), and the restore command. printerkeep writes it in the same commit
  as a `copy-to-shared`, when shared has none or when printerkeep's README is out of date (a new URL). No
  separate commit, no coordination. Without the marker line, printerkeep never changes it.
- `README.md` cannot be followed or copied to shared.
- A base does not move when the file did not change in shared (only other files changed). Else each copy
  of another file would change the state file on all printers, and make backups for nothing.
- Default branch: git cannot set it. The user README tells users with more printers to make `shared` the
  default branch on the git host.
- Low priority: a status column in the printer README's list of followed files.

Part 8 (2026-10-13): property tests (`tests/merge_properties.rs`, proptest, a development dependency only).

- Random `option: value` files; each side changes some values. Checked with the real `git merge-file`:
  - the three-way rule per line: the side that changed a line wins; lines changed on both sides to
    different values need decision, and `decide` gives exactly the expected file for both choices;
  - a file that needs decision is, for Klipper, the same configuration as the printer's version (comments
    only; the Klipper view deletes text from the first `#`, as the parser spike showed);
  - an override line never changes, also when shared has its own override line;
  - lines added at different places merge (also with only one line between them);
  - CRLF files stay CRLF.
- A test of the tests: a merge that is broken on purpose fails the first property at once.
- M1–M9 in integration tests (`tests/shared.rs`, `tests/repository.rs`): M1, M4 changes from shared with
  local changes; M2, M9 needs decision; M3 copy-to-shared; M5 status; M6 changes in the repository;
  M7 one merge after three changes in shared; M8 follow a file that is different on the printer.

M5 status: the "done when" is met in tests. Not yet tried on real printers.

### M6. Real-data tests

- Test fixtures from the four-printer setup in the problem description, with neutral names (printer 1–4).
  The fixtures test the cases in "Real differences, checked" in solution-options.md.
- **Done when:** all real cases behave as the docs say.

**Decisions made in M6 (2026-10-13):**

- Fixtures: five files from four real printers, shortened to the lines that show each case (452 lines from
  about 1700), in `tests/fixtures/four-printers/printer-1..4`. Stored with LF; the test writes CRLF where the
  real printer has CRLF. No serial IDs, host names or tokens (no `host-mcu.cfg`, `moonraker.conf`).
- `tests/real_data.rs`: line ends only (up to date, CRLF stays), an unrelated file at the same path (not
  touched), an identical file (up to date), KAMP values (stay; a shared change on the next line merges; the
  same value needs decision, or stays with an override mark), runout lines commented out in gcode (stay),
  an improvement in PRINT_END (reaches printers 2 and 3; printer 4 has it already), the same file at a
  different path (move once, then follow).
- Found with real data and fixed: both sides add the same lines, and only one side changes the line after
  them. The merge took the same lines out of the conflict, but then made a block for the next line. Now: if
  the rest of one side equals the base, the other side wins.

**Design question (found with real data), decided 2026-10-13:** `copy-to-shared` copies the whole file. Printer 4's real
improvement (the retract in PRINT_END) is in the same file as its lines for missing hardware
(`# STATUS_HEATING`, `# _RUNOUT_ON`, `# _AUTO_OFF_DISABLE` in gcode). Copying the file to shared would also
comment out those lines on printers 2 and 3. Override marks do not help: they are for `option: value` lines
only. Today the way is an edit of only the improvement in the web pages (tested). Decided: `copy-to-shared` shows the
changes that go to shared (a diff against the shared version) and the override lines that it keeps, and asks
first. Without a terminal it needs `--yes`. A begin/end override form for gcode lines stays a possibility for
later, if users need it.


### Printer names (2026-10-13)

The host name was the default printer name. That is a poor choice: image defaults (`mainsail`,
`raspberrypi`, `fluiddpi`) are common, DHCP can change the name, and the name was fixed only by
`connect`. Worst case: two printers with the same name, and the second `connect` continued the
first printer's history (it kept its own backups only as a local reference).

- The default name is `main`, the usual name of a repository's first branch. An empty `printer_name` means
  `main`. The name never comes from the system.
- `printerkeep install --printer-name <name>` (install.sh: `PRINTER_NAME`) sets the name before the first backup. A
  name that cannot be a branch stops the install before it changes anything.
- `printerkeep rename <name>` moves the local history (the branch, and the sent and fetched references) to
  the new name and sends it. The repository keeps the old branch (the output says so). It refuses a name that
  the repository has already. The "change the name only before the first send" warning is gone.
- `connect` with a branch of the same name in the repository:
  - common history: continue it (as before);
  - no common history, and the branch has `.cfg` files: stop. It is another printer, a new install of this
    printer (use `restore`), or a klipper-backup branch. printerkeep never takes over that history by itself;
  - no common history and no `.cfg` files (a new repository with a README or a license): join both histories
    with a merge commit. The README and the license are not in the config folder, so they are not in the
    backups after that.
- klipper-backup branches: see "Move from klipper-backup".
- Command line (decided 2026-10-13): the printer name is always `--printer-name <name>` (`install`,
  `restore`, and `connect` with M7), the same words as `PRINTER_NAME` and `printer_name:`. The rule:
  the main thing a command acts on is positional, and things that change how it acts are flags. Thus only
  `rename <printer name>` has it as a positional argument: the name is what it acts on. Not both forms for one
  command: two ways to write the same thing are harder to learn and to read. `install --name` and `restore
  <url> <name>` (v0.1) are gone.

### Move from klipper-backup (2026-10-13)

Built into `connect` (decided with the user; replaces the postponed migration guide and the idea of a
script):

- If the branch with the printer name has no common history with the printer and its README.md starts with
  `# klipper-backup`, printerkeep continues it. The next backup is a child of the last klipper-backup commit,
  with the files of the config folder at the top. git sees the move from `printer_data/config/` as renames
  and follows the files (tested on a public klipper-backup repository: `git log --follow printer.cfg` finds
  all 53 versions). The printer's own backups (usually only the one from `install`) stay on the printer as a
  kept reference.
- Continue, not convert: no history rewrite, no force push. Every klipper-backup commit stays as it is.
- First tried as a merge commit (the "join" for a new repository with a README). That hid the older history
  from `git log --follow`, because the first parent already had the files at the top.
- printerkeep does not check whether klipper-backup is still installed (that would mean following how users
  install and remove it). It prints a notice: stop klipper-backup, or it sends its backups to the same branch.
- With the default name `main`, the usual klipper-backup repository (one `main` branch) needs no options.
  One klipper-backup branch per printer: install each printer with `--printer-name <branch>`.
- Earlier notes (from the postponed guide): klipper-backup's clone `~/config_backup` has its token in the
  remote URL; a public klipper-backup repository had 825 commits, 86 of them changed the config folder.

### M7. Connect to a repository, and tell the user about problems

Found in a test on a real printer (2026-10-13). The user removed printerkeep's local data, installed again,
and wanted to run `connect` (and to test the klipper-backup conversion). What happened:

- `uninstall` keeps `printerkeep.conf`, so the new install was still connected (`repository:` and
  `printer_name:`, from an old test build). Its first backup tried to send at once. The install output did
  not show that.
- The printer name was `bluev2.home.ressukka.net` (the old host name default). The klipper-backup branch is
  `bluev2`, so the conversion could not happen.
- The new local history had no common backup with the repository's branch. Each run (every 5 minutes) the
  send was refused, and the log said "The repository has changes that are not on this printer". That message
  was wrong for the case and had no fix in it; the user tried `change-token`. Each run also wrote the log
  and `send-problem`.

The cause behind it: printerkeep expects the user to know hidden state (the name and the repository in
`printerkeep.conf`) and to select the right command among `connect`, `restore` and `rename`. The user
thinks "which printer in the repository is this?"; printerkeep thinks "is there a branch with my exact name?".

Terms: to **continue** a history = the printer's branch is set to the repository's last backup, and the next
backup is made on top of it from the files in the config folder. Backups that were only on the printer are
not deleted and not sent: they are **set aside** (`refs/printerkeep/kept/<name>-<time>`). Continue never
changes files.

1. **`connect` and `restore` ask which printer this is.**
   - Both first show what they found: the printer name (and where it comes from: `printerkeep.conf` or the
     default `main`), and the printer branches in the repository.
   - A branch with this printer's name and a different history: "Continue that history on this printer?"
     Yes: continue (a klipper-backup branch the same way: the files move to the top). No: select another
     name. The same question for every branch.
   - No branch with this printer's name, but other printers in the repository: "Is this printer one of
     them? Type its name, or press Enter for a new branch." A name: rename the printer to it, then continue.
     This rename must work although the repository has the name (`rename` refuses that today, as a guard).
     Users expect `rename` to rename the branch in the repository; it does not (it makes a new branch and
     leaves the old one). The README explains this since 2026-10-13. With M7, "make this printer continue
     that branch" is a question of `connect`, not a manual edit of `printerkeep.conf`.
   - A new branch, and the local history has more than the install's first backup: "Send the backups made
     on this printer, or start the history in the repository from now?" From now: one backup of the current
     files; the local history is set aside.
   - **`--printer-name` for both commands:** `connect <url> --printer-name <name>`, the same as
     `install` and `restore` (a user expected add-repository to take a printer name). With a name, this
     printer takes that name: if the repository has that branch, it continues it (after the question); if
     not, it starts a new branch with that name. This replaces the manual edit of `printer_name:`.
   - Without a terminal, printerkeep never guesses: it stops, lists what it found, and names the options
     (`--printer-name`, `--continue`, `--from-now`).
   - `restore` = the same, plus it writes the files of the backup (as now: never removes files, `--overwrite`
     for different files). After a restore, the install's first backup (of the default configuration) is
     set aside like any other local backup.
2. **The backup run continues by itself** when the repository's branch has no common backup with the local
   history and `printerkeep.conf` already names this repository and this printer (it was connected before).
   One log line says so. Else it stops sending, with the message from point 4.
3. **`install` shows the saved state:** "Connected to <url> as <name> (from printerkeep.conf)", or "Printer
   name: main (the default)".
4. **Messages:** each problem says what happened, why, and the exact command or step that fixes it. Review
   all user-facing errors for this. For example: "The branch <name> in the repository has backups with no
   common history with this printer's backups. printerkeep sends nothing, so nothing in the repository is
   overwritten. If this printer was set up again: printerkeep connect <url> (and answer yes). If it
   is a different printer: printerkeep rename <name>."
5. **Log changes, not states:** a problem is logged when it starts, when it changes and when it is solved
   ("Sending works again: sent 3 backups"). Runs in between are quiet, and `send-problem` is written only
   when its text changes. `printerkeep status` always shows the current problem.
6. ~~The Klipper console~~: dropped (2026-10-13, decided with the user). There is no standard way for a
   program outside Moonraker to publish a notice that Fluidd and Mainsail show (checked in their source):
   - announcements and Moonraker warnings can be added only inside Moonraker (`add_internal_announcement`,
     `add_warning` are Python functions; the public endpoints only list, dismiss and subscribe to feeds);
   - agent events (`connection.send_event`) are a public API, but neither Fluidd nor Mainsail shows them;
   - `[notifier]` (Apprise) needs the user's setup, and a program can start it only through a G-code macro;
   - the Klipper console (`printer.gcode.script`, `RESPOND`/`M118`) needs `[respond]`, scrolls away, and
     means that a backup tool sends G-code to the printer.
   The user gets problems through the log (Fluidd and Mainsail show it) and `printerkeep status`. See
   "Notifications in Fluidd and Mainsail" in the future possibilities of problem.md.

Tests: the `bluev2` case (local data removed, `printerkeep.conf` kept: the run continues by itself, logs one
line, then sends); install as `main`, connect as `foobar` (only `foobar` locally, the local backups set aside,
the repository's `main` not read or changed); the klipper-backup per-printer branch selected by name; "from
now"; no terminal (stops with the options); each problem logged once.

**Done when:** on a real printer, a reinstall and a reconnect, a klipper-backup conversion with a branch per
printer, and a restore after a new install all work with `connect` or `restore` and its questions,
without hidden state. A problem shows once in the log, with its fix, and in `printerkeep status`.

Part 1 (2026-10-13): points 1 and 3 (`app::Ask`, `app::Connect`, `repository::set_aside` and
`move_local`).

- The questions are a trait (`Ask`): a yes/no and a text question; `None` without a terminal. The CLI asks
  on the terminal; tests (`tests/connect.rs`) give fixed answers to the library.
- When printerkeep asks which printer this is: when the repository has *other* printers. With the default
  name `main` it always asks then (main is not the user's choice). With a chosen name (`printerkeep.conf`
  or `--printer-name`) it asks only when the repository has no branch with that name. Without a terminal,
  a chosen name is used (not a guess); the default `main` stops with the options.
- Found while testing: a printer with the default name `main`, and a repository that has `main` (another
  printer's, or klipper-backup's) and other printers. The first version went straight to "continue main?"
  and never asked which printer this is. Hence the rule above.
- A branch with backups that are not this printer's: printerkeep asks before it continues, also for a
  klipper-backup branch (before M7 it continued klipper-backup without a question). `--continue` in scripts.
  A branch without configuration (a new repository's README) is still joined without a question.
- "From now" is asked only when the local history has more than one backup (more than the install's).
- `restore` moves the local history (for example the install's first backup under `main`) to the restored
  name, so it is set aside, not left on an unused branch.
- Not done as planned, and dropped (decided with the user): the list of printers does not mark
  klipper-backup branches, and the "continue?" question is the same for every branch. The user knows which
  branch is the printer's when it is not clear. After a klipper-backup branch is continued, the output still
  says to stop klipper-backup, and the commit says "Continued the klipper-backup history".

Part 2 (2026-10-13): points 2, 4 and 5.

- The backup run continues the repository's history by itself when the send is refused, the histories
  have no common backup, and `printerkeep.conf` names this printer (`connect` and `restore` always
  write it). One line says so, with the reference where the local backups were set aside. Tested with the
  real case: local data removed, `printerkeep.conf` kept.
- All problems of a run (send, repository check, changes from shared) are one text in `send-problem`. The
  log gets them only when the text changes, and one line "The problem of the last runs is solved." when it
  is gone. The terminal shows the problems at each run. `status` shows the last problem also when all
  backups are sent.
- Found in a manual check: printerkeep took the last line of git's error output ("and the repository
  exists."). Now it takes the first `fatal:`/`error:` line. When the repository cannot be reached, the
  check and the send failed with the same cause: only the send line stays.
- Messages with a fix now: connection errors (check the network and the URL in `printerkeep.conf`), a
  refused token (access, "Contents: Read and write", expiry), restore with no backups or an unknown printer
  name, and the send race (tries again at the next run).

Backlog: log rotation. printerkeep rotates at 1 MB (`printerkeep.log` → `printerkeep.log.1`). Decide if
that is enough, or if it should work like Moonraker's own logs.

### Restore from the history (2026-10-14, for v1)

The main case: you edit `printer.cfg`, change your mind, and want an earlier version. One `restore` command
does all restores:

- `printerkeep restore [<file>] [--at <time>]` works on this printer's history (with a repository, the
  history includes its changes). `--repository <url>` and `--printer-name` give another place to restore
  from: another printer, or another repository. On a printer that isn't connected, `--repository` is the
  restore after a new install (as before): it connects and continues the printer's history.
- **No pause of the backups.** Restore first runs a backup, so the files now (an edit) are in the history.
  Then it writes the files, and makes a backup with the message `Restored <file> from the backup of <time>`.
  The history says what happened; another restore undoes it, and the output gives that command.
  `--overwrite` is gone: the files from before a restore are always in a backup.
- **Picking a backup:** `--at` uses git to read the time (`git rev-parse --before=`), so no date crate is
  needed. printerkeep takes the newest backup at or before that time. When there are 2 or more backups on
  that day and printerkeep runs on a terminal, it lists them, and Enter takes that one. Without `--at`, a
  terminal lists the earlier versions; a script gets an error with an example command.
- `.printerkeep-state.json` comes back only with a new install. `printerkeep.conf` comes back only with a new
  install, or when the user names it in this printer's own history.
- **Done when:** integration tests (`tests/restore.rs`) show: one file at a time without a question, the
  list on a terminal, a script takes the state at that time, a date alone, all files with later files kept,
  a deleted file, the same content (no backup), unreadable times, a time before the first backup, a file from
  another printer and from another repository (the connection doesn't change), the state file, and the undo
  command works. The tests for a restore after a new install use `--repository`.

## Tests

| Kind | What |
|---|---|
| Unit | settings parser, ignore rules, commit messages, needs-decision rendering, override matching |
| Property (`proptest`) | merge rules (see M5) |
| Integration | real `git` in temporary folders, local bare repositories as the "repository" |
| Klipper compatibility | Klipper's parser loads files with markers (M5 spike) |
| Cross targets | the release binaries run under qemu-user in CI |
| Manual | install, timer, update and restore on a real printer before each release |

## Open questions and where they are solved

| Question | Milestone |
|---|---|
| Notifications in Fluidd/Mainsail through Moonraker | no standard way for an outside program (checked in M7); a future possibility |
| Klipper parser and markers | M5 spike (done, see M5) |
| Default ignore rules | M1 |
| `executable` type details | M3 spike (done, see M3) |
