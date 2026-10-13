# Solution options

Status: **decided: option A + common core** (2026-10-11). We checked each option against
[problem.md](problem.md). Terms follow the vocabulary in [naming.md](naming.md). Names of commands and
files are in [naming.md](naming.md).

Most of the design does **not** depend on the shape of the repository. This document describes that part
once, as the common core. The options are different only in how they organise the repository.

## Common core (all options)

### C1. printerkeep keeps git data outside the config folder

The git data is outside the config folder (for example `~/.local/share/printerkeep/repo.git`). printerkeep
uses the config folder as the work tree only to read files for a backup. It does all merges in memory or in a
separate index. It writes a file into the config folder only when it has a final, valid result for that file.

- There is no `.git` folder in `printer_data/config`. Moonraker, Fluidd and Mainsail never see git data.
- Git conflict markers can never get into a live file, because printerkeep never runs `git merge` or
  `git pull` in the work tree.
- Experts can still use git: `git --git-dir=… --work-tree=…`, or a `printerkeep git …` passthrough.

### C2. A backup happens only when a file changed

1. Read the config folder into a git tree. Apply the ignore rules (the defaults, plus optional rules in the
   settings file). Ignore symlinks. Ignore line-ending differences.
2. If the tree is the same as in the last backup: stop. No commit, no push, and no write to the SD card. From
   0.2, a run checks the repository for changes with `git ls-remote` (no write), at most as often as
   `check_repository` says (default: each run). See M5 in implementation-plan.md.
3. If not: make a commit. The message lists the changed files, for example
   `printer.cfg, macros/macros.cfg changed`. If only the `SAVE_CONFIG` block changed, the message says so.

The default ignore rules (in `src/ignore.rs`) are: Klipper's `SAVE_CONFIG` copies
(`<name>-YYYYMMDD_HHMMSS.cfg`) and its temporary `*_autosave` file, Moonraker's `.<name>.bkp` copy of its
config file, `*.secrets`, `.git/` folders, and editor and system files (`*.swp`, `*~`, `.DS_Store`,
`__pycache__/`). The names of the Klipper and Moonraker files were checked in their source code. State files
of extensions (`variables.cfg`, `mmu_vars.cfg`) are backed up (see "Future possibilities" in problem.md).

### C3. Follow a shared file: settings file + state

Each printer has two files:

- **Settings file** (the user edits it). It is in the config folder, so Fluidd can edit it. It lists the files
  that this printer follows from shared, and the few optional settings (repository, more ignore rules). A user
  with one printer never needs to open it.
- **State** (printerkeep writes it). It is the file `.printerkeep-state.json` in the config folder, so each
  backup includes it on the printer's branch. For each followed file it records the **base**: the shared commit with the version of the file that this printer
  took last.

A followed file has the same path in shared and on the printer.

### C4. Changes from shared: a three-way merge per file

printerkeep does this for each followed file where the shared version is newer than the base:

| Printer's file = base? | Printer's file = shared version? | Result |
|---|---|---|
| yes | – | Write the shared version. base := new. |
| – | yes | Nothing to write (for example, this printer copied the file to shared). base := new. |
| no | no | Three-way merge. If the changes touch different lines: write the result, base := new. If they touch the same lines: the file **needs decision** (C5). |

A long time offline (M7) needs nothing special. printerkeep does one three-way merge from the base to the
current shared version. The number of shared commits between them does not matter.

When printerkeep writes a file, it keeps the line endings that the file has.

### C5. Needs decision: the user decides in the file

When a change from shared and a local change touch the same lines, printerkeep writes the parts that merge.
At each place where the changes touch the same lines, it keeps **this printer's lines active** and puts the
**shared lines immediately below, commented out**. Comment lines in plain language mark the place:

```
[gcode_macro _KAMP_Settings]
variable_mesh_margin: 5
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
variable_purge_amount: 40
```

- **One file, always valid.** The printer works as before. printerkeep added only comments. The user decides
  in the place where they would look anyway: the Fluidd editor.
