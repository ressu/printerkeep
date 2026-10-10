# Problem definition

Status: agreed problem statement. It deliberately contains no repo layout, mechanism, naming or language
decisions. Solution proposals are checked against this document.

## Background

[klipper-backup](https://github.com/Staubgeborener/klipper-backup) is the common way to back up a Klipper
printer's configuration to git. It has two shortcomings:

- It commits on every run, even when nothing changed.
- It is one-way, printer → repo. Changes made in the repo don't find their way back to the printer.

People with several printers have a harder problem. Their printers share some configuration (macros, toolhead
files, feature configs) but are not identical. The common workaround, one git branch per printer, makes
shared changes painful to coordinate: improvements made on one printer never reach the others, and it's hard
to see how printers have drifted apart.

Configuration is edited in two places: on the printer (Fluidd/Mainsail, `SAVE_CONFIG`) and in the repo. Neither
side can be the only source of truth.

## Design principles

1. **Single-printer first.** Backing up one printer must be clean and simple. A one-printer user never meets
   multi-printer concepts (shared files, deviations, promotion).
2. **Novice defaults, power-user escape hatches.** The target user is someone following KIAUH-style guides,
   not necessarily someone who knows git.
3. **Zero-config locally.** After install, history is kept on the printer with no setup. A remote is optional
   and can be added later.
4. **Never constrain how users write their config.** No required structure, fragments or override
   conventions. Complexity needed to support that lives in code, not in the user's workflow.
5. **No Linux desktop, no local repo clone, minimal CLI.** In order of preference: (a) nothing to do, (b) a UI
   the user already has, where it comes cheaply (Fluidd/Mainsail file editor or notifications, the git host's
   web UI), (c) a simple command over SSH on the printer. Rich Fluidd/Mainsail integration is not a requirement.
   Every user action must be possible with only the printer and a browser or SSH client.
6. **Simplicity.** Not every wish gets built. A feature that adds complexity has to show its underlying need,
   and simpler ways to meet that need win. The tool has to make sense for people other than its authors.
7. **Problem before mechanism; vocabulary before names.**

## Sources of change

- A human editing files in Fluidd/Mainsail.
- Klipper `SAVE_CONFIG`: rewrites the autosave block in `printer.cfg` and leaves `printer-<timestamp>.cfg`
  backups.
- Other services writing into the config directory: Moonraker (`.moonraker.conf.bkp`, update_manager),
  KlipperScreen, Crowsnest, timelapse.
- (Multi-printer) A human committing to the repo, from a desktop or the git host's web UI.

## Scenarios

### Single printer (v1 must be excellent at these)

- **S1 Backup.** A change is recorded only if something actually changed, with a useful message. Triggered
  by a timer plus a manual "backup now".
- **S2 Add a remote later.** A user with local-only history adds a remote (any git host; GitHub is the
  likely one) with guided authentication.
- **S3 Restore.** The SD card dies; after a fresh Klipper install, the config is restored from the remote.
- **S4 Self-update** through Moonraker's update_manager (the Fluidd/Mainsail update panel).
- **S5 Busy printer.** Never disturb a running print. Gating via
  [layerlock](https://github.com/ressu/layerlock) (systemd `ExecCondition=`) must be possible.
- **S6 Noise and secrets.** Generated files are ignored by default. Secret paths (e.g. Moonraker's secrets
  file) are never synced.

### Multi-printer (layered on top; a one-time guided migration from single-printer is acceptable)

- **M1 Shared change from the repo** reaches every printer that opted in to that file, without overwriting
  printer-specific differences.
- **M2 Shared content edited on a printer** is a local experiment, a *deviation*. It is backed up as that
  printer's deviation and never spread to other printers automatically.
- **M3 Promotion** of a deviation to the shared version is an explicit act, possible without a desktop clone.
- **M4 Lasting variants.** A printer keeps a small difference from shared content and still receives later
  shared updates: merged if clean, held and flagged on overlap.
- **M5 Drift visibility.** For each printer and shared file: in sync / behind / deviated / both. Visible
  without a desktop: e.g. a report in the repo, a Moonraker notification, or a status command on the printer.
- **M6 Printer-specific change from the repo** lands on that printer only.
- **M7 Long offline.** A printer comes back after weeks and catches up correctly.
- **M8 Adopt a drifted file.** Start sharing a file that already exists, with differences, on several
  printers. Possible without a desktop clone.
- **M9 Resolve a held merge.** A shared update overlaps a printer's local change. The user understands what
  happened and picks or edits the result, using at most the Fluidd editor, the git host's web UI or a simple
  command on the printer. The printer's config stays valid until it's resolved: conflict markers never appear
  in a live file.

### Apply policy (v1, fixed)

Incoming changes are written to disk, Klipper is never restarted, and the user is notified. Klipper only
reads its config on restart, so writing files does not disturb a running print.

## What is shared

- Identical whole files.
- Nearly identical files with small per-printer differences.
- Each shared file is used by some of the printers. Printers opt in per file; there is no group concept.

## Core problem (multi-printer)

For every shared file a printer uses, the system must know three versions:

1. the printer's current local version,
2. the shared version it last took in (the *base*),
3. the latest shared version.

Backup records (1). Propagating a shared change is a three-way merge of (1) and (3) against (2). Drift compares
all three.

## Evidence from real configs

Source: a real four-printer setup backed up with klipper-backup, one branch per printer. The printers are
called 1–4 here.

- **Identical files exist**: `macros/reset.cfg` on three printers; `spoolman.cfg`, `temperature.cfg` and
  `ui-helpers.cfg` on pairs.
- **Line-ending noise**: printer 2's `macros/temperature.cfg` differs from printer 3's only by CRLF.
- **Feature-presence variants**: `ui-helpers.cfg` and `macros.cfg` differ by lines commented out where a
  printer lacks LEDs or a runout sensor (`# STATUS_HEATING`, `# _RUNOUT_ON`).
- **Value variants**: `KAMP_Settings.cfg` differs only in `variable_*` values and in which `[include]`s are
  commented out.
- **Improvements that never spread**: printer 4 has a better `END_PRINT` retract (KAMP `tip_distance`);
  printer 3 added `LOAD_FILAMENT`/`UNLOAD_FILAMENT`.
- **Same path, unrelated content**: printer 1's `macros/temperature.cfg` is a different file from the others;
  `core/host-mcu.cfg` differs in substance. Same path does not mean shared.
- **Same file, different path**: `macros.cfg` at the root on printer 1 vs `macros/macros.cfg` elsewhere.
- **Non-Klipper files worth backing up**: `ukam/` firmware build configs, `crowsnest.conf`,
  `octoeverywhere.conf`, `moonraker.conf`. All of them live *inside* the config directory. KAMP itself is a
  symlink to an external repo and is not backed up.
- **Nothing outside the config directory is backed up.** klipper-backup is configured with
  `backupPaths=( "printer_data/config/*" )`, and every branch contains only `printer_data/config/…`.
  System-level configuration is handled separately (configuration management).

The real variants (commented-out lines, changed values, added macros) are what line-level three-way merges
handle well.

## Decisions

| Wish | Real need | Decision |
|---|---|---|
| Deviations + merge-if-clean (M2, M4) | Printers differ a little and still get shared improvements | Done in the tool: a printer's copy = shared version + its local changes, three-way merged against its base. Overlaps are held and flagged. Users are not asked to restructure configs (e.g. "identical shared file + Klipper overrides" was rejected). |
| Groups of printers | Some things apply to only some printers | No groups. Each printer opts in per file. |
| Desktop tooling (M3, M5, M9) | See drift, promote, resolve overlaps | No desktop tooling required. The tool runs on the printer; its CLI over SSH is acceptable, but zero-touch or existing UIs are preferred where cheap. |
| Timer and inotify | Changes captured promptly | Timer plus manual "backup now". No inotify in v1. |
| Configurable apply/restart | Don't surprise a printing machine | Fixed for v1 (see above). |
| Single → multi migration | Grow from one printer to several | A one-time guided migration is acceptable. |
| Self-update via update_manager | Easy install and update | Required (S4). update_manager natively handles git repos and Python packages; a compiled binary needs extra distribution work. This is an input to the language decision. |

Further decisions:

- **Scope is exactly the printer's config directory** (Moonraker's config root, normally
  `~/printer_data/config`), recursively, and nothing outside it. The rule for users: if Fluidd/Mainsail shows
  it in the config view, it's backed up (unless ignored); if not, it's not this tool's business. Arbitrary
  extra paths (klipper-backup's `backupPaths`) are not supported. They would mean several roots, path mapping,
  file permissions (some need sudo to write) and restores touching system files. Files such as firmware build
  configs are covered by keeping them in the config directory.

- **Same path required for sharing.** A shared file sits at the same relative path on every printer that
  opts in. No path mapping.
- **Adopting a drifted file (M8)**: one printer's version becomes the shared version. Every other opted-in
  printer keeps its current differences as local deviations.
- **Symlinks are skipped.** External tools (KAMP, Shake&Tune, `mainsail.cfg`, …) are restored by reinstalling
  them.
- **Line endings are normalised** for comparison. CRLF-only differences never cause drift or commits.

## Future considerations

Not v1 and not designed for, but the model shouldn't rule them out.

- **Adopting changes from other users.** Copy-paste may be enough. It has the same shape as M8: an outside
  version becomes the shared source and local differences merge on top. The model shouldn't assume every
  shared version comes from one of the user's own printers.
- **Groups of printers**, if they emerge naturally.
- **inotify-triggered backup**, a configurable apply/restart policy, power-user CLI conveniences.

## Non-goals

- Central push automation to printers.
- Templating as a requirement.
- Depending on GitHub-specific features.
- Backing up anything outside the config directory (system files, `printer_data/systemd`, Moonraker's
  database, gcodes, logs). That belongs to installers, configuration management or other backup tools.

## Next steps

1. `docs/solution-options.md`: 2–3 candidate models, plus today's branch-per-printer setup as a baseline,
   scored against every scenario. Each option includes step-by-step walkthroughs of M3, M8 and M9 for a user
   with only a browser and SSH, and is walked through the real drift cases above.
2. `docs/naming.md`: user-facing vocabulary first, then project, crate, binary, config file, systemd unit
   and update_manager names, checked for clashes.
3. Confirm the language (Rust is the leaning) and plan the implementation.
