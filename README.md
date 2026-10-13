# printerkeep

printerkeep keeps the configuration of your Klipper printer safe in git. With several printers, it also lets
them use the same files, while each printer keeps its own changes.

printerkeep works with Klipper and with firmware based on it, such as Kalico.

> **Status: in development, not released yet.**

printerkeep replaces [klipper-backup](https://github.com/Staubgeborener/klipper-backup), and can continue the
history of an existing klipper-backup repository (see [Move from klipper-backup](#move-from-klipper-backup)).

## What printerkeep does

- Makes a backup of the config folder, but only when a file changed.
- Keeps the history on the printer. A repository (for example on GitHub) is optional.
- Restores a printer from the repository.
- Writes changes that you make in the repository (for example in GitHub's web editor) into the printer's
  config folder.
- Updates itself through Moonraker's update manager.
- With several printers: shared files, with local changes on each printer (see
  [Several printers](#several-printers)).

## Install

printerkeep installs for the printer's user, in the home folder. It never uses sudo and needs no root
access. `printerkeep install` sets up the following:

| What | Where |
|---|---|
| The program, and `release_info.json` for Moonraker's update manager | `~/printerkeep/` |
| A link, so that the command is `printerkeep` | `~/.local/bin/printerkeep` |
| A timer that makes a backup every 5 minutes, if a file changed (systemd user units) | `~/.config/systemd/user/printerkeep.service`, `printerkeep.timer` |
| Updates in the Fluidd and Mainsail update panel | `[update_manager printerkeep]` in `moonraker.conf` |

systemd stops a user's timers when the user logs out, unless "lingering" is on for that user.
`printerkeep install` turns it on with `loginctl enable-linger`. If the system does not allow that, it shows
the command that an administrator can run. If [layerlock](https://github.com/ressu/layerlock) is installed,
no backup starts during a print.

There are three ways to install. They differ only in how you download the program and how you check it.

### 1. The install script, in one command

```sh
curl -fsSL https://github.com/ressu/printerkeep/releases/latest/download/install.sh | sh
```

The script downloads the program for your CPU, checks it against the checksum file of the release, and runs
`printerkeep install`. The checksum file comes from the same place as the program, so this check only finds a
damaged download. It does not prove that the program is genuine.

| Variable | Default | Description |
|---|---|---|
| `VERSION` | latest | Install a specific release, for example `v0.1` |
| `BINARY_SHA256` | none | The checksum of the program. The script then does not download the checksum file. |
| `PRINTER_DATA` | `~/printer_data` | The printer_data folder |
| `PRINTER_NAME` | `main` | The printer name, which is also its branch name in the repository. Give each printer its own name if several printers use the same repository. |

### 2. The install script, read and checked first

```sh
curl -fsSL https://github.com/ressu/printerkeep/releases/download/v0.1/install.sh -o install.sh
less install.sh
VERSION=v0.1 BINARY_SHA256="sha256:<checksum>" sh install.sh
```

Copy the checksum of the program for your CPU from the [release page](https://github.com/ressu/printerkeep/releases)
(GitHub shows a `sha256:` checksum for each file). The script installs the program only if its checksum
matches.

### 3. Without a script

Find the file for your CPU (`uname -m` shows it):

| `uname -m` | File |
|---|---|
| `aarch64` | `printerkeep-linux-aarch64` |
| `armv7l` | `printerkeep-linux-armv7` |
| `armv6l` | `printerkeep-linux-armv6` |
| `x86_64` | `printerkeep-linux-x86_64` |

```sh
curl -fsSLO https://github.com/ressu/printerkeep/releases/download/v0.1/printerkeep-linux-aarch64
sha256sum printerkeep-linux-aarch64      # compare with the checksum on the release page
chmod +x printerkeep-linux-aarch64
./printerkeep-linux-aarch64 install      # copies itself to ~/printerkeep
rm printerkeep-linux-aarch64
```

### Update and remove

- **Update:** use the update panel in Fluidd or Mainsail. Restart Moonraker once after the install, so that
  the panel shows printerkeep. To update by hand, install again in one of the three ways.
- **Remove:** `printerkeep uninstall`. It keeps the history of the backups, the login for the repository, and
  `printerkeep.conf`.

## Use

### Backups

After the install, printerkeep checks the config folder every 5 minutes, and makes a backup if a file
changed. The history is on the printer, in `printer_data/printerkeep`. Each backup has a message that says
which files changed, for example `Changed printer.cfg, added macros/new.cfg` or
`SAVE_CONFIG changed printer.cfg`.

```sh
printerkeep backup     # make a backup now
printerkeep status     # the last backup, changes since then, and the repository
```

printerkeep writes what it does to `printer_data/logs/printerkeep.log`. Fluidd and Mainsail show it with
the other logs. A run in which nothing changed writes nothing at all, so the SD card is not worn by
backups that did not happen.

### Send backups to a repository

A repository keeps a copy of your backups away from the printer, for example for when the SD card fails.
Make an **empty, private** repository, for example on GitHub. Then:

```sh
printerkeep add-repository https://github.com/<user>/<repository>.git
```

printerkeep asks for a personal access token. To make one on GitHub: Settings > Developer settings >
Personal access tokens > Fine-grained tokens > Generate new token. Repository access: "Only select
repositories", and select the repository. Permissions: "Contents", "Read and write".

- Do not put the token in the URL. printerkeep keeps the token in `printer_data/printerkeep/credentials`,
  which only your user can read. `printerkeep.conf` stores only the URL.
- After each backup, printerkeep sends the new backups to the repository. They go to the branch with the
  printer name (`main` if you did not set one). printerkeep never overwrites changes in the repository.
- If you made the repository with a README or a license (GitHub's "Add a README file" option), that is
  fine: printerkeep continues from it.
- printerkeep adds a `README.md` to the branch. It explains what the branch is and how to restore the
  printer. This file is not in your config folder. If your config folder has its own `README.md`,
  printerkeep backs up your file instead.
- When the token expires, `printerkeep status` and the log tell you. Make a new token, then run
  `printerkeep change-token`. printerkeep switches to the new token only if the repository accepts it.
- With an SSH URL (`git@github.com:<user>/<repository>.git`), printerkeep makes its own SSH key and shows the
  public key. Add it to the repository as a deploy key with write access.

### Move from klipper-backup

Use the same repository. First stop and remove klipper-backup on the printer, as its documentation
describes; if it keeps running, it sends its backups to the same branch. Make a new token for printerkeep,
then run `printerkeep add-repository` as above.

If the branch with the printer name (`main` by default) has klipper-backup's backups, printerkeep continues
that history. The files move from `printer_data/config` to the top of the branch, and the older backups stay
in the history. git follows the files across the move, so the history of each file is complete.

### Restore a printer

For example after a new SD card: install Klipper, Moonraker and printerkeep, then run:

```sh
printerkeep restore https://github.com/<user>/<repository>.git
```

printerkeep writes the files of the backup into the config folder and continues the history. It never
removes files. If a file in the config folder is different from the backup, printerkeep stops and lists
those files; add `--overwrite` to replace them. If the repository has backups of several printers, add the
printer name: `printerkeep restore <url> <printer name>`. Restart Moonraker and Klipper after a restore.

### Change files in the repository

You can edit a printer's files in the repository, for example in GitHub's web editor, on the branch with
the printer name. printerkeep checks the repository at each run (see `check_repository` in
[Settings](#settings)) and writes your changes into the config folder. If the printer changed the same
file, printerkeep merges both changes. If both changed the same lines, the file needs a decision (see
[Needs decision](#needs-decision)).

printerkeep never removes a file from the config folder. If you remove a file in the repository, the log
tells you, so that you can remove it on the printer too.

### Settings

printerkeep needs no settings. `printer_data/config/printerkeep.conf` is optional:

```
[printerkeep]
# The printer name, which is also its branch name in the repository. The default is main.
# To change it, run printerkeep rename <name>; do not edit this line, because the history must move too.
printer_name: printer-1
# Written by add-repository and restore.
repository: https://github.com/<user>/<repository>.git
# Files that printerkeep does not back up, one rule per line, like .gitignore rules.
ignore:
    *.csv
    timelapse/
# How often printerkeep checks the repository for changes: off, or a time such as 5m, 1h or 1d.
# The default is 5m (each run of the timer). printerkeep backup by hand always checks.
check_repository: 1h
```

printerkeep keeps the time of the last repository check in memory (`/run/user/<id>`), not on the SD card.

### Files that are not backed up

printerkeep always skips these files, in addition to your `ignore` rules:

- the copies that Klipper makes for `SAVE_CONFIG` (`printer-20261011_120000.cfg`), and its temporary
  `*_autosave` files,
- Moonraker's copy of its configuration (`.moonraker.conf.bkp`),
- `*.secrets` files: secrets must never go to a repository,
- `.git` folders of tools in the config folder,
- editor and system files (`*.swp`, `*.swo`, `*~`, `.DS_Store`, `__pycache__`, `*.pyc`),
- symbolic links. Tools such as KAMP come back when you install them again.

## Several printers

Each printer has its own branch, named after the printer, so give each printer its own name. Set it at
install with `--name printer-2` (or `PRINTER_NAME=printer-2` for the install script), or later with
`printerkeep rename printer-2`. One printer can keep the default name `main`. printerkeep never takes over
the branch of another printer with the same name: `add-repository` stops and tells you what to do.

Files that several printers use can be **shared**. A printer that **follows** a shared file gets the changes
to it, and keeps its own changes to the file (its **local changes**). Each printer follows only the files
that it needs. All printers must use the same repository.

### Copy a file to shared, and follow it

On the printer that has the best version of the file, run:

```sh
printerkeep copy-to-shared macros/macros.cfg
```

This makes the shared version the same as this printer's version, and this printer then follows the file.
Run it again later to send an improvement to the other printers.

Before it copies, printerkeep shows the changes that will go to the other printers, and asks you to confirm.
Check for lines that belong to this printer only, for example lines that you commented out because this
printer lacks some hardware. If there are such lines, answer no, and instead make only the improvement in
the shared file, in the web editor of the repository. (`--yes` skips the question.)

On each other printer, run:

```sh
printerkeep follow macros/macros.cfg
```

If the printer already has the file, its differences from the shared version stay as local changes. If it
does not, printerkeep writes the shared version. You can also add the file to `follow:` in
`printerkeep.conf`, for example in the Fluidd or Mainsail editor:

```
[printerkeep]
follow:
    macros/macros.cfg
```

To stop following a file: `printerkeep stop-following macros/macros.cfg`. The file stays on the printer.

### Changes from shared

At each run, printerkeep writes the changes from shared into the followed files, and keeps the local
changes. printerkeep never restarts Klipper: restart it when you want to use the changes. Each change is
saved as a backup, so it shows in the history.

### Needs decision

If a change from shared and a local change touch the same lines, printerkeep cannot choose for you. It keeps
the printer's lines active, and puts the shared lines below them, commented out with `#`. The file stays
valid configuration:

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

Keep the lines that you want, and delete all `####` lines. printerkeep records the decision at the next run.
Until then, it writes no more changes from shared into this file. To decide without editing the file:
`printerkeep decide macros/macros.cfg --use-shared` or `--keep-mine`.

### Lines for one printer only

Some lines must be different on each printer, for example pins. Mark them with an override comment:

```
[filament_switch_sensor runout]
switch_pin: ^PG12   # printerkeep: override
```

Changes from shared never change an override line, and `copy-to-shared` never copies it. If shared does not
have the line yet, shared gets `switch_pin: CHANGE_ME   # printerkeep: override`, and each printer that starts
to follow the file sets its own value. Klipper refuses `CHANGE_ME` for pins and numbers, so a printer never
starts with another printer's value.

### Status and the shared branch

`printerkeep status` shows each followed file with its status: up to date, local changes, changes waiting,
or needs decision.

The `shared` branch has a `README.md` that explains what the repository is and how to restore a printer.
Each printer's branch is in the branch list of the repository. With several printers, make `shared` the
default branch, so that the repository's front page shows this README. On GitHub: the repository's
Settings > General > Default branch.

## Design

The design is in [`docs/`](docs/). Start with [problem.md](docs/problem.md).

## Development

```sh
nix develop          # optional: toolchain with the release targets, qemu, shellcheck (or direnv: .envrc)
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
shellcheck install.sh scripts/*.sh
```

Without Nix, [rustup](https://rustup.rs) installs the toolchain from `rust-toolchain.toml`.

## License

[MIT](LICENSE)