- **No `####` lines = decided.** When the file has no more marker lines, printerkeep records the decision:
  base := the shared version that caused the decision. The lines the user kept are now the printer's local
  changes. The user types no command. Command for experts:
  `printerkeep decide <file> --use-shared|--keep-mine`.
- **While a file needs decision,** printerkeep applies no more changes from shared to it. The next change
  waits until the user decides. printerkeep refuses to copy a file with marker lines to shared.
- **The backup is the file on disk.** printerkeep backs up the file with the marker lines. The history shows
  one commit when the decision starts and one when the user decides. Both versions and the decision are in
  the history. On the git host, the user can see the decision in the file. A restore brings back the open
  decision. printerkeep never removes marker lines from a backup, because then the backup would be different
  from the disk.
- The marker text must never match user content, and printerkeep must find it easily. The `printerkeep:`
  token does this.
- A shared line gets a `#` with no space after it. When the user removes the `#`, the line is exactly the
  shared line. With `# `, the line would start with a space, and Klipper would read it as a part of the
  option above it (tested, M5).

### C6. Override: lines that never travel

The problem: printer 2 follows a shared filament sensor file and copies it to shared to send a macro
improvement. Printer 2's pin lines also go to shared. Printer 1 never changed its pin line. Thus, the
three-way merge on printer 1 sees the pins as a change from shared and **writes printer 2's pins on
printer 1**. When printerkeep copies a full file to shared, it cannot know which lines are for this printer
only.

The solution: an optional inline comment that marks a line as an **override**:

```
[filament_switch_sensor runout]
switch_pin: ^PG12   # printerkeep: override
```

- **A merge always keeps the printer's override line. Copy to shared never copies it.**
- The shared version can have a clear placeholder (`switch_pin: CHANGE_ME   # printerkeep: override`). When a
  new printer starts to follow the file, Klipper's config check fails clearly. The printer does not use the
  pins of a different printer.
- An override is a normal Klipper comment. Files that do not need it never see it. It is optional, not a
  required structure.
- Local changes without an override mark can go to shared. `printerkeep copy-to-shared` shows the changes
  that it will send and lists the override lines that it does not send.
- Decided: an override marks one line. A begin/end form for more lines is possible later, if real
  configurations need it.
- Later idea: printerkeep can suggest an override when the same line is different on most printers that
  follow the file.

### C7. Status and the README of shared

Each followed file has one status: *up to date*, *local changes*, *changes waiting* or *needs decision*.
`printerkeep status` on the printer shows it, and the log file `printer_data/logs/printerkeep.log` (Fluidd and
Mainsail show it) gets a line when something happens.

Decided 2026-10-13: the status is **not** in the repository. A status page that all printers write needs
coordination between printers (races, retries, a commit on `shared` for each status change). The `shared`
branch has a static `README.md`: what the repository is, and how to restore a printer. Users find the printer
branches with the branch list of the git host. If users need a page with all printers later, revisit this. A first step is a command that shows the
status of all printer branches (see "Future possibilities" in problem.md). A message in the Klipper console (for example with `RESPOND`) or a
Moonraker notification is possible later. See the open questions.

### C8. Repository, restore, update

- **S2 Add a repository:** one command on the printer, `printerkeep add-repository <url>`. The URL selects
  the authentication:
  - `https://…`: a **personal access token** (for GitHub: a fine-grained token for this one repository, with
    "Contents: read and write"). printerkeep asks for the token, or reads it from standard input. One token
    can serve all printers, so there are no keys to manage per printer.
  - `git@…` or `ssh://…`: an **SSH key**. printerkeep makes a key and shows the public key, with instructions
    to add it as a deploy key.

  printerkeep keeps the token with git's own `store` credential helper, in
  `printer_data/printerkeep/credentials` (mode 600), outside the config folder. **A token or key never goes in
  `printerkeep.conf`**: that file is in the config folder, so the repository would get it. When
  authentication fails (for example, an expired token), the status says so and tells the user to run
  `add-repository` again.
- **S3 Restore:** install printerkeep, then `printerkeep restore <url> [<printer name>]`. This needs a token or
  a key, the same as S2.
