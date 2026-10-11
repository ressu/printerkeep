# Solution options

Status: **decided: option A + common core** (2026-10-11). Evaluated against [problem.md](problem.md). Names in `<angle brackets>` are
placeholders until [naming](naming.md) is done.

Working through the scenarios showed that most of the design does **not** depend on repo shape. That part is
described once, as the common core. The options then differ only in how the repo is organised.

## Common core (all options)

### C1. The printer agent owns a separate git directory

The git metadata lives outside the config directory (e.g. `~/.local/share/<tool>/repo.git`). The config
directory is used as the work tree for *snapshots* only. All merges happen in memory or in a scratch index.
The agent writes a file into the config directory only when it has a final, clean result for it.

- No `.git` inside `printer_data/config`, so Moonraker's file manager, Fluidd and Mainsail never see it.
- Conflict markers can never reach a live file, because the porcelain `git merge`/`git pull` never touch the
  work tree.
- Power users can still use git: `git --git-dir=… --work-tree=…`, or a `<tool> git …` passthrough.

### C2. Backup = snapshot, commit only if the tree changed

1. Build a tree from the config directory, applying the ignore rules (built-in defaults plus an optional user
   file). Skip symlinks. Normalise line endings for comparison.
2. If the tree equals the last commit's tree: stop. No commit, no push, no network traffic.
3. Otherwise commit with a message that lists the changed files (e.g.
   `printer.cfg, macros/macros.cfg changed`). Note `SAVE_CONFIG` when only the autosave block changed.

Built-in ignores cover at least `printer-*.cfg` (SAVE_CONFIG backups), `.moonraker.conf.bkp`, the Moonraker
secrets file, and logs or caches that tools leave in the config directory.

### C3. Following a shared file: manifest + state

Two files per printer:

- **Manifest** (human-edited, lives in the config directory so Fluidd can edit it, e.g. `<tool>.conf`). It
  lists which paths this printer *follows* from the shared source, plus the few optional settings (remote,
  extra ignores). A single-printer user never needs to open it.
