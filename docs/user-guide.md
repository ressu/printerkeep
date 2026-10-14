# printerkeep user guide

This guide is for a printer that uses printerkeep on its own. The [README](../README.md) has the short
version: what printerkeep is, how to install it, and the everyday commands. For printers that share files,
see [Several printers](several-printers.md).

## What the install sets up

printerkeep installs only for the printer's user, and changes nothing outside the home folder.
`printerkeep install` sets up these parts:

| Part | Where |
|---|---|
| The program, and `release_info.json` for Moonraker's update manager | `~/printerkeep/` |
| A link, so that the command is `printerkeep` | `~/.local/bin/printerkeep` |
| A timer that runs `printerkeep sync` every 5 minutes (systemd user units) | `~/.config/systemd/user/printerkeep.service`, `printerkeep.timer` |
| Updates in the update panel of Fluidd and Mainsail | `[update_manager printerkeep]` in `moonraker.conf` |

systemd stops a user's timers when the user logs out, unless "lingering" is on for that user.
`printerkeep install` turns lingering on with `loginctl enable-linger`. If the system doesn't allow that,
printerkeep shows the command for an administrator.

If [layerlock](https://github.com/ressu/layerlock) is installed, backups wait until the print is done.
Without it, backups run at a low priority, also during a print.

## Other ways to install

The [README](../README.md#install) shows the install script in one command. You can also check the program
first, or install without the script. The three ways differ only in how you download the program and how you
check it.

### The install script, in one command

```sh
curl -fsSL https://github.com/ressu/printerkeep/releases/latest/download/install.sh | sh
```

The script downloads the program for your CPU, compares it with the checksum file of the release, and runs
`printerkeep install`. The checksum file comes from the same place as the program. So this check finds a
damaged download, but it can't prove that the program is genuine. For that, use the next way.

You can set these variables before `sh`, for example `curl … | PRINTER_NAME=printer-2 sh`:

| Variable | Default | Description |
|---|---|---|
| `VERSION` | latest | The release to install, for example `v0.5.0` |
| `BINARY_SHA256` | none | The checksum of the program. The script then checks against it instead of the checksum file. |
| `PRINTER_DATA` | `~/printer_data` | The printer_data folder |
| `PRINTER_NAME` | `main` | The printer name, which is also its branch in the repository (see [Printer name](#printer-name)) |

### The install script, read and checked first

```sh
curl -fsSL https://github.com/ressu/printerkeep/releases/download/v0.5.0/install.sh -o install.sh
less install.sh
VERSION=v0.5.0 BINARY_SHA256="sha256:<checksum>" sh install.sh
```

Replace `<checksum>` with the checksum of the program for your CPU. The
[release page](https://github.com/ressu/printerkeep/releases) shows a `sha256:` checksum for each file. The
script installs the program only if its checksum matches.

### Without a script

Find the file for your CPU. The command `uname -m` shows which one you need:

| `uname -m` | File |
|---|---|
| `aarch64` | `printerkeep-linux-aarch64` |
| `armv7l` | `printerkeep-linux-armv7` |
| `armv6l` | `printerkeep-linux-armv6` |
| `x86_64` | `printerkeep-linux-x86_64` |

Then download it, check it, and install it. For an `aarch64` printer:

```sh
curl -fsSLO https://github.com/ressu/printerkeep/releases/download/v0.5.0/printerkeep-linux-aarch64
sha256sum printerkeep-linux-aarch64      # compare with the checksum on the release page
chmod +x printerkeep-linux-aarch64
./printerkeep-linux-aarch64 install      # copies itself to ~/printerkeep
rm printerkeep-linux-aarch64
```

## Update and remove

- **Update:** use the update panel in Fluidd or Mainsail. Restart Moonraker once after the install, so that
  the panel shows printerkeep.
- **Update by hand:** run the install command again, in any of the three ways. It replaces the program and
  the timer. Your history, login, and settings stay.
- **After an update in the update panel:** the panel replaces only the program. When a release changes the
  timer, `printerkeep status` tells you, and you run `printerkeep install` once to update it.
- **Remove:** run `printerkeep uninstall`. It keeps the history, the login for the repository, and the
  settings file.

## Backups and the timer

The timer runs `printerkeep sync` every 5 minutes. A sync does three things:

1. It makes a backup when a file changed.
2. It writes the changes from the repository into the config folder (and, with several printers, the changes
   from shared).
3. It sends new backups to the repository.

The history is on the printer, in `printer_data/printerkeep`. Each backup has a message that says which files
changed, for example `Changed printer.cfg, added macros/new.cfg` or `SAVE_CONFIG changed printer.cfg`.

```sh
printerkeep sync       # all of it now: a backup, the changes, send
printerkeep backup     # a backup and send only; it never changes your files
printerkeep status     # the last backup, the changes since then, and the repository
```

You need `sync` by hand only to get the changes now. When the repository has changes on the printer's
branch, `backup` keeps its backup on the printer and tells you to run `sync`.

printerkeep writes what it does to `printer_data/logs/printerkeep.log`, which Fluidd and Mainsail show with
the other logs. A run in which nothing changed writes nothing, so quiet runs don't wear the SD card.

When something goes wrong, for example when the repository can't be reached, the log says what happened and
what to do. It says so once, and again when the problem is solved. Until then, `printerkeep status` shows the
problem.

## Send backups to a repository

A repository keeps a copy of your backups away from the printer, for example for when the SD card fails.
Make an empty repository on a git host, such as GitHub, GitLab, Gitea, or Codeberg. It can be public, so
that others can see your configuration, or private. printerkeep never sends `*.secrets` files (see
[Files that printerkeep skips](#files-that-printerkeep-skips)). Then connect the printer to it:

```sh
printerkeep connect https://github.com/<user>/<repository>.git
```

Replace the URL with the address of your repository.

printerkeep asks for an access token: a password for one program, which you make on the git host. To make
one on GitHub:

1. Go to Settings > Developer settings > Personal access tokens > Fine-grained tokens > Generate new token.
2. Under "Repository access", select "Only select repositories", and then the repository.
3. Under "Permissions", give "Contents" the access "Read and write".

On other hosts, make an access token that can read and write the repository. If the host needs your user
name with the token, put it in the URL: `https://<user name>@<host>/<owner>/<repository>.git`. You can also
use an SSH URL instead of a token (see below).

If the repository already has printers, printerkeep lists them and asks which one this printer is. Type its
name, or type a new name to start a new branch.

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
- printerkeep never overwrites changes in the repository. When the repository has changes on the printer's
  branch, the next sync takes them first.
- A repository that you made with a README or a license (GitHub's "Add a README file" option) works too.
  printerkeep continues from it.
- printerkeep adds a `README.md` to the branch, which says what the branch is and how to restore the printer.
  This file is in the repository only, not on the printer. If your config folder has its own `README.md`,
  printerkeep backs up yours instead.
- When the token expires, `printerkeep status` and the log tell you. Make a new token, then run the command
  below. printerkeep switches to the new token only if the repository accepts it.

  ```sh
  printerkeep change-token
  ```

- With an SSH URL (`git@github.com:<user>/<repository>.git`), printerkeep makes its own SSH key and shows the
  public key. Add it to the repository as a deploy key with write access.
- When the repository refuses what printerkeep sends, for example because GitHub's push protection found
  something that looks like a secret, printerkeep shows the repository's message. It names the file, and
  GitHub's message has a link to allow the secret.

## Move from klipper-backup

Use the same repository:

1. Remove klipper-backup from the printer (see the steps below). If it keeps running, it sends its backups
   to the same branch.
2. Make a new token for printerkeep.
3. Run `printerkeep connect`, as described in [Send backups to a repository](#send-backups-to-a-repository).
4. Select the klipper-backup branch of this printer: `main`, or the `branch_name` that you gave
   klipper-backup.
5. Answer yes to continue its history.

In a script, give the answers as options:

```sh
printerkeep connect <url> --printer-name <branch> --continue
```

printerkeep then continues the klipper-backup history. The files move from `printer_data/config` to the top
of the branch, and the older backups stay in the history. git follows the files across the move, so the
history of each file is complete.

### Remove klipper-backup

klipper-backup has no uninstall command. Its installer asks about each part, so your printer may have only
some of these. Removing one that isn't there does no harm.

1. Stop its services, if you installed them. They're system services, so these commands need `sudo`:

   ```sh
   sudo systemctl disable --now klipper-backup-filewatch.service klipper-backup-on-boot.service
   sudo rm -f /etc/systemd/system/klipper-backup-filewatch.service /etc/systemd/system/klipper-backup-on-boot.service
   sudo systemctl daemon-reload
   ```

2. Remove its cron job, which makes a backup every 4 hours:

   ```sh
   crontab -l | grep -v 'klipper-backup/script.sh' | crontab -
   ```

3. In `moonraker.conf`, delete the `[update_manager klipper-backup]` section, for example in the Fluidd or
   Mainsail editor. Then restart Moonraker.
4. Delete its folder. It has klipper-backup's token in the `.env` file:

   ```sh
   rm -rf ~/klipper-backup
   ```

5. On the git host, delete klipper-backup's token, so that it can't be used any more.

## Go back to an earlier version

Each backup keeps every file, so you can bring back an earlier version. Give the file and the time:

```sh
printerkeep restore printer.cfg --at "2 days ago"
```

printerkeep takes the version that the file had at that time. If it made several backups of the file on that
day, it lists them, and you pick one. Without `--at`, printerkeep lists the earlier versions of the file.
Without a file name, it restores all files.

`--at` takes times such as `"2 days ago"`, `yesterday`, `2026-10-12`, or `"2026-10-12 14:00"`. A date alone
means the end of that day.

Before it writes anything, printerkeep makes a backup of the files as they are now, so you can always go
back. The output gives the exact command for that. After the restore, printerkeep makes a backup that says
what it restored, and sends it to the repository, if there's one.

A restore adds and replaces files; it never removes any. It takes `printerkeep.conf` from an older backup
only when you name that file. It always keeps the current `.printerkeep-state.json`.

To take files from another repository, add `--repository <url>`. This printer stays connected to its own
repository.

## Restore a printer

For example after a new SD card, install Klipper, Moonraker, and printerkeep. Then run:

```sh
printerkeep restore --repository https://github.com/<user>/<repository>.git
```

If the repository has several printers, printerkeep asks which one this is. In a script, add
`--printer-name <printer name>`.

printerkeep connects the printer to the repository, writes the files of that printer's last backup into the
config folder, and continues its history. The backups that were only on this printer, such as the first
backup that the install made, are set aside. To restore an earlier backup, add `--at`.

The `README.md` on each printer's branch in the repository shows this command, with the URL and the printer
name filled in. So you can find it there when the printer is gone.

Restart Moonraker and Klipper after a restore.

## Change files in the repository

You can edit a printer's files in the repository, for example in GitHub's web editor, on the printer's
branch. printerkeep checks the repository at each sync (see `check_repository` in [Settings](#settings)) and
writes your changes into the config folder. When the printer changed the same file, printerkeep merges both
changes. When both changed the same lines, the file needs a decision (see
[Needs decision](several-printers.md#needs-decision)).

printerkeep keeps the files in the config folder that you remove in the repository. The log tells you about
each one, so that you can remove it on the printer too.

## Settings

printerkeep works without settings. To change what it does, use the settings file,
`printer_data/config/printerkeep.conf`:

```
[printerkeep]
# The printer name, which is also its branch in the repository. The default is main.
# To change it, run printerkeep rename <name> (see "Printer name"), because the history must move too.
printer_name: printer-1
# Written by connect and restore.
repository: https://github.com/<user>/<repository>.git
# Files that printerkeep skips, one rule per line, like .gitignore rules.
ignore:
    *.csv
    timelapse/
# How often printerkeep checks the repository for changes: off, or a time such as 5m, 1h, or 1d.
# The default is 5m, that is, each run of the timer. printerkeep sync by hand always checks.
check_repository: 1h
```

printerkeep keeps the time of the last repository check in memory (`/run/user/<id>`), not on the SD card.

## Printer name

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
  `printerkeep connect <url>` and select that printer. In a script, add
  `--printer-name <branch> --continue`.

## Files that printerkeep skips

printerkeep always skips these files, in addition to your `ignore` rules:

- the copies that Klipper makes for `SAVE_CONFIG` (`printer-20261011_120000.cfg`), and its temporary
  `*_autosave` files,
- Moonraker's copy of its configuration (`.moonraker.conf.bkp`),
- `*.secrets` files, because secrets must never go to a repository,
- `.git` folders of tools in the config folder,
- editor and system files (`*.swp`, `*.swo`, `*~`, `.DS_Store`, `__pycache__`, `*.pyc`),
- symbolic links. Tools such as KAMP come back when you install them again.

To skip more files, add them to `ignore` in the settings file. The next backup records a skipped file as
removed; the file stays on the printer, and the older backups keep it.
