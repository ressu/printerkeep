# Naming

Status: **decided** (2026-10-11): vocabulary, project name and artifact names.

## Writing rule

User-facing text follows the `technical-writing` skill: plain, natural English, the main point first, short
sentences, and things stated positively. This covers CLI output, questions, error messages, log lines,
in-file markers, the docs, and the README files that printerkeep writes into the repository.

This file decides the terms; the skill decides the style. When they disagree, this file wins.

- **One word, one meaning.** Use the terms below for these concepts, every time. A synonym, even for
  variety, makes the reader wonder if it means something else.
- **Words to avoid** are listed for each term. They are wrong or unclear for this audience.

Until 2026-10-13 the rule was a loose form of ASD-STE100 (Simplified Technical English). It gave stiff,
sometimes backwards English, so the skill replaces it; the skill keeps the useful parts of ASD-STE100.

## Vocabulary

### Every user (single printer)

| Term | Meaning | Avoid |
|---|---|---|
| **printer** | One machine running Klipper, with its own config folder. | machine, host, node |
| **printer name** | The name that identifies a printer in the repository (its branch name). | id, hostname |
| **config folder** | The printer's Klipper config directory (Moonraker's config root, normally `~/printer_data/config`). The tool's scope is exactly this folder. | config dir, printer_data |
| **backup** (noun and verb) | Save the current files of the config folder to the history. A backup is made only when a file changed. | snapshot, commit, sync |
| **history** | All backups of a printer. | log, commits |
| **restore** | Put a printer's files back from the repository, for example after a new install. | recover, clone |
| **repository** | The git repository where backups are stored (for example on GitHub). | remote, origin, repo (in messages) |
| **send** | Copy new backups from the printer to the repository. printerkeep never overwrites the repository. | push, upload, sync |
| **settings file** | The one file in the config folder where the user changes the tool's settings. | manifest, config file (ambiguous with Klipper config) |

### Users with more printers

| Term | Meaning | Avoid |
|---|---|---|
| **shared** (noun and adjective) | The set of files that printers can follow. A file in shared is a **shared file**; its content there is the **shared version**. Printers use only the shared files they follow. | pool, library, common, "share" as a verb |
| **follow** | A printer that follows a shared file gets changes to it from shared. Stopping is **stop following**. | subscribe, track, use, link |
| **copy to shared** | Make the shared version of a file equal to this printer's version. Used to add a file to shared and to send an improvement to other printers. | share, promote, publish, push, adopt, upload |
| **local changes** | The differences between this printer's file and the shared version. They stay on this printer unless the user copies the file to shared. | deviation, drift, diff, patch |
| **override** | A line marked so that it never leaves this printer and is never changed by shared, for example pin lines. Marked with the inline comment `# printerkeep: override`. | pin (means GPIO pins in Klipper), fix, force, local line |
| **needs decision** | A change from shared and a local change touch the same lines. The tool cannot choose; the user must. The file shows both, with the shared lines commented out. | conflict, held, merge error |
| **changes from shared** | Changes to followed files that come from shared. | update (reserved for updates of the tool itself), upstream, incoming |

### Status words

Each followed file has exactly one status:

| Status | Meaning |
|---|---|
| **up to date** | Same as the shared version. |
| **local changes** | Has all changes from shared, plus local changes. |
| **changes waiting** | Changes from shared are not applied yet (for example, the printer was offline). |
| **needs decision** | See above. The user must edit the file. |

### Internal only (code and developer docs)

The user never sees these: **base** (the shared version a printer last took in), **three-way merge**,
**tree**, **snapshot**. **Branch** appears only where users look at the repository on the git host: each
printer has its own branch, and shared is the branch named `shared`.

## Phrasing examples

| Situation | Write | Don't write |
|---|---|---|
| CLI, adding a file | `Copied macros.cfg to shared. Other printers can now follow it.` | `Shared macros.cfg!` |
| Status | `macros.cfg: needs decision. Open the file and look for the #### lines.` | `macros.cfg: merge conflict (held)` |
| Status | `printer-2 follows 3 shared files. 1 needs decision.` | `printer-2 is drifting on 1 file` |
| No change | `No files changed. No backup made.` | `Nothing to commit, working tree clean` |

## Project name: printerkeep

A *keep* is the strongest, safest tower of a castle: the place where valuable things are kept. **printerkeep**
keeps printer configuration safe and keeps printers in step.

Why this name:

- **Says what it does.** "Keep" is also the core verb of the vocabulary (backup, history).
- **Firmware-neutral.** It names the printer, not Klipper, so it fits Kalico and other Klipper derivatives
  equally well.
- **Distinctive.** On 2026-10-11 it was free on crates.io and PyPI and had zero GitHub repositories with the
  name. That avoids klipper-backup's problem of a name made of two generic words.

Candidates considered and rejected:

| Candidate | Reason |
|---|---|
| Single nautical words (bosun, logbook, stow, keel, …) | Almost all taken on crates.io or PyPI; several clash with known tools (GNU Stow, Bosun). |
| homebase | Confused with Home Assistant, homelab, homepage. |
| drybox, raft, galley, heirloom | Very hard to search; raft is a computer-science term; galley and heirloom are crowded on GitHub. |
| layerkeep | Fits a gcode store better than configuration. |
| firmwarden, stepherd, stepward | Not chosen. |
| quoin | Liked (short, distinct), but taken on both registries; also a company name and a masonry term. |
| typecase, bedkeeper | Available, not exciting enough. |
| seedbed / seedbedd | Runner-up: a strong character name (seed repository + print bed). Plain "seedbed" is hard to search; the Fluidd-style "seedbedd" fixes that but needs a tagline to explain. |
| cfgkeep, configkeep | Too close to existing "ConfigKeeper" projects; "cfg" is jargon for novices. |