- **State** (machine-written, in the agent's data directory and mirrored into the repo). For each followed
  path it records the **base**: the shared commit whose version of that file this printer last took in.

Following uses the same path on both sides (decided in problem.md).

### C4. Propagation = per-file three-way merge

For every followed file where the shared version moved past the base:

| local == base | shared == local | Result |
|---|---|---|
| yes | – | Fast-forward: write shared, base := new |
| – | yes | Already there (e.g. this printer was the source of a promotion): base := new |
| no | no | `merge-file(local, base, shared)`. Clean → write result, base := new. Overlap → **held** (C5) |

Long offline periods (M7) need nothing special: one three-way merge from base to latest, however many shared
commits lie between.

When writing back, keep the file's existing line endings.

### C5. Held merges: decided in the file itself, as comments

When a merge overlaps, the agent writes the clean parts of the merge and, at each overlap, keeps **this
printer's lines active** and puts the **shared lines right below them, commented out**. Marker comments in
plain language frame the overlap:

```
[gcode_macro _KAMP_Settings]
variable_mesh_margin: 5
#### SYNC: decision needed ##############
#### The shared version changed this too.
#### Active now (this printer):
variable_purge_margin: 20
#### Shared version (to use it, remove
#### the # from the next line and delete
#### the line above):
# variable_purge_margin: 12
#### When done, delete all #### lines.
#### END SYNC ############################
variable_purge_amount: 40
```

- **One file, always valid.** The printer behaves exactly as before; only comments were added. The user
  decides where they would look anyway, in the Fluidd editor.
- **Resolution = no markers left.** On a later run with no `#### SYNC` blocks in the file, the agent records
  the resolution: base := the shared version the hold was made from. Whatever the user kept is now the
  printer's deviation. Zero commands. CLI shortcut: `<tool> resolve <path> --take-shared|--keep-mine`.
- **While held**, the file gets no further shared merges; the next one waits until it's resolved. Promoting a
  file that still has markers is refused.
- **Backups stay honest.** The snapshot commits the file as it is on disk, markers included. History shows
  one commit when the hold appears and one when it's resolved, recording both alternatives and the choice. On
  the git host, the held state can be read in the file itself. A restore brings the pending decision back
  intact. The markers are never stripped from backups: that would make the backup differ from the disk.
- The marker token must be distinctive enough that it never matches user content and is easy for the agent to
  detect. The exact wording is part of naming.

### C5b. Lines that never travel (per-printer values, "placeholders")

Problem found while walking through promotion. Printer 2 promotes a shared filament-sensor file to share a
macro fix. Printer 2's pin lines go into the shared version as well. Printer 1 never changed its pin line, so
its three-way merge sees the pins as a shared update and **silently writes printer 2's pins onto printer 1**.
Whole-file promotion can't tell lasting per-printer lines from improvements meant for everyone.

Fix: an optional inline marker that makes a line printer-local:

```
[filament_switch_sensor runout]
switch_pin: ^PG12   # <tool>:local
```

- **Merges always keep the printer's version of a local line. Promotion never copies it into the shared
  version.**
- The shared version can carry an obvious placeholder (`switch_pin: CHANGE_ME   # <tool>:local`). A printer
  that starts following the file then fails Klipper's config check loudly instead of using someone else's pins.
- It's plain, valid Klipper config: an inline comment. Files that don't need it never see it. This is opt-in
  convention, not imposed structure.
- Unmarked deviations are still treated as candidates for promotion. `<tool> promote` shows the diff it is
  about to share and lists the local lines it is leaving out.
- Decided: per-line marker only. A begin/end block form may be added later if real configs need it.
- Later idea: the agent could *suggest* marking a line when that line deviates on most followers.

### C6. Status and drift

On every run the agent writes a short human-readable status:

- into the config directory (e.g. `<tool>-status.md`, readable in Fluidd), and
- into the repo, where the hub page (per option) shows every printer.

Each followed file is reported as one of: *in sync*, *behind*, *deviated* or *held*. There is also
`<tool> status` on the printer. A Klipper console message (e.g. via `RESPOND`) or a Moonraker notification is
a later nice-to-have; see open questions.

### C7. Remote, restore, update

- **S2 Add a remote**: one command on the printer (`<tool> remote <url>`). It creates an SSH key and prints the
  public key with instructions for adding it as a deploy key. Zero-SSH variant: put the URL in the manifest
  from Fluidd; the agent writes the public key into the status file.
- **S3 Restore**: install, then `<tool> restore <url> [<printer>]`. This needs a key, the same as S2.
- **S4 Self-update**: an update_manager entry. How depends on the language decision (git repo vs binary
  release); it doesn't depend on the option chosen here.
- **S5 Busy printer**: the agent runs as a systemd service plus timer. Layerlock plugs into `ExecCondition=`.
  The manual trigger is `<tool> backup` (and optionally a Klipper macro via a shell-command extension).

## Options

### Option 0: baseline (today)

One branch per printer, maintained by klipper-backup. Shared changes are propagated by hand (cherry-picks and
copy-paste over SSH or the web UI). Included only to keep score.

### Option A: branch per printer + a `shared` branch

```
branch <printer-1>   = printer-1's config dir, 1:1 (files at the root)
branch <printer-2>   = printer-2's config dir
branch shared        = the pool of shared files, at the same paths they have on printers
                       + hub README with every printer's status (C6)
```

"Shared" means *available in the pool*, not *used by every printer*. Each printer follows only the files it
uses (C3). For example, nevermore config can be followed by two printers and LED config by two others:

```
pool:       nevermore.cfg  stealthburner_leds.cfg  macros/macros.cfg  features/filament.cfg
printer 1:  nevermore.cfg  stealthburner_leds.cfg  macros/macros.cfg  features/filament.cfg
printer 2:                 stealthburner_leds.cfg  macros/macros.cfg  features/filament.cfg
printer 3:  nevermore.cfg                          macros/macros.cfg
```

The pool has one shared version per path. Similarly named files with different content (printer 1's
`temperature.cfg`) simply aren't followed, or are renamed once if they should be shared separately.

- A single printer is exactly what users expect from klipper-backup: their config, at the repo root, on one
  branch. No migration to multi-printer: a second printer gets its own branch, and `shared` appears the first
  time something is shared.
- Printers only ever push to their own branch. `shared` changes only through promotion, adoption and status
  updates. Status updates touch one file per printer, so pushes that race simply rebase.
- Each printer's history stays clean and its own.
- GitHub *compare* (`shared...printer-1`) shows how a printer's shared files differ. It's noisy, though,
  because every non-shared file shows up as well.
- Downsides: branches are less discoverable for novices ("where is my other printer?"). The hub README on
  `shared` (made the default branch once you go multi-printer) links to every printer branch. Promoting via the
  web UI means switching branches between copy and paste.

Note on today's pain: coordinating branches is annoying because it's manual today. In option A, printer
branches are never merged into one another. All coordination is the per-file merge in C4, done by the agent.

### Option B: one branch, a directory per printer + `shared/`

```
printers/<printer-1>/…   = printer-1's config dir
printers/<printer-2>/…
shared/…                 = shared files, same relative paths
README.md                = hub status (C6)
```

- Everything is visible on one page; good for discoverability.
- Promoting via the web UI is copy-and-paste on the same branch.
- A single printer gets an extra directory level (`printers/<name>/printer.cfg`). The alternative, a root
  layout for a single printer, needs a migration when going multi-printer.
- Every printer pushes to the same branch: fetch, rebase, retry. Content conflicts can only happen inside a
  printer's own directory (a web edit racing a printer change), so this is resolvable but more moving parts.
- History mixes all printers. GitHub can filter history by path, but the default view is noisy.
- No diff view in the web UI between `shared/x` and `printers/a/x`.

### Option C: a repo per printer + a shared repo

- A single printer is identical to option A.
- Sharing means a second repo the printers read from (and promotion writes to).
- Most isolation, and it fits the "adopt from other users" future (another person's repo is just another
  source).
- Auth friction: on GitHub a deploy key belongs to exactly one repo, so each printer needs two keys or a
  user-level token. That doubles the setup in S2/S3. Drift spans two repos, so there's no single hub view.
- Any option can get the "other users' sources" future later by letting a manifest entry name an external
  repo/ref as its source. Option C's advantage there is not unique.

### Considered and rejected

- **Config dir is a git checkout (`.git` in `printer_data/config`)**: `git pull` would write conflict markers
  into live files. Moonraker and Fluidd would see `.git`. C1 keeps every benefit without these risks.
- **Storing deviations as patches** (`printers/b/macros.cfg.patch`): the repo would no longer show what's
  actually on the printer, and restore would need the tool. This fails the backup purpose.
- **Real git merges of a shared branch into printer branches**: a merge brings in *every* shared file, which
  breaks per-file opt-in. Promotion would have to merge a whole printer branch into shared. Per-file merges
  with recorded bases (C3/C4) are simpler and do exactly what's needed.

## Scenario matrix

✓ = handled well, ~ = works with friction, ✗ = not handled. "Core" means the common core covers it the same way
for A, B and C.

| Scenario | 0 Baseline | A Branches + shared | B Monorepo dirs | C Repos + shared repo |
|---|---|---|---|---|
| S1 Backup, commit only on change | ✗ empty commits | ✓ core | ✓ core | ✓ core |
| S2 Add remote later | ~ edit `.env` | ✓ core | ✓ core | ~ two repos to authorise once sharing |
| S3 Restore | ~ manual | ✓ checkout branch | ✓ checkout subdir | ✓ clone repo |
| S4 Self-update | ✓ | ✓ core | ✓ core | ✓ core |
| S5 Busy printer | ~ | ✓ core | ✓ core | ✓ core |
| S6 Noise & secrets | ~ manual list | ✓ core | ✓ core | ✓ core |
| Single-printer simplicity | ✓ | ✓ root layout, one branch | ~ extra dir level (or migration) | ✓ root layout |
| Single → multi | – | ✓ no migration | ✓ no migration (if dirs from day one) | ~ create + authorise 2nd repo |
| M1 Shared change reaches printers | ✗ manual | ✓ core | ✓ core | ✓ core |
| M2 Printer edit stays local | ~ | ✓ core | ✓ core | ✓ core |
| M3 Promotion | ✗ manual | ✓ 1 cmd; web: copy across branches | ✓ 1 cmd; web: copy within branch | ~ 1 cmd; web: copy across repos |
| M4 Lasting variants | ✗ | ✓ core | ✓ core | ✓ core |
| M5 Drift visibility | ✗ | ✓ hub README + compare view | ✓ hub README | ~ split over repos |
| M6 Repo-side printer change | ~ manual pull | ✓ edit printer branch | ✓ edit `printers/<x>/` | ✓ edit printer repo |
| M7 Long offline | ~ | ✓ core | ✓ core (+ push races) | ✓ core |
| M8 Adopt drifted file | ✗ | ✓ see walkthrough | ✓ see walkthrough | ~ |
| M9 Resolve held merge | ✗ | ✓ core (Fluidd) | ✓ core (Fluidd) | ✓ core (Fluidd) |
| Web UI browsing | ~ | ~ branch switching | ✓ one tree | ~ many repos |
| Per-printer history | ✓ | ✓ | ~ interleaved | ✓ |
| Push concurrency | ✓ | ✓ own branch | ~ shared branch, rebase/retry | ✓ |
| Future: other users' sources | ✗ | ✓ source = repo/ref | ✓ | ✓ |

## Walkthroughs (browser + SSH to the printer only)

### M3: promote printer 1's improved `macros/macros.cfg`

**Options A/B, on the printer (1 command):** `ssh printer-1`, `<tool> promote macros/macros.cfg`. The
shared version becomes printer 1's version, printer 1's base moves to it, and the other followers merge it in
on their next run.

**Options A/B, web UI only (0 commands):** open printer 1's `macros/macros.cfg` (option A: on printer 1's
branch; option B: under `printers/printer-1/`) and copy it. Edit the shared copy (option A: on the `shared`
branch; option B: `shared/macros/macros.cfg`), paste and commit. On printer 1's next run the shared version
equals its local version, so per C4 it is "already there" and its base moves up. No extra step on printer 1.

