# Implementation plan

Status: draft (2026-10-11). Based on [problem.md](problem.md), [solution-options.md](solution-options.md),
[naming.md](naming.md) and [language.md](language.md).

## Releases

| Release | Content | Goal |
|---|---|---|
| **0.1** | One printer: local history, repository, restore, installer, update_manager. | A replacement for klipper-backup. The authors move their own printers to printerkeep. |
| **0.2** | More printers: follow, changes from shared, needs decision, override, copy to shared, overview. Changes in the repository get to the printer (M6). | The full design. |

0.1 already uses the 0.2 repository format (one branch per printer, files at the top). Thus, 0.2 needs no
migration.

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
  main.rs        command line (clap): backup, status, restore, add-repository, follow, stop-following,
                 copy-to-shared, decide
  settings.rs    printerkeep.conf: strict Moonraker-style subset (C3)
  paths.rs       config folder, printer_data, git data, log file
  git.rs         thin wrapper around the git command (GIT_DIR, GIT_INDEX_FILE, GIT_SSH_COMMAND)
  snapshot.rs    read the config folder into a git tree: ignore rules, no symlinks, line endings (C2)
  backup.rs      commit only on change, commit messages, push (C2)
  repository.rs  add-repository, keys, existing branches, restore (C8)
  log.rs         printer_data/logs/printerkeep.log and the status text (C7)
  -- 0.2 --
  state.rs       .printerkeep-state.json: base per followed file (C3)
  merge.rs       three-way merge per file, needs-decision block, override rule (C4–C6)
  shared.rs      follow, stop-following, copy-to-shared, changes from shared
  overview.rs    README.md on the shared branch
```

### Git commands that printerkeep uses

All are old, stable commands: `hash-object -w`, `update-index` (with a separate `GIT_INDEX_FILE`),
`write-tree`, `commit-tree`, `update-ref`, `rev-parse`, `cat-file`, `ls-tree`, `fetch`, `push`,
`merge-file`. printerkeep never runs `git merge`, `git pull` or `git checkout` in the config folder (C1).

### Merge engine (0.2)

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
  `printerkeep-linux-{aarch64,armv7,armv6,x86_64}` and a SHA-256 checksum file. Pre-releases for tests.
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
  that this printer does not have"). 0.2 merges them (M6).
- `restore <url> [<printer name>]`: fetch the branch and write its files into the config folder. If the
  folder has files that are different from the branch, printerkeep stops and lists them, unless the user
  adds `--overwrite`. After a restore, printerkeep tells the user to restart Moonraker and Klipper.
- Printer name: `printer_name:` in the settings file, default the host name. Not `shared`.
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
  check the SHA-256 checksum (`BINARY_SHA256` or the checksum file), `VERSION` and `PRERELEASE` variables,
  then run `printerkeep install`. The README shows three ways to install: the script in one command, the
  script read and checked against the release page, and no script.
- **Done when:** on a real printer, one command installs printerkeep, the timer makes backups, and Fluidd's
  update panel updates printerkeep from one pre-release to the next.

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
    The `PRINTERKEEP_BACKUP` macro is not in 0.1 (Klipper has no standard way to run a command).
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
- `install` makes the first backup, after it changed `moonraker.conf`.

### M4. Release 0.1

- User README: install, first backup, add a repository, restore.
- People who move from klipper-backup start again with printerkeep. A guide that keeps the klipper-backup
  history comes after 0.1 (see "After 0.1: move from klipper-backup").
- **Done when:** the authors' printers run 0.1.

**Decisions made in M4 (2026-10-12):**

- `status` shows why the last try to send did not work (for example an expired token), because `status`
  uses no network. `backup` writes it to `printer_data/printerkeep/send-problem`, and removes it after a send
  that works.
- No README in the printer branch, as klipper-backup has: a printer branch is the config folder, and restore
  writes all its files. The overview on `shared` (C7, 0.2) is the place for it.

### M5. More printers (0.2)

- Spike first: a CI job that loads test configurations with Klipper's own parser (`klippy/configfile.py`, a
  pinned Klipper checkout, Python 3). It proves that `#### printerkeep` blocks, commented-out lines and
  override comments have no effect, also in multi-line `gcode:` values (open question 2).
- State file, `follow`, `stop-following`, the `shared` branch.
- Merge engine (above), changes from shared (C4), needs decision and its detection (C5), `decide`.
- Override rule in merges and in `copy-to-shared` (C6). `copy-to-shared` shows the changes it sends and the
  override lines it keeps back.
- Changes in the repository on the printer's own branch (M6): the same per-file merge, with the printer's
  branch in the repository as the other side.
- Overview: `README.md` on `shared`, one section per printer. Pushes at the same time: fetch, rebase, retry.
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

### M6. Real-data tests (0.2)

- Test fixtures from the four-printer setup in the problem description, with neutral names (printer 1–4).
  The fixtures test the cases in "Real differences, checked" in solution-options.md.
- **Done when:** all real cases behave as the docs say.

### After 0.1: move from klipper-backup

Postponed (2026-10-12): the first guide was not good enough for 0.1. A draft,
`docs/migrate-from-klipper-backup.md`, is in the version history. What we learned:

- The conversion is plain git on the printer: `git subtree split --prefix=printer_data/config -b <printer
  name>` in klipper-backup's clone `~/config_backup`, then a push. That clone has klipper-backup's login in
  its remote URL. `git filter-branch --subdirectory-filter` makes the same commits (same hashes, tested), for
  systems without `git subtree`.
- printerkeep continues the history of an existing printer branch (M2). Backups that printerkeep made before
  stay as `refs/printerkeep/kept/*` (tested on a copy of a real repository).
- The usual klipper-backup repository has one `main` branch. A public one had 825 commits, 86 after the
  split, because klipper-backup also commits with no changes.
- One branch per printer, with the printer name, collides with the new branch. Copy the old branch to
  `klipper-backup/<name>`, then replace it with `--force-with-lease` (tested).
- Order: stop klipper-backup, convert, remove klipper-backup, install printerkeep. `~/config_backup` is
  removed only after the push.
- The branch must have the printer name before the first backup that printerkeep sends. A changed
  `printer_name` after backups on the printer leaves those backups on a branch that printerkeep no longer
  uses.
- We expect that most users start again and do not keep the history.
- After `add-repository`, the backup that `printerkeep install` made stays on the printer as a kept ref, with
  a message, even when it has the same files as the repository. Possible improvement: continue the
  repository's history with no kept ref in that case.

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
| Notifications in Fluidd/Mainsail through Moonraker | after 0.2; the log file is enough until then |
| Klipper parser and markers | M5 spike (done, see M5) |
| Default ignore rules | M1 |
| `executable` type details | M3 spike (done, see M3) |
