# Language and platform

Status: **decided** (2026-10-11). Rust, with the system git command, for Linux hosts with systemd.

## Decision

- **Language: Rust.**
- **Git: the system `git` command.** printerkeep runs `git` for all repository work (objects, commits, fetch,
  push). printerkeep's own code does the merge output (C4–C6 in [solution-options.md](solution-options.md)).
- **Platforms in version 1: Linux with systemd** on aarch64, armv7, armv6 and x86_64. For example: Raspberry
  Pi OS, MainsailOS, Armbian (BTT CB1/CB2 and similar boards), Debian and Ubuntu.

## Facts behind the decision

### update_manager supports compiled programs

Moonraker's update_manager has an `executable` type. It downloads a release asset from GitHub. The installer
writes `release_info.json` next to the program. The `asset_name` field in that file selects the asset, so
the installer writes the asset name for the CPU of the printer (for example `printerkeep-aarch64`). Each
update downloads the same asset name. The Moonraker documentation uses
[klipper_estimator](https://github.com/Annex-Engineering/klipper_estimator), a Rust program, as its example.

Result: a Rust program can install and update through update_manager (S4) without workarounds. The
`executable` type uses GitHub releases. This applies to printerkeep's own releases only. The user's
configuration repository can be on any git host.

### Merges need only an old git

printerkeep merges each file separately. That needs only `git merge-file`, which exists in all git versions
that are in use. No part of the design needs `git merge-tree --write-tree` (git 2.38 or later). Thus, the git
version on the printer is not a problem.

The system `git`:

- uses the user's SSH configuration and keys, the same as any other git use on the printer,
- is installed on practically every Klipper host (KIAUH and update_manager both need it),
- avoids SSH and OpenSSL libraries in the Rust build, which make cross-compiling difficult.

printerkeep does not use git's conflict output. It makes its own output: the `#### printerkeep: needs
decision` block (C5) and the override rule (C6). This code is the core of printerkeep. It must have many
tests. Rust's type system and tests are a good fit for this.

### Platforms

| Host | CPU | Rust target (static, musl) |
|---|---|---|
| Raspberry Pi 3/4/5, CM4, 64-bit OS | aarch64 | `aarch64-unknown-linux-musl` |
| BTT CB1/CB2, other Armbian boards | aarch64 | `aarch64-unknown-linux-musl` |
| Raspberry Pi, 32-bit OS | armv7 | `armv7-unknown-linux-musleabihf` |
| Raspberry Pi Zero W (first generation) | armv6 | `arm-unknown-linux-musleabihf` |
| x86 mini PCs, virtual machines | x86_64 | `x86_64-unknown-linux-musl` |

All four targets have good Rust support. Static binaries need no libraries on the printer.

**Not in version 1:** hosts such as the Creality K1 series. They use MIPS CPUs (weak Rust support) and have
no systemd, so the timer and layerlock design (S1, S5) does not work there in any language. Support is
possible later, if users need it.

## Options that we did not select

| Language | Advantages | Why not |
|---|---|---|
| Python | Runs on every Klipper host (Klipper is Python). update_manager installs it from PyPI. The most possible contributors. | Weaker guarantees for the merge core. Must support many Python versions (3.7 to 3.12). Slower start on small hosts. |
| Go | Easy cross-compiling, also MIPS. Same model as layerlock. | No large advantage over Rust for version 1. The Rust preference decides. |

## Costs we accept

- A CI release pipeline that cross-compiles four targets and publishes the assets that the `executable` type
  needs. This is set up once, with standard tools (for example `cross` or `cargo-zigbuild`).
- An installer that selects the correct asset for the CPU and writes `release_info.json`.
- A dependency on the system `git`. printerkeep checks for it and gives a clear message if it is missing.
