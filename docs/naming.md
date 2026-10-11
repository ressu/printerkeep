# Naming

Status: **vocabulary and project name decided** (2026-10-11). Artifact names come later (see the end of this file).

## Writing rule

User-facing text loosely follows [ASD-STE100](https://www.asd-ste100.org/) (Simplified Technical English).
This covers CLI output, status files, in-file markers, notifications, docs and the overview page.

- **One word, one meaning.** Use the terms below for these concepts and nothing else. No synonyms, even for
  variety.
- **Simple verbs, short sentences.** Say what happened and what the user must do.
- **Words to avoid** are listed per term. They are wrong or ambiguous for this audience.

## Vocabulary

### Every user (single printer)

| Term | Meaning | Avoid |
|---|---|---|
| **printer** | One machine running Klipper, with its own config folder. | machine, host, node |
| **printer name** | The name that identifies a printer in the repository (its branch name). | id, hostname |
| **config folder** | The printer's Klipper config directory (Moonraker's config root, normally `~/printer_data/config`). The tool's scope is exactly this folder. | config dir, printer_data |
| **backup** (noun and verb) | Save the current files of the config folder to the history. A backup is made only when a file changed. | snapshot, commit, sync |
| **history** | All backups of a printer. | log, commits |
| **restore** | Put a printer's files back from the repository, e.g. after a new install. | recover, clone |
| **repository** | The git repository where backups are stored (e.g. on GitHub). | remote, origin, repo (in messages) |
| **settings file** | The one file in the config folder where the user changes the tool's settings. | manifest, config file (ambiguous with Klipper config) |

### Users with more printers

| Term | Meaning | Avoid |
|---|---|---|
| **shared** (noun and adjective) | The set of files that printers can follow. A file in shared is a **shared file**; its content there is the **shared version**. Printers use only the shared files they follow. | pool, library, common, "share" as a verb |
| **follow** | A printer that follows a shared file gets changes to it from shared. Stopping is **stop following**. | subscribe, track, use, link |
| **copy to shared** | Make the shared version of a file equal to this printer's version. Used to add a file to shared and to send an improvement to other printers. | share, promote, publish, push, adopt, upload |
| **local changes** | The differences between this printer's file and the shared version. They stay on this printer unless the user copies the file to shared. | deviation, drift, diff, patch |
| **override** | A line marked so that it never leaves this printer and is never changed by shared, e.g. pin lines. Marked with the inline comment `# printerkeep: override`. | pin (means GPIO pins in Klipper), fix, force, local line |
| **needs decision** | A change from shared and a local change touch the same lines. The tool cannot choose; the user must. The file shows both, with the shared lines commented out. | conflict, held, merge error |
| **changes from shared** | Changes to followed files that come from shared. | update (reserved for updates of the tool itself), upstream, incoming |
| **overview** | The page in the repository that shows all printers and the state of their followed files. | dashboard, hub, report |

### Status words

Each followed file has exactly one status:

| Status | Meaning |
|---|---|
| **up to date** | Same as the shared version. |
| **local changes** | Has all changes from shared, plus local changes. |
| **changes waiting** | Changes from shared are not applied yet (e.g. the printer was offline). |
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
| Overview | `printer-2 follows 3 shared files. 1 needs decision.` | `printer-2 is drifting on 1 file` |
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

## Open naming items

These depend on the language decision and on file-format choices. See [solution-options.md](solution-options.md).

- Package and binary names. The binary is typed often, so keep it short.
- Settings, state and status file names and extensions. Files in the config folder must not end in `.cfg`,
  because `[include *.cfg]` globs would make Klipper load them.
- Command names. The docs use these provisional names: `backup` (it also applies changes from shared),
  `follow`, `stop-following`, `copy-to-shared`, `decide`, `restore`, `status`, `repository`.
- The final text of the `#### printerkeep: needs decision` marker block. The `printerkeep:` token is decided.
- systemd unit names, the Moonraker `[update_manager …]` entry name, an optional Klipper macro name.
