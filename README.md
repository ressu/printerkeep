# printerkeep

printerkeep keeps the configuration of your Klipper printer safe in git. It also keeps more printers in step:
printers can follow shared files and still keep their own changes.

printerkeep works with Klipper and with firmware derived from it, such as Kalico.

> **Status: in development.** printerkeep does not work yet. Release 0.1 replaces
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
