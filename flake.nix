{
  description = "underwrap — wrapper-oriented secret brokerage for local command execution";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, crane, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Common source filtering — only include Rust/Cargo files.
        src = craneLib.cleanCargoSource ./.;

        # Common build inputs.
        commonArgs = {
          inherit src;
          strictDeps = true;
          buildInputs = [ ];
          nativeBuildInputs = [ ];
        };

        # Build workspace dependencies first (for caching).
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        # Build the full workspace — all crates.
        workspace = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
        });

        # Individual crate packages (share the same dep artifacts).
        mkCrate = name: craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = name;
          cargoExtraArgs = "--package ${name}";
        });

      in {
        packages = {
          default = workspace;
          underwrapd = mkCrate "underwrapd";
          underwrapctl = mkCrate "underwrapctl";
          underwrap-exec = mkCrate "underwrap-exec";
        };

        checks = {
          # Workspace-wide cargo test.
          workspace-test = craneLib.cargoTest (commonArgs // {
            inherit cargoArtifacts;
          });
          # Clippy.
          workspace-clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });
          # Formatting.
          workspace-fmt = craneLib.cargoFmt {
            inherit src;
          };
        };

        devShells.default = craneLib.devShell {
          # Extra packages available in the dev shell.
          packages = with pkgs; [
            rust-analyzer
            cargo-watch
            cargo-edit
            cargo-expand
          ];
        };
      }
    );
}
