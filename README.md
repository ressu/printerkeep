# printerkeep

printerkeep keeps the configuration of your Klipper printer safe in git. It also keeps more printers in step:
printers can follow shared files and still keep their own changes.

printerkeep works with Klipper and with firmware derived from it, such as Kalico.

> **Status: in development, not released yet.** Release 0.1 replaces
> [klipper-backup](https://github.com/Staubgeborener/klipper-backup) for one printer.

## What printerkeep does

- Makes a backup of the config folder only when a file changed.
- Keeps the history on the printer. A repository (for example on GitHub) is optional.
- Restores a printer from the repository.
- Updates itself through Moonraker's update manager.
- From release 0.2: shared files for more printers, with local changes on each printer.

## Install

printerkeep installs for the printer's user, in the home folder. It never uses sudo and needs no root
access. `printerkeep install` makes these:

| What | Where |
|---|---|
| The program, and `release_info.json` for Moonraker's update manager | `~/printerkeep/` |
| A link, so the command is `printerkeep` | `~/.local/bin/printerkeep` |
| A timer that makes a backup every 5 minutes, when a file changed (systemd user units) | `~/.config/systemd/user/printerkeep.service`, `printerkeep.timer` |
| Updates in the Fluidd and Mainsail update panel | `[update_manager printerkeep]` in `moonraker.conf` |

systemd stops the timers of a user when the user logs out, unless "lingering" is on for that user.
`printerkeep install` turns it on with `loginctl enable-linger`. If the system does not permit that, it tells
you the command for an administrator. With [layerlock](https://github.com/ressu/layerlock) installed, no
backup starts during a print.

Select one of these three ways. They are different only in how you get the program and how you check it.

### 1. The install script, in one command

```sh
curl -fsSL https://github.com/ressu/printerkeep/releases/latest/download/install.sh | sh
```

The script downloads the program for your CPU, compares it with the checksum file of the release, and runs
`printerkeep install`. The checksum file comes from the same place as the program. Thus, this check finds
only a damaged download. It does not prove that the program is the correct one.

| Variable | Default | Description |
|---|---|---|
| `VERSION` | latest | Install a specific release, for example `v0.1.0` |
| `PRERELEASE` | `0` | `1` installs the latest pre-release |
| `BINARY_SHA256` | none | The checksum of the program. The script then does not download the checksum file. |
| `PRINTER_DATA` | `~/printer_data` | The printer_data folder |

### 2. The install script, read and checked first

```sh
curl -fsSL https://github.com/ressu/printerkeep/releases/download/v0.1.0/install.sh -o install.sh
less install.sh
VERSION=v0.1.0 BINARY_SHA256="sha256:<checksum>" sh install.sh
```

Copy the checksum of the program for your CPU from the [release page](https://github.com/ressu/printerkeep/releases)
(GitHub shows a `sha256:` checksum for each file). The script installs the program only if it has that
checksum.

### 3. Without a script

Select the file for your CPU (`uname -m` shows it):

| `uname -m` | File |
|---|---|
| `aarch64` | `printerkeep-linux-aarch64` |
| `armv7l` | `printerkeep-linux-armv7` |
| `armv6l` | `printerkeep-linux-armv6` |
| `x86_64` | `printerkeep-linux-x86_64` |

```sh
curl -fsSLO https://github.com/ressu/printerkeep/releases/download/v0.1.0/printerkeep-linux-aarch64
sha256sum printerkeep-linux-aarch64      # compare with the checksum on the release page
chmod +x printerkeep-linux-aarch64
./printerkeep-linux-aarch64 install      # copies itself to ~/printerkeep
rm printerkeep-linux-aarch64
```

### Update and remove

- **Update:** in the update panel of Fluidd or Mainsail. Restart Moonraker once after the install, so that the
  panel shows printerkeep. To update by hand, install again in one of the three ways.
- **Remove:** `printerkeep uninstall`. It keeps the history of the backups, the login for the repository and
  `printerkeep.conf`.

## Use

### Backups

After the install, printerkeep makes a backup every 5 minutes, but only when a file in the config folder
changed. The history is on the printer, in `printer_data/printerkeep`. Each backup has a message that tells
which files changed, for example `Changed printer.cfg, added macros/new.cfg`, or
`SAVE_CONFIG changed printer.cfg`.

```sh
printerkeep backup     # make a backup now
printerkeep status     # the last backup, changes since then, and the repository
```

printerkeep writes what it does to `printer_data/logs/printerkeep.log`. Fluidd and Mainsail show it with the
other logs.

### Send backups to a repository

A repository keeps your backups safe when the printer's SD card fails. Make an **empty, private** repository,
for example on GitHub. Then:

```sh
printerkeep add-repository https://github.com/<user>/<repository>.git
```

printerkeep asks for a personal access token. On GitHub: Settings > Developer settings > Personal access
tokens > Fine-grained tokens > Generate new token. Repository access: "Only select repositories", select the
repository. Permissions: "Contents", "Read and write".

- Do not put the token in the URL. printerkeep keeps it in `printer_data/printerkeep/credentials`, which only
  your user can read. `printerkeep.conf` gets the URL only.
- After each backup, printerkeep sends new backups to the repository, on a branch with the name of the
  printer. It never overwrites changes in the repository.
- The branch has a `README.md` that tells what it is and how to restore the printer. printerkeep writes it.
  It is not in the config folder. If your config folder has its own `README.md`, printerkeep backs up your
  file and adds none.
- When the token expires, `printerkeep status` and the log tell you. Make a new token, then run
  `printerkeep change-token`. printerkeep uses the new token only if the repository accepts it.
- With an SSH URL (`git@github.com:<user>/<repository>.git`), printerkeep makes its own SSH key and shows the
  public key. Add it to the repository as a deploy key with write access.

### Restore a printer

For example after a new SD card: install Klipper, Moonraker and printerkeep, then:

```sh
printerkeep restore https://github.com/<user>/<repository>.git
```

printerkeep writes the files of the backup into the config folder, and continues the history. It never
removes files. If a file in the config folder is different from the backup, printerkeep stops and lists the
files. Add `--overwrite` to replace them. If the repository has backups of more printers, add the printer
name: `printerkeep restore <url> <printer name>`. Restart Moonraker and Klipper after a restore.

### Settings

printerkeep needs no settings. `printer_data/config/printerkeep.conf` is optional:

```
[printerkeep]
# The name of the branch in the repository. The default is the host name.
printer_name: printer-1
# Written by add-repository and restore.
repository: https://github.com/<user>/<repository>.git
# Files that printerkeep does not back up, one rule on each line, like .gitignore rules.
ignore:
    *.csv
    timelapse/
# How often printerkeep checks the repository for changes: off, or a time such as 5m, 1h or 1d.
# The default is 5m (each run of the timer). printerkeep backup by hand always checks.
check_repository: 1h
```

A run with no changes writes nothing to the SD card: no backup, no log line. The time of the last check is in
memory (`/run/user/<id>`), not on the SD card.

Change `printer_name` only before the first backup that you send. A different name starts a different
history.

### Files that are not backed up

printerkeep always skips these files, in addition to your `ignore` rules:

- the copies that Klipper makes for `SAVE_CONFIG` (`printer-20261011_120000.cfg`) and its temporary
  `*_autosave` files,
- Moonraker's copy of its configuration (`.moonraker.conf.bkp`),
- `*.secrets` files: secrets must never go to a repository,
- `.git` folders of tools in the config folder,
- editor and system files (`*.swp`, `*.swo`, `*~`, `.DS_Store`, `__pycache__`, `*.pyc`),
- symbolic links. Tools such as KAMP come back when you install them again.

## Design

The design is in [`docs/`](docs/). Start with [problem.md](docs/problem.md).

## Development

```sh
nix develop          # optional: toolchain, cargo-zigbuild, zig, qemu, shellcheck (or direnv: .envrc)
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
shellcheck install.sh scripts/*.sh
```

Without Nix, [rustup](https://rustup.rs) installs the toolchain from `rust-toolchain.toml`.

## License

[MIT](LICENSE)
