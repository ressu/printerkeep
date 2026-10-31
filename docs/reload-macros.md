# Reload macros after a sync

Status: **rejected for now** (2026-10-31). printerkeep doesn't call Kalico's `RELOAD_GCODE_MACROS`. Kalico
users read about the command in [several-printers.md](several-printers.md) and run it themselves. Terms follow
[naming.md](naming.md).

## The idea

After a sync writes changes from shared into the followed files, Klipper uses them only after a restart
([problem.md](problem.md), "How changes are applied"). Kalico has a G-code command, `RELOAD_GCODE_MACROS`,
that reloads macros without a restart. printerkeep could call it after each sync that changed a file, so the
user doesn't need to restart.

## What the command does

Checked in the Kalico source at commit `c127b54c3` (2026-10-24). The command was added in `9c2e494ad`
(2024-06-14). The code is `cmd_RELOAD_GCODE_MACROS` in `klippy/extras/gcode_macro.py`, and the description
is in `docs/G-Codes.md`.

- **Kalico only.** Mainline Klipper doesn't have the command.
- **Macro bodies only.** It reads the config files again and replaces the `gcode:` template of each
  `[gcode_macro]` section that is already loaded. It doesn't add new macros or remove deleted ones. It doesn't
  reload `variable_*` values, `description`, `rename_existing`, `[delayed_gcode]`, or any other section.
- **It can stop halfway.** It replaces the macros one at a time. When the template of one macro has an error,
  it raises an error partway through: the macros before it are new and the ones after it are old. A config
  that Klipper can't read fails before any change.
- **It works during a print.** A print that runs a macro later (for example `END_PRINT`) then runs the new
  version.

## What printerkeep would need

- A connection to the printer: Moonraker's `printer.gcode.script` over HTTP or a websocket, or Klipper's Unix
  socket (`klippy_uds`). printerkeep has no HTTP client (CLAUDE.md: no HTTP client crates), so this means a
  hand-written client, `curl`, or a second API to follow. Moonraker may also need authorization.
- A check for Kalico, because the command fails on Klipper.
- A check that no print is running, as layerlock does for backups (problem.md S5: printerkeep never disturbs
  a print).
- Timeouts, error handling, and log lines for each failure.
- Two decisions changed: problem.md says printerkeep "never restarts Klipper and tells the user", and
  implementation-plan.md (M7, item 6) dropped messages in the Klipper console partly because a backup tool
  would then send G-code to the printer.

## Decision

Rejected for now. The cost is a new runtime dependency on Moonraker or Klipper, with all the checks above. The
gain is small and only for Kalico users: most changes from shared touch more than macro bodies, so the printer
would run partly on the new config and partly on the old one. The user couldn't tell which parts are live,
and a restart would still be needed. A restart stays the one clear way to use the changes.

Instead, [several-printers.md](several-printers.md) tells Kalico users about `RELOAD_GCODE_MACROS`. They know
what changed, so they can choose it when only macro bodies changed.

## When to look again

- Klipper or Kalico adds a reload that applies the whole config, or none of it when there's an error.
- printerkeep gets a connection to Moonraker for another reason, for example notices in Fluidd and Mainsail
  (problem.md, "Future possibilities").