**Option C:** same as the web flow, but the paste goes into the other repo.

### M8: start sharing `macros/macros.cfg`, which has drifted on three printers

**Options A/B, on the printers (1 command each):**
1. On the printer whose version should become the shared one: `<tool> share macros/macros.cfg`. It writes
   the shared version and follows it (in sync).
2. On each other printer: `<tool> follow macros/macros.cfg`. With no base recorded yet, base := the current
   shared version, so this printer's existing differences become its deviation. Nothing on the printer
   changes.

Total: 3 commands across 3 printers.

**Options A/B, Fluidd + web UI only (0 commands):**
1. Web UI: create the shared copy from printer X's version (paste).
2. On each printer, Fluidd: add `macros/macros.cfg` to the manifest's follow list. The agent picks it up on its
   next run with the same "no base → base := current shared" rule.

Real data check: following `macros/macros.cfg` with printer 3 as the source, printer 4 keeps its
commented-out `STATUS_*` lines and better retract as its deviation. If printer 3 later adds something, it
merges into printer 4 cleanly unless it touches the same lines. Printer 4's better retract can be promoted
with M3, and printer 2 then receives it as a merge.

### M9: a held merge on printer 2

1. Status (Fluidd status file, hub README) shows `KAMP_Settings.cfg: decision needed`.
2. In Fluidd, open `KAMP_Settings.cfg`. The `#### SYNC` block shows this printer's active line and the shared
   line commented out below it.