- **S4 Update printerkeep:** an update_manager entry. How depends on the language (git repository or compiled
  release). It does not depend on the option.
- **S5 Busy printer:** printerkeep runs as a systemd service with a timer. layerlock connects through
  `ExecCondition=`. The manual start is `printerkeep backup` (optionally also a Klipper macro through a
  shell-command extension). Each run makes a backup and applies changes from shared.

## Options

### Option 0: the current setup

One branch per printer, kept by klipper-backup. People copy shared changes manually (cherry-picks and copy and
paste over SSH or the web pages). This option is here only for comparison.

### Option A: one branch per printer + a `shared` branch

```
branch <printer-1>   = printer 1's config folder, 1:1 (files at the top)
branch <printer-2>   = printer 2's config folder
branch shared        = the shared files, at the same paths as on the printers
                       + a static README.md: what the repository is, how to restore (C7)
```

"Shared" means *available to follow*, not *used by every printer*. Each printer follows only the files it
needs (C3). For example, two printers follow the nevermore file and two other printers follow the LED file:

```
shared:     nevermore.cfg  stealthburner_leds.cfg  macros/macros.cfg  features/filament.cfg
printer 1:  nevermore.cfg  stealthburner_leds.cfg  macros/macros.cfg  features/filament.cfg
printer 2:                 stealthburner_leds.cfg  macros/macros.cfg  features/filament.cfg
printer 3:  nevermore.cfg                          macros/macros.cfg
```

Shared has one version per path. If files with similar names have different content (printer 1's
`temperature.cfg`), the printers do not follow them. To share both, rename one of them once.

- One printer is exactly what users expect from klipper-backup: their configuration, at the top of the
  repository, on one branch. No migration for more printers: the second printer gets its own branch. The
  `shared` branch appears when the first file is copied to shared.
- Printers push only to their own branch. Only copy to shared and status updates change `shared`. Each
  status update changes one file per printer, so pushes at the same time only need a rebase.
- Each printer has its own, clean history.
- GitHub *compare* (`shared...printer-1`) shows how a printer's shared files are different. But it also shows
  every file that is not shared.
- Disadvantages: beginners do not find branches easily ("where is my other printer?"). The README on
  `shared` (the default branch when there are more printers) tells that each printer has a branch. Copy to shared
  in the web pages needs a branch change between copy and paste.

Why branches are not a problem here: today, branches are difficult because people coordinate them manually.
In option A, printerkeep never merges printer branches into each other. The three-way merge per file (C4)
does all coordination.

### Option B: one branch, a folder per printer + `shared/`

```
printers/<printer-1>/…   = printer 1's config folder
printers/<printer-2>/…
shared/…                 = shared files, same paths
README.md                = what the repository is (C7)
```

- All printers are on one page. Easy to find.
- Copy to shared in the web pages is copy and paste on one branch.
- One printer gets an extra folder level (`printers/<name>/printer.cfg`). The alternative, files at the top
  for one printer, needs a migration for more printers.
- All printers push to the same branch: fetch, rebase, try again. Conflicts can occur only in a printer's own
  folder (a web edit at the same time as a printer change). This is possible to solve, but more complex.
- The history mixes all printers. GitHub can show the history of one folder, but the default view is mixed.
- The web pages cannot show the difference between `shared/x` and `printers/a/x`.

### Option C: one repository per printer + a shared repository

- One printer is the same as option A.
- Shared is a second repository. Printers read from it, and copy to shared writes to it.
- The most separation. It fits the future possibility "changes from other people" (another person's
  repository is one more source).
- More authentication: on GitHub, a deploy key works for one repository only. Each printer needs two keys or a
  user token. That doubles the setup in S2 and S3.
- Every option can support "changes from other people" later: a line in the settings file can name another
  repository as the source. Thus, this is not an advantage of option C only.

### Rejected

- **The config folder is a git checkout (`.git` in `printer_data/config`).** `git pull` would write git
  conflict markers into live files. Moonraker and Fluidd would see `.git`. C1 gives all the advantages without
  these risks.
