# Changelog

What changes in each release of printerkeep. Read it before you update, so you know what to expect.

Fluidd and Mainsail link to the release page on GitHub, which shows the same notes.

## 1.0.0 (2026-10-31)

printerkeep 1.0 has everything that was planned: backups of one printer, a repository, restore, updates in
the update panel, and several printers that follow files from shared. Nothing changes when you update from
0.7.0.

From 1.0 on, the names of the commands, settings, and files stay the same. If a later release renames one,
the old name keeps working.

## 0.7.0 (2026-10-26)

### New

- A bed mesh change alone no longer makes a backup. It waits and goes into the next backup with another
  change, so your history stays readable and a restore still gets a recent mesh. To back it up now, run
  `printerkeep backup`. The new `bundled_changes` setting lets you choose what waits, for example
  `variables.cfg`, or nothing at all. See "Changes that wait for the next backup" in the user guide.

### Fixed

- When several followed files aren't in shared, `sync` shows one notice with one `copy-to-shared` command
  for all of them, instead of one notice for each file.

## 0.6.0 (2026-10-15)

### New

- `copy-to-shared` lets you choose change by change. For each change, copy it to shared, keep it on this
  printer as a local change, or mark it as an override so that it never leaves this printer. So a printer
  that fixes a macro no longer sends its own macro values along with the fix.
- `connect` has clearer messages. When klipper-backup is still set up, it tells you that printerkeep makes the
  backups from now on, and shows how to remove klipper-backup.
