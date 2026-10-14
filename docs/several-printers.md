# Several printers

printerkeep can keep several printers in one repository. Printers can also share files, such as macros: a
change on one printer reaches the others, and each printer keeps its own differences, such as its pins.

Start with the [README](../README.md) for one printer: install, connect, and the everyday commands. The
[user guide](user-guide.md) has the details for each printer.

## One repository, one branch for each printer

All printers use the same repository, and each printer has its own branch. Give each printer its own name
(see [Printer name](user-guide.md#printer-name)); one printer can keep the default name `main`. printerkeep
never takes over the branch of another printer with the same name: `connect` stops and tells you what to do.

Files that several printers use can be **shared**. A printer that **follows** a shared file gets the changes
to it and keeps its own changes to the file, its **local changes**. Each printer follows only the files that
it needs.

## Copy a file to shared, and follow it

On the printer that has the best version of the file, run:

```sh
printerkeep copy-to-shared macros/macros.cfg
```

This makes the shared version the same as this printer's version, and this printer then follows the file.
Run it again later to send an improvement to the other printers.

Before it copies, printerkeep shows the changes that go to the other printers and asks you to confirm. Look
for lines that belong to this printer only, for example lines that you commented out because this printer
lacks some hardware. If you find such lines, answer no. Then make only the improvement in the shared file, in
the web editor of the repository. In a script, `--yes` skips the question.

On each other printer, run:

```sh
printerkeep follow macros/macros.cfg
```

If the printer already has the file, its differences from the shared version stay as local changes. If it
doesn't have the file, printerkeep writes the shared version. You can also add the file to `follow:` in the
settings file, for example in the Fluidd or Mainsail editor:

```
[printerkeep]
follow:
    macros/macros.cfg
```

To stop following a file, run the command below. The file stays on the printer.

```sh
printerkeep unfollow macros/macros.cfg
```

You can give several files at once, for example `printerkeep copy-to-shared macros.cfg leds.cfg`. The same
goes for `follow`, `unfollow`, and `decide`. printerkeep checks all the files first. If one can't be
used, it changes nothing and tells you why. A symlink can't be shared, because printerkeep doesn't back up
symlinks.

## Changes from shared

At each sync, printerkeep writes the changes from shared into the followed files and keeps the local changes.
printerkeep never restarts Klipper, so restart it when you want to use the changes. Each change is saved as a
backup, so it shows in the history.

`printerkeep backup` doesn't write changes from shared. They wait for the next sync.

## Needs decision

When a change from shared and a local change touch the same lines, printerkeep can't choose for you. It keeps
the printer's lines active and puts the shared lines below them, commented out with `#`. The file stays valid
configuration:

```
#### printerkeep: needs decision ##########
#### Shared changed this line too.
#### Active now (this printer):
variable_purge_margin: 20
#### Shared version. To use it, remove the
#### # from the next line and delete the
#### line above:
#variable_purge_margin: 12
#### When done, delete all #### lines.
#### printerkeep: end ######################
```

Keep the lines that you want, and delete all `####` lines. printerkeep records the decision at the next sync,
and until then it leaves this file alone.

To decide without editing the file, run one of these:

```sh
printerkeep decide macros/macros.cfg --use-shared
printerkeep decide macros/macros.cfg --keep-mine
```

## Lines for one printer only

Some lines must be different on each printer, for example pins. Mark them with an override comment:

```
[filament_switch_sensor runout]
switch_pin: ^PG12   # printerkeep: override
```

Changes from shared never change an override line, and `copy-to-shared` keeps it on this printer. When
shared doesn't have the line yet, shared gets `switch_pin: CHANGE_ME   # printerkeep: override`, and each
printer that starts to follow the file sets its own value. Klipper refuses `CHANGE_ME` for pins and numbers,
so a printer never starts with another printer's value.

## Status and the shared branch

`printerkeep status` shows each followed file with its status: up to date, local changes, changes waiting,
or needs decision.

The `shared` branch has a `README.md` that explains what the repository is and how to restore a printer.
Each printer's branch is in the branch list of the repository. With several printers, make `shared` the
default branch, so that the front page of the repository shows this README. On GitHub, go to the
repository's Settings > General > Default branch.

To take one file from another printer, use `restore` with its name:

```sh
printerkeep restore macros.cfg --printer-name printer-2
```
