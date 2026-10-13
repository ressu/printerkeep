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

Decided 2026-10-13: `major.minor`, tagged `v<major>.<minor>` (for example `v0.2`). A bug fix is the next
minor release. No patch numbers and no pre-releases: printerkeep is a program, not a library, and
update_manager only checks if the installed version is the same as the release title. `Cargo.toml` needs
three numbers, so it has `<major>.<minor>.0`; printerkeep shows `<major>.<minor>`, and the release workflow
refuses a tag that does not match.

## Project setup

- **License:** MIT (the same as layerlock).
- **Rust:** edition 2024. `rust-toolchain.toml` pins the toolchain for every contributor. An optional
  `flake.nix` gives a Nix devShell with the toolchain, `git`, `cargo-zigbuild`/zig, Python 3 (for the Klipper
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
    connection.rs       add-repository, change-token, restore, rename
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
- Release workflow on a tag: build the four musl targets with `cargo-zigbuild`, publish
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

- `add-repository <url>`, two kinds of authentication (the URL selects):
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
- `restore <url> [<printer name>]`: fetch the branch and write its files into the config folder. If the
  folder has files that are different from the branch, printerkeep stops and lists them, unless the user
  adds `--overwrite`. After a restore, printerkeep tells the user to restart Moonraker and Klipper.
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
  - `add-repository` uses the stored token. It asks for a token only if there is none, or if the repository
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
- `add-repository` and `restore` write `printer_name` and `repository` to printerkeep.conf, so a new host
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

- User README: install, first backup, add a repository, restore.
- People who move from klipper-backup use the same repository: `add-repository` continues the
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
`add-repository`. Worst case: two printers with the same name, and the second `add-repository` continued the
first printer's history (it kept its own backups only as a local reference).

- The default name is `main`, the usual name of a repository's first branch. An empty `printer_name` means
  `main`. The name never comes from the system.
- `printerkeep install --name <name>` (install.sh: `PRINTER_NAME`) sets the name before the first backup. A
  name that cannot be a branch stops the install before it changes anything.
- `printerkeep rename <name>` moves the local history (the branch, and the sent and fetched references) to
  the new name and sends it. The repository keeps the old branch (the output says so). It refuses a name that
  the repository has already. The "change the name only before the first send" warning is gone.
- `add-repository` with a branch of the same name in the repository:
  - common history: continue it (as before);
  - no common history, and the branch has `.cfg` files: stop. It is another printer, a new install of this
    printer (use `restore`), or a klipper-backup branch. printerkeep never takes over that history by itself;
  - no common history and no `.cfg` files (a new repository with a README or a license): join both histories
    with a merge commit. The README and the license are not in the config folder, so they are not in the
    backups after that.
- klipper-backup branches: see "Move from klipper-backup".

### Move from klipper-backup (2026-10-13)

Built into `add-repository` (decided with the user; replaces the postponed migration guide and the idea of a
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
  One klipper-backup branch per printer: install each printer with `--name <branch>`.
- Earlier notes (from the postponed guide): klipper-backup's clone `~/config_backup` has its token in the
  remote URL; a public klipper-backup repository had 825 commits, 86 of them changed the config folder.

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
| Notifications in Fluidd/Mainsail through Moonraker | later; the log file is enough until then |
| Klipper parser and markers | M5 spike (done, see M5) |
| Default ignore rules | M1 |
| `executable` type details | M3 spike (done, see M3) |
