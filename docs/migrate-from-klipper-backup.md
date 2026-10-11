# Move from klipper-backup to printerkeep

This guide moves one printer from [klipper-backup](https://github.com/Staubgeborener/klipper-backup) to
printerkeep. The history of your backups stays. You need SSH access to the printer. You do not need a
different computer.

## What changes

klipper-backup keeps a copy of the config folder in its git repository, in the folder `printer_data/config`
of one branch (usually `main`). printerkeep keeps the files of the config folder at the top of a branch that
has the name of the printer.

With one git command, you make a printer branch from the history of klipper-backup. printerkeep then
continues that branch. The klipper-backup branch does not change. Your repository keeps it as an archive.

## Before you start

- **The repository.** Use the repository that klipper-backup uses now. printerkeep adds a branch to it.
- **A personal access token for printerkeep.** On GitHub: Settings > Developer settings > Personal access
  tokens > Fine-grained tokens > Generate new token. Repository access: "Only select repositories", select
  your backup repository. Permissions: "Contents", "Read and write". printerkeep asks for it in step 4.
- klipper-backup can continue to run until step 6. Its backups go to its own branch.

## 1. Install printerkeep

Install printerkeep as the [README](../README.md#install) tells. The installer makes the first backup on the
printer. printerkeep does not send it anywhere yet.

## 2. Find the printer name

printerkeep uses the host name of the printer as the printer name. The new branch must have this name.

```sh
name=$(printerkeep status | sed -n 's/^Printer name: //p')
echo "$name"
```

The commands in steps 3 and 4 use `$name`. If you log out before step 4, do this step again.

## 3. Make the printer branch from the klipper-backup history

klipper-backup keeps its copy of the repository in `~/config_backup`. Go there, and make sure that it has the
expected folder:

```sh
cd ~/config_backup
git status            # "On branch main" (or the branch_name in klipper-backup's .env)
ls printer_data       # "config"
```

Make the branch, and send it to the repository:

```sh
git subtree split --prefix=printer_data/config -b "$name"
git push origin "$name"
```

The push uses the login of klipper-backup. You do not type a token.

If git says `'subtree' is not a git command`, use these two commands in place of `git subtree split`. The
result is the same:

```sh
git branch "$name"
FILTER_BRANCH_SQUELCH_WARNING=1 git filter-branch --subdirectory-filter printer_data/config "$name"
```

If `ls` in `~/config_backup` does not show `printer_data`, your configuration is in a different folder (old
installations use `klipper_config`). Use that folder in `--prefix=` or in `--subdirectory-filter`.

## 4. Connect printerkeep to the repository

Use the URL of the repository, without a token:

```sh
cd
printerkeep add-repository https://github.com/<user>/<repository>.git
```

printerkeep asks for the token from "Before you start". The output is similar to this:

```
Connected to the repository https://github.com/<user>/<repository>.git.
Stored the token.
The repository has backups of printer-1. printerkeep continues that history.
The backups that were only on this printer are not sent. They stay on this printer as refs/printerkeep/kept/printer-1-1791759705.
Backup made: Changed printer.cfg
Sent 1 backup to the repository.
```

The backups that printerkeep made on the printer before this step are not lost. They stay on the printer. The
history in the repository is the klipper-backup history, with the backups of printerkeep after it.

The first backup of printerkeep can add files that klipper-backup did not back up, or remove files that
printerkeep does not back up (see [Files that are not backed up](../README.md#files-that-are-not-backed-up)). If you had your own `exclude`
patterns in klipper-backup's `.env`, put them in `printerkeep.conf`:

```
[printerkeep]
ignore:
    *.csv
    *.zip
```

## 5. Check the result

```sh
printerkeep status
```

It must show `All backups are in the repository.`

On GitHub, select the branch with the printer name. The history has the klipper-backup backups first, and the
printerkeep backups after them. To show this branch first on GitHub, make it the default branch: the
repository's Settings > General > Default branch.

## 6. Remove klipper-backup

Do this step when you are satisfied with printerkeep.

klipper-backup installed system services with `sudo`. Thus, some of these commands need `sudo`. They remove
parts of klipper-backup, not of printerkeep.

```sh
# The timer of klipper-backup (cron)
crontab -l | grep -v 'klipper-backup/script.sh' | crontab -
# The services of klipper-backup, if you installed them
sudo systemctl disable --now klipper-backup-on-boot.service klipper-backup-filewatch.service
sudo rm -f /etc/systemd/system/klipper-backup-on-boot.service /etc/systemd/system/klipper-backup-filewatch.service
sudo systemctl daemon-reload
# The program and its copy of the repository. Both contain the old token.
rm -rf ~/klipper-backup ~/config_backup
```

Then:

- Remove the `[update_manager klipper-backup]` section from `moonraker.conf` (in the Fluidd or Mainsail
  editor), and restart Moonraker.
- If your configuration has a macro that starts klipper-backup, remove it.
- Remove the old token on GitHub (Settings > Developer settings > Personal access tokens), if nothing else
  uses it.

## More printers

If klipper-backup had one branch for each printer (`branch_name` in its `.env`), do steps 1 to 6 on each
printer. On each printer, `~/config_backup` has that printer's branch. Other repository layouts are not in
this guide.
