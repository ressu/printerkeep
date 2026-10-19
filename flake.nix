{
  description = "printerkeep development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, rust-overlay, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      }));
    in
    {
      devShells = forAllSystems (pkgs:
        let
          # Same toolchain as rust-toolchain.toml, plus the release targets for local cross builds.
          rust = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
            targets = [
              "aarch64-unknown-linux-musl"
              "armv7-unknown-linux-musleabihf"
              "arm-unknown-linux-musleabihf"
              "x86_64-unknown-linux-musl"
            ];
          };
        in
        {
          default = pkgs.mkShell {
            packages = [
              rust
              pkgs.git
              pkgs.jujutsu
              pkgs.python3 # Klipper parser tests (M5)
              pkgs.qemu # run ARM release binaries locally
              pkgs.shellcheck # install.sh and scripts/
              pkgs.git-cliff # drafts the release notes in CHANGELOG.md
            ];
          };
        });
    };
}