- **Store local changes as patches** (`printers/b/macros.cfg.patch`). The repository would not show the files
  that are on the printer, and a restore would need printerkeep. This does not work as a backup.
- **Real git merges of the shared branch into printer branches.** A merge brings *every* shared file, so a
  printer cannot follow only some files. Copy to shared would need a merge of a full printer branch into
  shared. A merge per file with a recorded base (C3, C4) is simpler and does exactly what is necessary.

## Scenario comparison

✓ = works well, ~ = works with difficulty, ✗ = does not work. "Core" = the common core does it the same way
for A, B and C.

| Scenario | 0 Current | A Branches + shared | B Folders | C Repositories + shared |
|---|---|---|---|---|
| S1 Backup only on change | ✗ empty commits | ✓ core | ✓ core | ✓ core |
| S2 Add a repository later | ~ edit `.env` | ✓ core | ✓ core | ~ two repositories to authorise |
| S3 Restore | ~ manual | ✓ check out branch | ✓ check out folder | ✓ clone repository |
| S4 Update printerkeep | ✓ | ✓ core | ✓ core | ✓ core |
| S5 Busy printer | ~ | ✓ core | ✓ core | ✓ core |
| S6 Noise and secrets | ~ manual list | ✓ core | ✓ core | ✓ core |
| Simple with one printer | ✓ | ✓ files at top, one branch | ~ extra folder (or migration) | ✓ files at top |
| One → more printers | – | ✓ no migration | ✓ no migration (if folders from the start) | ~ make and authorise a 2nd repository |
| M1 Changes from shared | ✗ manual | ✓ core | ✓ core | ✓ core |
| M2 Printer edit stays local | ~ | ✓ core | ✓ core | ✓ core |
| M3 Copy to shared | ✗ manual | ✓ 1 command; web: copy between branches | ✓ 1 command; web: copy on one branch | ~ 1 command; web: copy between repositories |
| M4 Lasting local changes | ✗ | ✓ core | ✓ core | ✓ core |
| M5 Status | ✗ | ✓ status command + compare view | ✓ status command | ~ in two repositories |
| M6 Printer's file changed in the repository | ~ manual pull | ✓ edit printer branch | ✓ edit `printers/<x>/` | ✓ edit printer repository |
| M7 Long offline | ~ | ✓ core | ✓ core (+ push races) | ✓ core |
| M8 Share a file that is already different | ✗ | ✓ see below | ✓ see below | ~ |
| M9 Needs decision | ✗ | ✓ core (Fluidd) | ✓ core (Fluidd) | ✓ core (Fluidd) |
| Find things in the web pages | ~ | ~ branch changes | ✓ one tree | ~ many repositories |
| History per printer | ✓ | ✓ | ~ mixed | ✓ |
| Pushes at the same time | ✓ | ✓ own branch | ~ one branch, rebase and retry | ✓ |
| Future: changes from other people | ✗ | ✓ source = repository/branch | ✓ | ✓ |

## Step by step (only a browser and SSH to the printer)

### M3: copy printer 1's better `macros/macros.cfg` to shared

**Options A and B, on the printer (1 command):** `ssh printer-1`, then
`printerkeep copy-to-shared macros/macros.cfg`. The shared version becomes printer 1's version. Printer 1's
base moves to it. The other printers that follow the file merge it at their next run.

**Options A and B, web pages only (0 commands):** open printer 1's `macros/macros.cfg` (option A: on printer
1's branch; option B: in `printers/printer-1/`) and copy it. Edit the shared file (option A: on the `shared`
branch; option B: `shared/macros/macros.cfg`), paste and commit. At printer 1's next run, the shared version is
the same as its own version. Thus, C4 says "nothing to write" and the base moves. Printer 1 needs no more
steps.

**Option C:** the same as with web pages, but the paste goes into the other repository.

### M8: start to share `macros/macros.cfg`, which is already different on three printers

