# printerkeep

printerkeep keeps the configuration of your Klipper printer safe in git. It also keeps more printers in step:
printers can follow shared files and still keep their own changes.

printerkeep works with Klipper and with firmware derived from it, such as Kalico.

> **Status: in development.** printerkeep does not work yet. Release 0.1 replaces
> [klipper-backup](https://github.com/Staubgeborener/klipper-backup) for one printer.

## What printerkeep does

- Makes a backup of the config folder only when a file changed.
- Keeps the history on the printer. A repository (for example on GitHub) is optional.
- Restores a printer from the repository.
- Updates itself through Moonraker's update manager.
- From release 0.2: shared files for more printers, with local changes on each printer.

## Design

The design is in [`docs/`](docs/). Start with [problem.md](docs/problem.md).

## Development

```sh
nix develop          # optional: toolchain, cargo-zigbuild, zig, qemu (or direnv: .envrc)
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Without Nix, [rustup](https://rustup.rs) installs the toolchain from `rust-toolchain.toml`.

## License

[MIT](LICENSE)
