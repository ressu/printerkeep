# Problem definition

Status: agreed. This document says *what* printerkeep must do. It does not say *how*. We check every
solution against it. Terms follow the vocabulary in [naming.md](naming.md).

## Background

Most people back up a Klipper printer's configuration to git with
[klipper-backup](https://github.com/Staubgeborener/klipper-backup). It has two problems:

- It makes a commit on every run, also when no file changed.
- It works in one direction only: printer to repository. A change in the repository does not get to the
  printer.

People with more printers have a bigger problem. Their printers use some of the same configuration (macros,
toolhead files, feature files), but the printers are not identical. A common workaround is one git branch per
printer. With that workaround, a good change on one printer does not get to the other printers. It is also
difficult to see how the printers are different.

People change the configuration in two places: on the printer (Fluidd, Mainsail, `SAVE_CONFIG`) and in the
repository. Neither place can be the only source of truth.

printerkeep is for Klipper and for firmware derived from it, such as Kalico. In this document, "Klipper"
includes these.

## Design principles

1. **One printer first.** A backup of one printer must be clean and simple. A user with one printer never
   sees multi-printer terms (shared, follow, copy to shared, override).
2. **Defaults for beginners, options for experts.** The target user follows KIAUH-style guides and does not
   necessarily know git.
3. **No setup for local history.** After installation, printerkeep keeps history on the printer. A
   repository is optional. The user can add it later.
4. **Never control how users write their configuration.** No required structure, fragments or override
   conventions. If this needs complexity, the complexity goes in the code, not in the user's work.
5. **No Linux desktop, no local clone, few commands.** In order of preference: (a) the user does nothing,
   (b) the user uses a tool they already have, where this is easy to support (the Fluidd or Mainsail editor,
   the git host's web pages), (c) the user types a simple command over SSH on the printer. A large Fluidd or
   Mainsail integration is not a requirement. Every user action must be possible with only the printer and a
   browser or an SSH client.
6. **Simplicity.** We do not build every wish. A feature that adds complexity must show the need behind it. If
   a simpler way meets the need, we use the simpler way. printerkeep must make sense to people other than its
   authors.
7. **Problem before solution. Vocabulary before names.**

## Sources of change

- A person edits files in Fluidd or Mainsail.
- Klipper `SAVE_CONFIG` changes the autosave block in `printer.cfg` and writes `printer-<timestamp>.cfg` files.
- Other services write into the config folder: Moonraker (`.moonraker.conf.bkp`, update_manager),
  KlipperScreen, Crowsnest, timelapse.
- (More printers) A person makes a commit in the repository, from a desktop or the git host's web pages.

## Scenarios

### One printer (version 1 must do these very well)

- **S1 Backup.** printerkeep makes a backup only when a file changed. The backup has a useful message. A timer
  starts it. The user can also start it manually.
- **S2 Add a repository later.** A user with only local history adds a repository (any git host; GitHub is
  the most likely). printerkeep guides the user through authentication. A personal access token (HTTPS) and
  an SSH key both work. With a token, one token can serve all printers.
- **S3 Restore.** The SD card fails. The user installs Klipper again and restores the configuration from the
  repository.
- **S4 Update printerkeep** with Moonraker's update_manager (the update panel in Fluidd or Mainsail).
- **S5 Busy printer.** printerkeep never disturbs a print. It must be possible to stop printerkeep during a
  print with [layerlock](https://github.com/ressu/layerlock) (systemd `ExecCondition=`).
- **S6 Noise and secrets.** printerkeep ignores generated files by default. It never sends secret files (for
  example Moonraker's secrets file) to the repository.

### More printers (added on top; a one-time guided migration from one printer is acceptable)

- **M1 Changes from shared** get to every printer that follows the file. They do not remove the printer's
  local changes.
- **M2 A shared file edited on a printer.** The edit is a local change. It is an experiment on that printer.
  printerkeep backs it up and never sends it to other printers automatically.
- **M3 Copy to shared.** The user decides to send a local change to the other printers. This must be possible
  without a local clone.
- **M4 Lasting local changes.** A printer keeps a small difference from shared and still gets later changes
  from shared. If the changes do not touch the same lines, printerkeep merges them. If they touch the same
  lines, the file needs decision.
- **M5 Status.** For each printer and each followed file, the user can see one status: up to date, local
  changes, changes waiting or needs decision. The user can see it without a desktop, for example in the
  overview in the repository, in Fluidd, or with a command on the printer.
- **M6 A printer's own file changed in the repository** gets to that printer only.
- **M7 Long offline.** A printer is offline for weeks. When it comes back, it gets all changes correctly.
- **M8 Start to share a file that is already different on many printers.** This must be possible without a
  local clone.
- **M9 Needs decision.** A change from shared and a local change touch the same lines. The user understands
  what happened and selects or edits the result. The user needs at most the Fluidd editor, the git host's web
  pages or a simple command on the printer. Until the user decides, the configuration stays valid. Git
  conflict markers never get into a live file.

### How changes are applied (version 1, fixed)

printerkeep writes changes to the files, never restarts Klipper and tells the user. Klipper reads its
configuration only when it starts. Thus, writing files does not disturb a print.

## What printers share

- Identical files.
- Files that are almost identical, with small differences per printer.
- Each shared file is used by some printers, not necessarily all. Each printer follows each file it wants.
  There are no groups of printers.

## The core problem (more printers)

For each file that a printer follows, printerkeep must know three versions:

1. the printer's current version,
2. the shared version that the printer took last (the *base*),
3. the current shared version.

A backup records version 1. To apply changes from shared, printerkeep does a three-way merge of versions 1 and
3, with version 2 as the base. The status compares all three versions.

## Evidence from real configurations

Source: a real setup with four printers, backed up with klipper-backup, one branch per printer. This document
calls the printers 1–4.

- **Identical files exist.** `macros/reset.cfg` is identical on three printers. `spoolman.cfg`,
  `temperature.cfg` and `ui-helpers.cfg` are identical on two printers each.
- **Line endings.** Printer 2's `macros/temperature.cfg` is different from printer 3's only in its line
  endings (CRLF).
- **Different features.** `ui-helpers.cfg` and `macros.cfg` have lines that are commented out on printers
  without LEDs or without a runout sensor (`# STATUS_HEATING`, `# _RUNOUT_ON`).
- **Different values.** `KAMP_Settings.cfg` is different only in `variable_*` values and in which `[include]`
  lines are commented out.
- **Good changes that did not spread.** Printer 4 has a better `END_PRINT` retract (KAMP `tip_distance`).
  Printer 3 added `LOAD_FILAMENT` and `UNLOAD_FILAMENT`.
- **Same path, different file.** Printer 1's `macros/temperature.cfg` is a different file from the other
  printers' files. `core/host-mcu.cfg` is very different between printers. Thus, the same path does not mean
  the same file.
- **Same file, different path.** Printer 1 has `macros.cfg` at the top of the config folder. The other
  printers have `macros/macros.cfg`.
- **Other useful files.** `ukam/` firmware build files, `crowsnest.conf`, `octoeverywhere.conf` and
  `moonraker.conf`. All of them are in the config folder. KAMP itself is a symlink to another repository and
  is not backed up.
- **Nothing outside the config folder.** The klipper-backup setting is
  `backupPaths=( "printer_data/config/*" )`. Every branch has only `printer_data/config/…`. A separate tool
  (configuration management) controls the system-level configuration.

A three-way merge per line handles these real differences well: commented-out lines, changed values and added
macros.

## Decisions

| Wish | Real need | Decision |
|---|---|---|
| Local changes, merged when possible (M2, M4) | Printers are a little different and still get good changes | printerkeep does this. A printer's file = the shared version + its local changes, with a three-way merge against its base. When changes touch the same lines, the file needs decision. Users do not change the structure of their configuration. ("Identical shared file + Klipper overrides" was rejected for this reason.) |
| Groups of printers | Some files are for some printers only | No groups. Each printer follows each file it wants. |
| Desktop tools (M3, M5, M9) | See status, copy to shared, decide | No desktop tools are necessary. printerkeep runs on the printer. Its commands over SSH are acceptable, but no action or an existing tool is better where it is easy. |
| Timer and file watching | Back up changes soon | A timer and a manual start. No file watching (inotify) in version 1. |
| Settings for apply and restart | Do not surprise a printer that prints | Fixed in version 1 (see above). |
| One printer → more printers | Grow from one printer to more | A one-time guided migration is acceptable. |
| Update with update_manager | Easy installation and updates | Required (S4). update_manager's `executable` type installs compiled programs from GitHub releases. See [language.md](language.md). |

More decisions:

- **The scope is the config folder only.** That is Moonraker's config root (usually `~/printer_data/config`),
  with all its subfolders, and nothing outside it. The rule for users: if Fluidd or Mainsail shows a file in
  the configuration view, printerkeep backs it up (unless it is ignored). If not, printerkeep does not touch
  it. Other paths (like klipper-backup's `backupPaths`) are not supported. They would need more roots, path
  mapping, file permissions (some files need sudo) and restores that change system files. Keep files such as
  firmware build files in the config folder.
- **A shared file has the same path on each printer.** No path mapping.
- **Start to share a file that is already different (M8).** The version of one printer becomes the shared
  version. On each other printer that follows the file, its current differences become local changes.
- **printerkeep ignores symlinks.** To restore tools such as KAMP, Shake&Tune or `mainsail.cfg`, install
  them again.
- **Line endings do not count.** printerkeep ignores line-ending differences when it compares files. A
  difference only in line endings never causes a backup or a status change.

## Future possibilities

Not in version 1 and not designed for. But the design must not make them impossible.

- **Changes from other people.** Copy and paste is possibly enough. It is similar to M8: a version from
  another person becomes the shared version, and local changes merge on top. The design must not expect that
  every shared version comes from the user's own printers.
- **Groups of printers**, if users need them.
- **Backup when a file changes (inotify)**, settings for apply and restart, more commands for experts.
- **Time gating for state files.** Files that extensions rewrite often (`[save_variables]` files such as
  `variables.cfg`, Happy Hare's `mmu_vars.cfg`) can cause a backup at almost every run. Version 1 backs them
  up like other files. Later, printerkeep can back up such files less often (for example, at most once per
  hour).

## Not goals

- Central control that pushes configuration to printers.
- Templates as a requirement.
- Features that work only with GitHub.
- Backups of files outside the config folder (system files, `printer_data/systemd`, Moonraker's database,
  gcode files, logs). Installers, configuration management or other backup tools do this.

## Next steps

1. Done: [solution-options.md](solution-options.md) (option A).
2. Done: vocabulary and project name in [naming.md](naming.md).
3. Done: language and platforms in [language.md](language.md) (Rust, system git, Linux with systemd).
4. Done: names of commands, files, systemd units and the update_manager entry in [naming.md](naming.md).
5. Done: [implementation-plan.md](implementation-plan.md).
