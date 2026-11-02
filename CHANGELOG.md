# Changelog

What changes in each release of printerkeep. Read it before you update, so you know what to expect.

Fluidd and Mainsail link to the release page on GitHub, which shows the same notes.

## Unreleased

### Removed

- The diagram of the printers in the README of `shared`: the table of printers shows the same. Printers
  with 1.1 leave the new README as it is, and their rows update after you update them.

## 1.1.0 (2026-11-02)

### New

- The README of `shared` lists the printers that follow shared files, with the files that each one follows
  and a link to its branch. Each printer updates its own row when it checks the repository, so the list can
  be behind for a while. Printers that still run 1.0 appear in the list after you update them.
- You can add your own text to the README of `shared`. printerkeep updates only the parts between its
  `printerkeep:begin` and `printerkeep:end` comments, and keeps the rest. The first printer that you update
  replaces the README that 1.0 wrote with the new one.
- The README of each printer branch links each followed file to its version on shared, and shows how to go
  back to an earlier version. After you update, each printer makes one backup, `README.md from printerkeep`,
  with the new README.

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
