# Changelog

What changes in each release of printerkeep. Read it before you update, so you know what to expect.

Fluidd and Mainsail link to the release page on GitHub, which shows the same notes.

## 0.6.0 (2026-10-15)

### New

- `copy-to-shared` lets you choose change by change. For each change, copy it to shared, keep it on this
  printer as a local change, or mark it as an override so that it never leaves this printer. So a printer
  that fixes a macro no longer sends its own macro values along with the fix.
- `connect` has clearer messages. When klipper-backup is still set up, it tells you that printerkeep makes the
  backups from now on, and shows how to remove klipper-backup.
