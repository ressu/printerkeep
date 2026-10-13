# printerkeep

printerkeep keeps the configuration of your Klipper printers in a git repository: one repository for one or
more printers. It's a standalone tool that stays out of your way. After the install it runs by itself, and
you don't need git skills or a second computer.

It has a few firm opinions, so that it can stay simple:

- **Each printer has its own branch** in the repository, with the files of its config folder.
- **Edits go both ways.** Change a file on the printer, or in the repository (for example in GitHub's web
  editor): printerkeep brings the two together. When both changed the same lines, it marks them in the file
  for you to decide.
- **Several printers can use the same files,** and each printer keeps its own differences, such as its pins.

printerkeep works with Klipper and with firmware based on it, such as Kalico.

> **Status:** early releases (0.x). Commands and files can still change.

## What printerkeep does

- Makes a backup of the config folder whenever a file changed.
- Keeps the history on the printer, and optionally in a repository, for example on GitHub.
- Writes changes that you make in the repository into the printer's config folder.
- Restores a printer from the repository.
- With several printers: shared files, with local changes on each printer (see
  [Several printers](#several-printers)).
- Updates itself through Moonraker's update manager.
- Continues the history of an existing [klipper-backup](https://github.com/Staubgeborener/klipper-backup)
  repository (see [Move from klipper-backup](#move-from-klipper-backup)).

## Install

printerkeep installs for the printer's user, in the home folder. It never uses sudo, and it needs no root
access. `printerkeep install` sets up these parts:

| Part | Where |
|---|---|
| The program, and `release_info.json` for Moonraker's update manager | `~/printerkeep/` |
| A link, so that the command is `printerkeep` | `~/.local/bin/printerkeep` |
| A timer that makes a backup every 5 minutes when a file changed (systemd user units) | `~/.config/systemd/user/printerkeep.service`, `printerkeep.timer` |
| Updates in the update panel of Fluidd and Mainsail | `[update_manager printerkeep]` in `moonraker.conf` |

systemd stops a user's timers when the user logs out, unless "lingering" is on for that user. `printerkeep
install` turns lingering on with `loginctl enable-linger`. If the system doesn't allow that, printerkeep shows
the command for an administrator. If [layerlock](https://github.com/ressu/layerlock) is installed, backups
wait until the print is done.

You can install in three ways. They differ only in how you download the program and how you check it.

### 1. The install script, in one command

```sh
curl -fsSL https://github.com/ressu/printerkeep/releases/latest/download/install.sh | sh
```

The script downloads the program for your CPU, compares it with the checksum file of the release, and runs
`printerkeep install`. The checksum file comes from the same place as the program, so this check finds a
damaged download, but it can't prove that the program is genuine. For that, use the second way.

You can set these variables before `sh`, for example `curl … | PRINTER_NAME=printer-2 sh`:

| Variable | Default | Description |
|---|---|---|
| `VERSION` | latest | The release to install, for example `v0.2.1` |
| `BINARY_SHA256` | none | The checksum of the program. The script then checks against it instead of the checksum file. |
| `PRINTER_DATA` | `~/printer_data` | The printer_data folder |
| `PRINTER_NAME` | `main` | The printer name, which is also its branch in the repository. Give each printer its own name when several printers use the same repository. |

### 2. The install script, read and checked first

```sh
curl -fsSL https://github.com/ressu/printerkeep/releases/download/v0.2.1/install.sh -o install.sh
less install.sh
VERSION=v0.2.1 BINARY_SHA256="sha256:<checksum>" sh install.sh
```

Replace `<checksum>` with the checksum of the program for your CPU. The
[release page](https://github.com/ressu/printerkeep/releases) shows a `sha256:` checksum for each file. The
script installs the program only if its checksum matches.

### 3. Without a script

Find the file for your CPU. The command `uname -m` shows which one you need:

| `uname -m` | File |
|---|---|
| `aarch64` | `printerkeep-linux-aarch64` |
| `armv7l` | `printerkeep-linux-armv7` |
| `armv6l` | `printerkeep-linux-armv6` |
| `x86_64` | `printerkeep-linux-x86_64` |

Then download it, check it, and install it. For an `aarch64` printer:

```sh
curl -fsSLO https://github.com/ressu/printerkeep/releases/download/v0.2.1/printerkeep-linux-aarch64
sha256sum printerkeep-linux-aarch64      # compare with the checksum on the release page
chmod +x printerkeep-linux-aarch64
./printerkeep-linux-aarch64 install      # copies itself to ~/printerkeep
rm printerkeep-linux-aarch64
```

### Update and remove

- **Update:** use the update panel in Fluidd or Mainsail. Restart Moonraker once after the install, so that
  the panel shows printerkeep. To update by hand, install again in any of the three ways.
- **Remove:** run `printerkeep uninstall`. It keeps the history, the login for the repository, and the
  settings file.

## Use

### Backups

After the install, printerkeep checks the config folder every 5 minutes and makes a backup when a file
changed. The history is on the printer, in `printer_data/printerkeep`. Each backup has a message that says
which files changed, for example `Changed printer.cfg, added macros/new.cfg` or
`SAVE_CONFIG changed printer.cfg`.

```sh
printerkeep backup     # make a backup now
printerkeep status     # the last backup, the changes since then, and the repository
```

printerkeep writes what it does to `printer_data/logs/printerkeep.log`, which Fluidd and Mainsail show with
the other logs. A run in which nothing changed writes nothing, so quiet runs don't wear the SD card.

When something goes wrong, for example when the repository can't be reached, the log says what happened and
what to do. It says so once, and again when the problem is solved. Until then, `printerkeep status` shows the
problem.

### Send backups to a repository

A repository keeps a copy of your backups away from the printer, for example for when the SD card fails.
Make an **empty, private** repository, for example on GitHub. Then run:

```sh
printerkeep add-repository https://github.com/<user>/<repository>.git
```

Replace `<user>` and `<repository>` with your GitHub user name and the name of the repository.

printerkeep asks for a personal access token. To make one on GitHub, go to Settings > Developer settings >
Personal access tokens > Fine-grained tokens > Generate new token. Under "Repository access", select "Only
select repositories" and then the repository. Under "Permissions", give "Contents" the access "Read and
write".

If the repository already has printers, printerkeep lists them and asks which one this printer is. Type
its name, or type a new name to start a new branch.

If that branch has backups that aren't from this printer, printerkeep asks before it continues that history.
This happens, for example, after you set the printer up again. Continuing doesn't change your files. The
backups that were only on this printer stay on it, set aside, and aren't sent.

If the branch is new and the printer already has several backups, printerkeep asks whether to send them all
or to start the history in the repository from now.

In a script, printerkeep can't ask, and it doesn't guess. Give the answers as options instead:

| Option | Meaning |
|---|---|
| `--printer-name <name>` | This printer's name in the repository: an existing printer, or a new branch |
| `--continue` | Continue that branch, also when its backups aren't from this printer |
| `--from-now` | For a new branch: send only a backup of the files as they are now |

More about the repository:

- printerkeep keeps the token in `printer_data/printerkeep/credentials`, which only your user can read. The
  settings file stores only the URL, so give the URL without the token.
- After each backup, printerkeep sends the new backups to the printer's branch. It never overwrites changes
  in the repository.
- A repository that you made with a README or a license (GitHub's "Add a README file" option) works too.
  printerkeep continues from it.
- printerkeep adds a `README.md` to the branch, which says what the branch is and how to restore the printer.
  This file stays in the repository; it isn't on the printer. If your config folder has its own `README.md`,
  printerkeep backs up yours instead.
- When the token expires, `printerkeep status` and the log tell you. Make a new token, then run the command
  below. printerkeep switches to the new token only if the repository accepts it.

  ```sh
  printerkeep change-token
  ```

- With an SSH URL (`git@github.com:<user>/<repository>.git`), printerkeep makes its own SSH key and shows the
  public key. Add it to the repository as a deploy key with write access.

### Move from klipper-backup

Use the same repository:

1. Stop and remove klipper-backup on the printer, as its documentation describes. If it keeps running, it
   sends its backups to the same branch.
2. Make a new token for printerkeep.
3. Run `printerkeep add-repository` as described above.
4. Select the klipper-backup branch of this printer: `main`, or the `branch_name` that you gave
   klipper-backup.
5. Answer yes to continue its history.

In a script, give the answers as options:

```sh
printerkeep add-repository <url> --printer-name <branch> --continue
```

printerkeep then continues the klipper-backup history. The files move from `printer_data/config` to the top
of the branch, and the older backups stay in the history. git follows the files across the move, so the
history of each file is complete.

### Restore a printer

For example after a new SD card, install Klipper, Moonraker, and printerkeep. Then run:

```sh
printerkeep restore https://github.com/<user>/<repository>.git
```

If the repository has several printers, printerkeep asks which one this is. In a script, add
`--printer-name <printer name>`.

printerkeep writes the files of that printer's last backup into the config folder and continues its history.
The backups that were only on this printer, such as the first backup that the install made, are set aside.

A restore only adds and replaces files; it never removes any. If a file in the config folder differs from the
backup, printerkeep stops and lists those files. To replace them with the backup, add `--overwrite`.

Restart Moonraker and Klipper after a restore.

### Change files in the repository

You can edit a printer's files in the repository, for example in GitHub's web editor, on the printer's
branch. printerkeep checks the repository at each run (see `check_repository` in [Settings](#settings)) and
writes your changes into the config folder. When the printer changed the same file, printerkeep merges both
changes. When both changed the same lines, the file needs a decision (see [Needs decision](#needs-decision)).

printerkeep keeps the files in the config folder that you remove in the repository. The log tells you about
each one, so that you can remove it on the printer too.

### Settings

printerkeep works without settings. To change its behavior, use the settings file,
`printer_data/config/printerkeep.conf`:

```
[printerkeep]
# The printer name, which is also its branch in the repository. The default is main.
# To change it, run printerkeep rename <name> (see "Printer name"), because the history must move too.
printer_name: printer-1
# Written by add-repository and restore.
repository: https://github.com/<user>/<repository>.git
# Files that printerkeep skips, one rule per line, like .gitignore rules.
ignore:
    *.csv
    timelapse/
# How often printerkeep checks the repository for changes: off, or a time such as 5m, 1h, or 1d.
# The default is 5m, that is, each run of the timer. printerkeep backup by hand always checks.
check_repository: 1h
```

printerkeep keeps the time of the last repository check in memory (`/run/user/<id>`), not on the SD card.

### Printer name

The printer name is also the name of the printer's branch in the repository. The default is `main`. Set it
at install with `--printer-name`, or with `PRINTER_NAME` for the install script. To change it later, run:

```sh
printerkeep rename printer-2
```

What `rename` does:

- On the printer, the history moves to the new name, and the settings file gets the new name.
- In the repository, the next send makes a **new** branch with the new name and the whole history. The old
  branch stays as it was: printerkeep leaves it alone. Remove it in the web pages of the repository when you
  no longer need it.
- If the repository already has a branch with the new name, from another printer or from klipper-backup,
  `rename` stops. It never takes over a branch. To make this printer continue that branch, run
  `printerkeep add-repository <url>` and select that printer. In a script, add
  `--printer-name <branch> --continue`.

### Files that printerkeep skips

printerkeep always skips these files, in addition to your `ignore` rules:

- the copies that Klipper makes for `SAVE_CONFIG` (`printer-20261011_120000.cfg`), and its temporary
  `*_autosave` files,
- Moonraker's copy of its configuration (`.moonraker.conf.bkp`),
- `*.secrets` files, because secrets must never go to a repository,
- `.git` folders of tools in the config folder,
- editor and system files (`*.swp`, `*.swo`, `*~`, `.DS_Store`, `__pycache__`, `*.pyc`),
- symbolic links. Tools such as KAMP come back when you install them again.

## Several printers

All printers use the same repository, and each printer has its own branch. Give each printer its own name
(see [Printer name](#printer-name)); one printer can keep the default name `main`. printerkeep never takes
over the branch of another printer with the same name: `add-repository` stops and tells you what to do.

Files that several printers use can be **shared**. A printer that **follows** a shared file gets the changes
to it and keeps its own changes to the file, its **local changes**. Each printer follows only the files
that it needs.

### Copy a file to shared, and follow it

On the printer that has the best version of the file, run:

```sh
printerkeep copy-to-shared macros/macros.cfg
```

This makes the shared version the same as this printer's version, and this printer then follows the file.
Run it again later to send an improvement to the other printers.

Before it copies, printerkeep shows the changes that will go to the other printers and asks you to confirm.
Look for lines that belong to this printer only, for example lines that you commented out because this
printer lacks some hardware. If you find such lines, answer no. Then make only the improvement in the shared
file, in the web editor of the repository. In a script, `--yes` skips the question.

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
printerkeep stop-following macros/macros.cfg
```

### Changes from shared

At each run, printerkeep writes the changes from shared into the followed files and keeps the local changes.
printerkeep never restarts Klipper, so restart it when you want to use the changes. Each change is saved as a
backup, so it shows in the history.

### Needs decision

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

Keep the lines that you want, and delete all `####` lines. printerkeep records the decision at the next run,
and until then it leaves this file alone.

To decide without editing the file, run one of these:

```sh
printerkeep decide macros/macros.cfg --use-shared
printerkeep decide macros/macros.cfg --keep-mine
```

### Lines for one printer only

Some lines must be different on each printer, for example pins. Mark them with an override comment:

```
[filament_switch_sensor runout]
switch_pin: ^PG12   # printerkeep: override
```

Changes from shared never change an override line, and `copy-to-shared` keeps it on this printer. When
shared doesn't have the line yet, shared gets `switch_pin: CHANGE_ME   # printerkeep: override`, and each
printer that starts to follow the file sets its own value. Klipper refuses `CHANGE_ME` for pins and numbers,
so a printer never starts with another printer's value.

### Status and the shared branch

`printerkeep status` shows each followed file with its status: up to date, local changes, changes waiting,
or needs decision.

The `shared` branch has a `README.md` that explains what the repository is and how to restore a printer.
Each printer's branch is in the branch list of the repository. With several printers, make `shared` the
default branch, so that the front page of the repository shows this README. On GitHub, go to the
repository's Settings > General > Default branch.

## Design

The design is in [`docs/`](docs/). Start with [problem.md](docs/problem.md).

## Development

```sh
nix develop          # optional: the toolchain with the release targets, qemu, and shellcheck (or direnv: .envrc)
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
shellcheck install.sh scripts/*.sh
```

Without Nix, [rustup](https://rustup.rs) installs the toolchain from `rust-toolchain.toml`.

## License

[MIT](LICENSE)