## Artifact names

Decided 2026-10-11. Version 1 supports one Klipper instance per host (see [language.md](language.md) for
platforms). The data paths are per `printer_data` folder, so more instances per host are possible later.

### Program and distribution

| Artifact | Name |
|---|---|
| GitHub repository, crate, binary | `printerkeep`. Later crates in a workspace use `printerkeep-*`. |
| Release assets | `printerkeep-linux-aarch64`, `printerkeep-linux-armv7`, `printerkeep-linux-armv6`, `printerkeep-linux-x86_64`. The installer selects one from `uname -m`. |
| Program folder | `~/printerkeep/`, with the binary (named as its release asset, for example `printerkeep-linux-aarch64`) and `release_info.json` (update_manager `path`). `~/.local/bin/printerkeep` is a link to the binary. |
| Installer | `install.sh`, a release asset. It downloads and checks the program, then runs `printerkeep install`. |
| update_manager entry | `[update_manager printerkeep]` with `type: executable` |
| systemd units | systemd user units in `~/.config/systemd/user/`: `printerkeep.service` (`Type=oneshot`, runs `printerkeep backup`) and `printerkeep.timer`. layerlock connects with `ExecCondition=` in the service. |
| Klipper macro (optional, not yet) | `PRINTERKEEP_BACKUP`. For now, the manual start is `printerkeep backup`. |

### Files on the printer

| File | Name and place | Notes |
|---|---|---|
| Settings file | `printerkeep.conf` in the config folder | Moonraker style (see below). Never `.cfg`: `[include *.cfg]` would make Klipper load it. |
| State | `.printerkeep-state.json` in the config folder | printerkeep writes it. Backups include it, so it is on the printer's branch and comes back with a restore. Fluidd hides dotfiles. |
| Log and status | `printer_data/logs/printerkeep.log` | Fluidd and Mainsail show the logs folder. Each run writes the status of each followed file and, after `add-repository`, the public key. Not in the config folder, so it never causes a backup. |
| Git data | `printer_data/printerkeep/repo.git` | Outside the config folder (C1). |

### Settings file format

The settings file uses a strict subset of the Moonraker format: `[section]`, `key: value`, values that
continue on indented lines, and full-line `#` comments. Lists are indented lines, as in Moonraker's
`persistent_files:`. This subset is small. printerkeep does not need a full INI parser.

```
# printerkeep.conf
[printerkeep]
printer_name: printer-1
repository: git@github.com:me/printers.git
follow:
  macros/macros.cfg
  KAMP_Settings.cfg
ignore:
  timelapse/*
check_repository: 1h
```

`printer_name` is optional. The default is `main` (decided 2026-10-13: never the host name, which is often an
image default such as `mainsail` or `raspberrypi`, or set by DHCP). A user with one printer and no repository does not
need this file. When the user runs `add-repository`,
printerkeep writes it.

### Commands

| Command | Does |
|---|---|
| `printerkeep backup` | Makes a backup if a file changed, and applies changes from shared. The timer runs this. |
| `printerkeep status` | Shows the status of the printer and of each followed file. |
| `printerkeep restore <url> [--printer-name <printer name>]` | Restores a printer from the repository. `--printer-name` selects the printer when the repository has several. |
| `printerkeep add-repository <url> [--printer-name <printer name>] [--continue] [--from-now]` | Connects the printer to a repository. Shows the printers in it and asks which one this printer is; continues that printer's branch (asks first if its backups are not from this printer) or starts a new one. Without a terminal, the options give the answers. With an `https://` URL it uses the stored token, or asks for a personal access token; with an SSH URL it makes an SSH key and shows the public key. |
| `printerkeep change-token` | Replaces the token of the repository, for example when it expired. The new token replaces the old one only if the repository accepts it. |
| `printerkeep install [--printer-name <printer name>]` | Installs printerkeep for this user, or updates the installation. No root access. `--printer-name` sets the printer name. |
| `printerkeep uninstall` | Removes what `install` made. Keeps the history, the login and `printerkeep.conf`. |
| `printerkeep follow <file>` | Starts to follow a shared file. |
| `printerkeep stop-following <file>` | Stops following a shared file. The file stays on the printer. |
| `printerkeep rename <printer name>` | Gives the printer a new name. The history moves to the new branch; the repository keeps the old branch. |
| `printerkeep copy-to-shared <file> [--yes]` | Makes the shared version equal to this printer's version, except override lines. Shows the changes and asks first; `--yes` does not ask. |
| `printerkeep decide <file> --use-shared\|--keep-mine` | Finishes a needs-decision file without editing it. |

Commands are verbs. `add-repository` replaces the earlier `repository`.

### In the repository

| Item | Name |
|---|---|
| Printer branch | The printer name. Default: `main`. A printer cannot be named `shared`. |
| Shared branch | `shared` |
| Printer README | `README.md` at the top of each printer branch: what the branch is and how to restore the printer. Not in the config folder; its first line is the marker `<!-- printerkeep: … -->`. |
| README of shared | `README.md` on the `shared` branch: what the repository is, how to restore a printer. Static; printerkeep writes it with a copy to shared when it is missing or out of date (a new URL). Without its marker line, printerkeep never changes it. |

### Markers in configuration files

| Marker | Text |
|---|---|
| Override | `# printerkeep: override` (inline, at the end of the line) |
| Needs decision | A block that starts with `#### printerkeep: needs decision` and ends with `#### printerkeep: end`. Every line in the block starts with `####` or is a configuration line. The exact help text in the block can change; the start and end lines cannot. |
