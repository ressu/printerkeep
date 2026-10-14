# printerkeep

printerkeep keeps a history of your Klipper printer's configuration, so you can always go back to a version
that worked. It runs on the printer by itself. You don't need git skills or a second computer.

It works with Klipper and with firmware based on it, such as Kalico.

> **Status:** early releases (0.x). Commands and files can still change.

## Why printerkeep

You change your printer's configuration all the time: in the Fluidd or Mainsail editor, with `SAVE_CONFIG`
after a calibration, or by adding a macro from someone else. Sooner or later a change breaks something, and
you no longer remember what the file looked like before. Or the SD card fails, and the configuration is gone.

printerkeep is for those moments:

- **Every change is saved.** printerkeep checks the config folder every 5 minutes and makes a backup when a
  file changed. Each backup says which files changed, for example `SAVE_CONFIG changed printer.cfg`.
- **Going back is one command.** Bring back one file or all of them, as they were at a time you name, for
  example "2 days ago".
- **A copy away from the printer.** Connect a git repository, for example on GitHub, and printerkeep sends
  each backup there. After a new SD card, one command puts everything back.
- **Edits go both ways.** You can also change a file in the repository, for example in GitHub's web editor.
  printerkeep writes the change into the printer's config folder.
- **It stays out of your way.** It never restarts Klipper. It runs at a low priority, and with
  [layerlock](https://github.com/ressu/layerlock) it waits until a print is done. A run in which nothing
  changed writes nothing, so it doesn't wear the SD card. It updates through the update panel in Fluidd and
  Mainsail.

If you use [klipper-backup](https://github.com/Staubgeborener/klipper-backup), printerkeep can continue its
history in the same repository.

## Install

Connect to the printer with SSH, as the user that runs Klipper (often `pi`). Then run:

```sh
curl -fsSL https://github.com/ressu/printerkeep/releases/latest/download/install.sh | sh
```

The script downloads printerkeep for your printer, checks the download, and sets it up for your user, in
your home folder. When it's done, it makes the first backup and shows what it set up.

Then restart Moonraker once, so that the update panel in Fluidd and Mainsail shows printerkeep. Use the power
menu in Fluidd or Mainsail, or run:

```sh
sudo systemctl restart moonraker
```

That's all. From now on, printerkeep makes the backups by itself.

To update printerkeep later, use the update panel in Fluidd or Mainsail, or run the same install command
again.

To check the program before you run it, or to install without a script, see
[other ways to install](docs/user-guide.md#other-ways-to-install).

## Use

These commands cover what you need for one printer. Run them on the printer, over SSH.

### See what printerkeep is doing

```sh
printerkeep status
```

It shows the last backup, the files that changed since then, and the repository. When something needs your
attention, such as a token that expired, `status` says what to do. printerkeep also writes what it does to
`printerkeep.log`, which Fluidd and Mainsail show with the other logs.

### Make a backup now

The timer makes a backup every 5 minutes. To make one now, for example before you try something, run:

```sh
printerkeep backup
```

`backup` only saves and sends. It never changes your files.

### Send the backups to a repository

A repository keeps a copy of your backups away from the printer. Make an empty repository on a git host, such
as GitHub, GitLab, Gitea, or Codeberg. It can be public, so that others can see your configuration, or
private, to keep it to yourself. Then run:

```sh
printerkeep connect https://github.com/<user>/<repository>.git
```

Replace the URL with the address of your repository. printerkeep asks for an access token, and shows how to
make one on GitHub. With an SSH URL, it makes a key for you instead. From then on, each backup goes to the
repository too. The [user guide](docs/user-guide.md#send-backups-to-a-repository) has the details for other
hosts.

To get the changes from the repository now, instead of at the next run of the timer, run:

```sh
printerkeep sync
```

### Go back to an earlier version

To bring back a file as it was at some time, run:

```sh
printerkeep restore printer.cfg --at "2 days ago"
```

Without `--at`, printerkeep lists the earlier versions of the file, and you pick one. Without a file name, it
restores all files. Your files from before the restore stay in the history, and the output gives the command
that brings them back.

### Restore the printer after a new SD card

Install Klipper, Moonraker, and printerkeep again. Then run:

```sh
printerkeep restore --repository https://github.com/<user>/<repository>.git
```

printerkeep writes the files of the last backup into the config folder and continues the history. Then
restart Moonraker and Klipper. You don't need to remember this command: the README on the printer's branch in
the repository shows it, with the URL and the printer name filled in.

### More

The [user guide](docs/user-guide.md) has the details: the settings file, the printer name, the files that
printerkeep skips, SSH keys, moving from klipper-backup, updates, and how to remove printerkeep.

## Several printers

printerkeep can keep several printers in one repository, each with its own branch. Printers can also share
files, such as macros: a change on one printer reaches the others, and each printer keeps its own
differences, such as its pins. See [Several printers](docs/several-printers.md).

## Design and development

The design is in [`docs/`](docs/). Start with [problem.md](docs/problem.md). To build and test:

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
