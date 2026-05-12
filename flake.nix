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
    let
      # Per-system outputs (packages, devShells, checks).
      perSystem = flake-utils.lib.eachDefaultSystem (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };

          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" "llvm-tools-preview" ];
          };

          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

          # Common source filtering — Rust/Cargo files plus per-crate src/assets/
          # so admin-ui's bundled CSS (and any future static assets) survive
          # the sandboxed build.
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (craneLib.filterCargoSources path type)
              || (pkgs.lib.hasInfix "/src/assets/" path);
            name = "source";
          };

          # RustSec advisory database for cargo-audit.
          advisoryDb = pkgs.fetchzip {
            url = "https://github.com/RustSec/advisory-db/archive/refs/heads/main.tar.gz";
            hash = "sha256-ehYI2+TnkdX86YRk+38K8ulZTDyifBbUTFub7IQ9vSk=";
          };

          # Common build inputs.
          commonArgs = {
            inherit src;
            pname = "underwrap";
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
            underwrapd = mkCrate "frontend-underwrapd";
            underwrap-exec = mkCrate "frontend-exec";
            underwrap-ns = mkCrate "frontend-ns";
            underwrap-debug-tool = mkCrate "frontend-debug-tool";
            underwrap-backendd = mkCrate "backend-backendd";
            underwrap-secretsd = mkCrate "frontend-secretsd";
            underwrap-admin-ui = mkCrate "backend-admin-ui";
          };

          checks = {
            # Workspace-wide cargo test.
            # Binary-dependent integration tests skip gracefully in sandbox.
            workspace-test = craneLib.cargoTest (commonArgs // {
              inherit cargoArtifacts;
            });

            # Faster, better test runner for local/CI use.
            workspace-nextest = craneLib.cargoNextest (commonArgs // {
              inherit cargoArtifacts;
              partitions = 1;
              partitionType = "count";
            });

            # Clippy.
            workspace-clippy = craneLib.cargoClippy (commonArgs // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });

            # Formatting.
            workspace-fmt = craneLib.cargoFmt {
              inherit src;
              pname = "underwrap";
              version = "0.1.0";
            };

            # RustSec advisory database checks.
            workspace-audit = craneLib.cargoAudit {
              inherit src;
              advisory-db = advisoryDb;
              # RUSTSEC-2023-0071 affects RSA decryption timing. underwrap uses
              # ssh-key for unlock-key signature verification with public keys,
              # not RSA private-key decryption. Keep the audit gate enabled but
              # suppress this specific advisory until the dependency graph moves.
              cargoAuditExtraArgs = "--ignore yanked --ignore RUSTSEC-2023-0071";
            };

            # Supply-chain policy checks.
            # Keep this lightweight and policy-free for now; audit handles
            # advisories separately and license policy can be added later via deny.toml.
            workspace-deny = craneLib.cargoDeny {
              inherit src;
              pname = "underwrap";
              version = "0.1.0";
              cargoDenyChecks = "bans sources";
            };

            # Coverage summary / report generation.
            workspace-llvm-cov = craneLib.cargoLlvmCov (commonArgs // {
              inherit cargoArtifacts;
            });
          };

          devShells.default = craneLib.devShell {
            # Extra packages available in the dev shell.
            packages = with pkgs; [
              rust-analyzer
              cargo-watch
              cargo-edit
              cargo-expand
              cargo-nextest
              cargo-llvm-cov
              cargo-audit
              cargo-deny
              cargo-udeps
              cargo-mutants
            ];
          };
        }
      );

      # System-independent outputs.
      systemIndependent = {
        # NixOS module.
        nixosModules.default = ./nix/modules/underwrap.nix;
      };

      # NixOS VM test (x86_64-linux only since it needs KVM).
      vmTest =
        let
          system = "x86_64-linux";
          pkgs = import nixpkgs { inherit system; };

          spoofScript = pkgs.writeScript "spoof.py"
            (builtins.readFile ./nix/tests/spoof.py);

          ancestorAttackScript = pkgs.writeScript "ancestor-attack.py"
            (builtins.readFile ./nix/tests/ancestor-attack.py);

          testScript = builtins.readFile ./nix/tests/test.py;

        in {
          checks.${system} = (perSystem.checks.${system} or {}) // {
            vm-test = pkgs.testers.nixosTest {
              name = "underwrap-vm-test";

              nodes.machine = { pkgs, ... }: import ./nix/tests/node.nix {
                inherit pkgs;
                underwrapPackage = perSystem.packages.${system}.default;
                nixosModule = self.nixosModules.default;
              };

              testScript = ''
                import os
                os.environ["SPOOF_SCRIPT"] = "${spoofScript}"
                os.environ["ANCESTOR_ATTACK_SCRIPT"] = "${ancestorAttackScript}"
                os.environ["PYTHON3"] = "${pkgs.python3}/bin/python3"
                ${testScript}
              '';
            };
          };
        };

    in
      # Merge all outputs.
      perSystem
      // systemIndependent
      // vmTest;
}