**Options A and B, on the printers (1 command on each):**
1. On the printer whose version becomes the shared version: `printerkeep copy-to-shared macros/macros.cfg`.
   This makes the shared file and makes the printer follow it (up to date).
2. On each other printer: `printerkeep follow macros/macros.cfg`. There is no base yet, so base := the current
   shared version. The printer's current differences become its local changes. No file on the printer
   changes.

Total: 3 commands on 3 printers.

**Options A and B, Fluidd and web pages only (0 commands):**
1. Web pages: make the shared file from one printer's version (paste).
2. On each printer, in Fluidd: add `macros/macros.cfg` to the list of followed files in the settings file.
   printerkeep sees it at the next run and uses the same rule: no base → base := current shared version.

Check with real data: printer 3's version becomes the shared version of `macros/macros.cfg`. Printer 4 keeps
its commented-out `STATUS_*` lines and its better retract as local changes. When printer 3 adds something
later, the change merges on printer 4, unless it touches the same lines. Printer 4 can copy its better
retract to shared (M3). Then printer 2 gets it as a change from shared.

### M9: needs decision on printer 2

1. The status (the log file in Fluidd, `printerkeep status`) shows `KAMP_Settings.cfg: needs decision`.
2. In Fluidd, open `KAMP_Settings.cfg`. The `####` block shows this printer's active line and the shared line,
   commented out, below it.
3. Keep one line. Delete the `####` lines. Save.
4. At the next run, printerkeep records the decision. 0 commands.
   Command for experts: `printerkeep decide KAMP_Settings.cfg --use-shared|--keep-mine`.

This is the same for options A, B and C.

### Copy to shared with override lines (C6)

Printer 1 and printer 3 follow the filament sensor file. Their pin lines have `# printerkeep: override`.
Printer 3 improves the runout macro and runs `printerkeep copy-to-shared features/filament.cfg`. The macro
change goes to shared. The pin lines do not. Printer 1 merges the macro change and keeps its pins.

## Real differences, checked

- **Only line endings are different** (printer 2 and printer 3, `temperature.cfg`): printerkeep ignores line
  endings, so the status is up to date. The file keeps its CRLF on disk.
- **Same path, different file** (printer 1, `macros/temperature.cfg`): printer 1 does not follow that file.
  Each printer follows each file separately, so nothing happens automatically.
- **Different `KAMP_Settings` values:** if the printers follow the file, each printer's `variable_*` values are
  local changes. A shared change on a different line merges. A shared change on the *same* `variable_` line
  needs decision. That is correct, because a person must decide.
- **Same file, different path** (printer 1, `macros.cfg` at the top): printer 1 moves the file once, changes
  its `[include]` and then follows the file like the other printers.

## Decision

**Option A (one branch per printer + a `shared` branch) with the common core.** Needs decision uses comments
in the file (C5). Lines for one printer only use the override mark (C6).

- It is the best for one printer: a klipper-backup user sees their configuration at the top of the
  repository, with no new terms and no migration later.
- Printers push only to their own branch. Each history stays clean. The compare view in the web pages shows
  some differences at no cost.
- The difficult part of branches (manual coordination) is exactly what the core does automatically.

Accepted disadvantage compared to B: it is more difficult to find things on the git host. The README on the
`shared` branch helps. The core does not depend on the shape of the repository. Thus, B stays available if A
is a problem in practice.

## Open questions (check before the implementation)

1. Can a different service show a notification in Fluidd or Mainsail through Moonraker? Or is the Klipper
   console (`RESPOND`) the only easy channel? This changes C7, not the option.
2. Solved in M5: Klipper's and Kalico's parsers ignore the C5 blocks and the override comments, also in
   multi-line `gcode:` values (`tests/klipper/check_markers.py`).
3. Default ignore rules: find what Moonraker, KlipperScreen, Crowsnest, timelapse, Spoolman and others write
   into the config folder.
4. Decided in [language.md](language.md): the system `git` command. Merges per file need only
   `git merge-file`, which all git versions in use have.
5. Decided in [naming.md](naming.md): the state is `.printerkeep-state.json` in the config folder, so it is on
   the printer's branch.