3. Keep one, then delete the `####` lines. Save.
4. On the next run the agent records the resolution. 0 commands.
   CLI shortcut: `<tool> resolve KAMP_Settings.cfg --take-shared|--keep-mine`.

This is identical across options A, B and C.

### Promotion with local lines (C5b)

The filament sensor file is followed by printer 1 and printer 3, and the pin lines are marked
`# <tool>:local`. Printer 3 improves the runout macro and runs `<tool> promote features/filament.cfg`. The
macro change goes into the shared version; the pin lines don't. Printer 1 merges the macro change cleanly and
keeps its pins.

## Real drift cases

- **CRLF-only difference** (printer 2 vs printer 3 `temperature.cfg`): equal after normalisation, so it's in
  sync. The file keeps its CRLF on disk.
- **Same path, unrelated content** (printer 1 `macros/temperature.cfg`): printer 1 simply doesn't follow that
  file. The manifest is per path, per printer, so nothing happens automatically.
- **KAMP_Settings value variants**: if followed, each printer's `variable_*` values are its deviation. A shared
  change to a different line merges cleanly. A shared change to the *same* `variable_` line is held, and that
  is correct, because it's a real decision.
- **Same file, different path** (printer 1 `macros.cfg` at the root): printer 1 moves the file once and updates
  its `[include]`, then follows it like the others.

## Decision

**Option A (branch per printer + `shared` pool branch) on top of the common core.** Held merges use the
in-file commented form (C5); per-printer lines use the inline `local` marker (C5b).

- It is the best single-printer experience: a klipper-backup user sees their config at the repo root, with no
  extra concepts and no migration later.
- Printers push only to their own branch, histories stay clean, and the web compare view gives some drift
  inspection for free.
- What used to be annoying about branches (manual coordination) is exactly what the core automates.

Accepted cost compared with B: discoverability on the git host. This is mitigated by the hub README on the
`shared` branch. Because the core doesn't depend on repo shape, B stays a fallback if this turns out wrong in
practice.

## Open questions / to verify before implementation

1. Can a third-party service raise a notification in Fluidd/Mainsail via Moonraker, or is the Klipper console
   (`RESPOND`) the only cheap channel? This affects C6, not the choice of option.
2. Confirm with tests against Klipper's config parser that C5 marker blocks and commented alternatives are
   inert everywhere they can appear, including inside multi-line `gcode:` values (real configs already have
   commented lines there).
3. Ignore defaults: inventory what Moonraker, KlipperScreen, Crowsnest, timelapse, Spoolman, etc. write into
   the config directory.
4. Git on the printer: rely on system `git` (versions vary across MainsailOS/Pi OS releases; `merge-tree
   --write-tree` needs ≥ 2.38) or embed a git library. This feeds into the language decision.
5. Where state (C3) lives in the repo for option A: on the printer's branch (e.g. a dotfile at the root of the
   branch, also kept out of Klipper's way) or on `shared` next to the status.
