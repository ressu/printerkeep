# Move from klipper-backup to printerkeep

This guide moves one printer from [klipper-backup](https://github.com/Staubgeborener/klipper-backup) to
printerkeep. You need SSH access to the printer. You do not need a different computer.

## Start again, or keep the history

**Start again.** This is the easy way. Do step 1 and step 3, then install printerkeep and give it a new, empty
repository, as the [README](../README.md#install) tells. The old repository stays on GitHub as an archive.

**Keep the history.** Do all the steps. printerkeep then continues the history of klipper-backup.

klipper-backup keeps a copy of the config folder in the folder `printer_data/config` of one branch, usually
`main`. printerkeep keeps the files of the config folder at the top of a branch that has the name of the
printer. One git command makes a printer branch from the klipper-backup history. The klipper-backup branch
does not change. It stays in the repository as an archive.

## Before you start

- **A personal access token for printerkeep.** On GitHub: Settings > Developer settings > Personal access
  tokens > Fine-grained tokens > Generate new token. Repository access: "Only select repositories", select
  your backup repository. Permissions: "Contents", "Read and write". You need it in step 4.

## 1. Stop klipper-backup

klipper-backup installed system services with `sudo`. Thus, some of these commands need `sudo`. They stop
klipper-backup. Its files stay until step 3, because step 2 uses them.

```sh
# The timer of klipper-backup (cron)
crontab -l | grep -v 'klipper-backup/script.sh' | crontab -
# The services of klipper-backup, if you installed them
sudo systemctl disable --now klipper-backup-on-boot.service klipper-backup-filewatch.service
sudo rm -f /etc/systemd/system/klipper-backup-on-boot.service /etc/systemd/system/klipper-backup-filewatch.service
sudo systemctl daemon-reload
```

If your configuration has a macro that starts klipper-backup, remove it.

## 2. Make the printer branch from the klipper-backup history

klipper-backup keeps its copy of the repository in `~/config_backup`. Go there, and make sure that it has the
expected folder:

```sh
cd ~/config_backup
git status            # "On branch main" (or the branch_name in klipper-backup's .env)
ls printer_data       # "config"
```

printerkeep uses the host name of the printer as the printer name. Make the branch with that name, and send
it to the repository:

```sh
name=$(hostname)
git subtree split --prefix=printer_data/config -b "$name"
git push origin "$name"
```

The push uses the login of klipper-backup. You do not type a token.

The new branch has fewer backups than the klipper-backup branch. It has only the backups that changed the
config folder. klipper-backup also made backups with no changes ("No new changes pushed").

If git says `'subtree' is not a git command`, use these two commands in place of `git subtree split`. The
result is the same:

```sh
git branch "$name"
FILTER_BRANCH_SQUELCH_WARNING=1 git filter-branch --subdirectory-filter printer_data/config "$name"
```

If `ls` does not show `printer_data`, your configuration is in a different folder (old installations use
`klipper_config`). Use that folder in `--prefix=` or in `--subdirectory-filter`.

### If the klipper-backup branch already has the printer name

Some people gave klipper-backup one branch for each printer, with the name of the printer. Then git refuses
to make the branch: `branch '<name>' is not an ancestor of commit`. Copy the klipper-backup branch to
`klipper-backup/<name>` first, then replace the branch with the new history:

```sh
git branch -m "$name" klipper-backup
git push origin "klipper-backup:refs/heads/klipper-backup/$name"
git subtree split --prefix=printer_data/config -b "$name"
git push --force-with-lease origin "$name"
```

Do not change the order. The second command keeps the klipper-backup history in the repository before the
last command replaces the branch.

## 3. Remove klipper-backup

```sh
cd
rm -rf ~/klipper-backup ~/config_backup
```

Both folders contain the old token. Then:

- Remove the `[update_manager klipper-backup]` section from `moonraker.conf` (in the Fluidd or Mainsail
  editor), and restart Moonraker.
- Remove the old token on GitHub (Settings > Developer settings > Personal access tokens), if nothing else
  uses it.

## 4. Install printerkeep and connect it to the repository

Install printerkeep as the [README](../README.md#install) tells. Then connect it to the repository. Use the
URL of the repository, without a token:

```sh
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

The install made the first backup on the printer. That backup is the one that stays on the printer. The
history in the repository is the klipper-backup history, with the backups of printerkeep after it.

The first backup of printerkeep can add files that klipper-backup did not back up, or remove files that
printerkeep does not back up (see [Files that are not backed up](../README.md#files-that-are-not-backed-up)).
If you had your own `exclude` patterns in klipper-backup's `.env`, put them in `printerkeep.conf`:

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
printerkeep backups after them. GitHub still shows the klipper-backup branch first. To show the printer
branch first, make it the default branch: the repository's Settings > General > Default branch.

## More printers

Do the steps on each printer, with that printer's branch. If more printers sent backups to the same
klipper-backup branch, that history mixes the printers. Then start again on those printers.
