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
            extensions = [ "rust-src" "rust-analyzer" ];
          };

          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

          # Common source filtering — only include Rust/Cargo files.
          src = craneLib.cleanCargoSource ./.;

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
            underwrapd = mkCrate "underwrapd";
            underwrapctl = mkCrate "underwrapctl";
            underwrap-exec = mkCrate "underwrap-exec";
          };

          checks = {
            # Workspace-wide cargo test.
            # Binary-dependent integration tests skip gracefully in sandbox.
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
        in {
          checks.${system} = (perSystem.checks.${system} or {}) // {
            # NixOS VM integration test.
            vm-test = pkgs.testers.nixosTest {
              name = "underwrap-vm-test";

              nodes.machine = { pkgs, ... }: {
                imports = [ self.nixosModules.default ];

                services.underwrap = {
                  enable = true;
                  package = perSystem.packages.${system}.default;

                  wrappers = {
                    test-echo = {
                      target = "${pkgs.coreutils}/bin/env";
                      secret = "test/token";
                      injection = { mode = "env"; variable = "MY_TOKEN"; };
                      envVariable = "MY_TOKEN";
                    };
                    test-echo2 = {
                      target = "${pkgs.coreutils}/bin/echo";
                      secret = "test/other";
                      injection = { mode = "env"; variable = "OTHER_SECRET"; };
                      envVariable = "OTHER_SECRET";
                    };
                    test-locked = {
                      target = "${pkgs.coreutils}/bin/env";
                      secret = "test/locked-tok";
                      injection = { mode = "env"; variable = "LOCKED_TOKEN"; };
                      envVariable = "LOCKED_TOKEN";
                      requireUnlock = true;
                    };
                  };

                  secrets = {
                    "test/token" = { mutable = false; };
                    "test/other" = { mutable = true; };
                    "test/locked-tok" = { mutable = false; };
                  };
                };
              };

              testScript = let
                uwctl = "${perSystem.packages.${system}.default}/bin/underwrapctl --socket /run/underwrap/control.sock";
                wrapDir = "$(echo /nix/store/*-underwrap-wrappers)";
              in ''
                machine.wait_for_unit("underwrapd.service")
                machine.wait_for_file("/run/underwrap/read.sock")
                machine.wait_for_file("/run/underwrap/control.sock")

                # === Service & user ===
                machine.succeed("systemctl is-active underwrapd.service")
                machine.succeed("id underwrap")

                # === Socket existence and permissions ===
                machine.succeed("test -S /run/underwrap/read.sock")
                machine.succeed("test -S /run/underwrap/control.sock")

                # Directory should be 0750.
                perms = machine.succeed("stat -c %a /run/underwrap").strip()
                assert perms == "750", f"expected dir perms 750, got {perms}"

                # Read socket should be 0660.
                perms = machine.succeed("stat -c %a /run/underwrap/read.sock").strip()
                assert perms == "660", f"expected read socket perms 660, got {perms}"

                # Control socket should be 0600.
                perms = machine.succeed("stat -c %a /run/underwrap/control.sock").strip()
                assert perms == "600", f"expected control socket perms 600, got {perms}"

                # === Store & read secrets ===
                machine.succeed("echo -n 'ghp_testvmtoken' | ${uwctl} store test/token")
                machine.succeed("echo -n 'other_secret_val' | ${uwctl} store test/other")

                # === Status queries ===
                status = machine.succeed("${uwctl} status")
                assert "test-echo" in status, f"expected test-echo in status: {status}"
                assert "test-echo2" in status, f"expected test-echo2 in status: {status}"

                status = machine.succeed("${uwctl} status test-echo")
                assert "true" in status, f"expected secret_stored true: {status}"

                # === Wrapper env injection ===
                output = machine.succeed(
                    "UNDERWRAP_CONFIG_DIR=${wrapDir}/etc/underwrap/wrappers "
                    "${wrapDir}/bin/test-echo"
                )
                assert "MY_TOKEN=ghp_testvmtoken" in output, (
                    f"expected MY_TOKEN in env output: {output}"
                )

                # === Wrapper argument passthrough ===
                output = machine.succeed(
                    "UNDERWRAP_CONFIG_DIR=${wrapDir}/etc/underwrap/wrappers "
                    "${wrapDir}/bin/test-echo2 hello world"
                )
                assert "hello world" in output, f"expected 'hello world': {output}"

                # === Undeclared secret → rejected ===
                machine.fail("echo -n 'bad' | ${uwctl} store undeclared/secret")

                # === Immutable secret rejects overwrite ===
                machine.fail("echo -n 'overwrite' | ${uwctl} store test/token")

                # Original value preserved after failed overwrite.
                output = machine.succeed(
                    "UNDERWRAP_CONFIG_DIR=${wrapDir}/etc/underwrap/wrappers "
                    "${wrapDir}/bin/test-echo"
                )
                assert "MY_TOKEN=ghp_testvmtoken" in output, (
                    f"immutable secret was overwritten: {output}"
                )

                # === Mutable secret accepts overwrite ===
                machine.succeed("echo -n 'updated_val' | ${uwctl} store test/other")
                status = machine.succeed("${uwctl} status test/other")
                assert "true" in status, f"expected stored true after overwrite: {status}"

                # === Multi-line secret (OAuth JSON) ===
                machine.succeed(
                    'printf \'{"refresh_token":"rt_abc","client_id":"123"}\' | '
                    "${uwctl} store test/other"
                )
                status = machine.succeed("${uwctl} status test/other")
                assert "true" in status

                # === Unlock: locked wrapper denied before unlock ===
                machine.succeed("echo -n 'locked_secret' | ${uwctl} store test/locked-tok")
                machine.fail(
                    "UNDERWRAP_CONFIG_DIR=${wrapDir}/etc/underwrap/wrappers "
                    "${wrapDir}/bin/test-locked"
                )

                # === Unlock: unlock then read succeeds ===
                machine.succeed("${uwctl} unlock test-locked --seconds 60")
                output = machine.succeed(
                    "UNDERWRAP_CONFIG_DIR=${wrapDir}/etc/underwrap/wrappers "
                    "${wrapDir}/bin/test-locked"
                )
                assert "LOCKED_TOKEN=locked_secret" in output, (
                    f"expected LOCKED_TOKEN after unlock: {output}"
                )

                # === Unlock: lock again, denied ===
                machine.succeed("${uwctl} lock test-locked")
                machine.fail(
                    "UNDERWRAP_CONFIG_DIR=${wrapDir}/etc/underwrap/wrappers "
                    "${wrapDir}/bin/test-locked"
                )

                # === No secret material in daemon logs ===
                logs = machine.succeed("journalctl -u underwrapd.service --no-pager")
                assert "ghp_testvmtoken" not in logs, "secret leaked to logs!"
                assert "other_secret_val" not in logs, "secret leaked to logs!"
                assert "refresh_token" not in logs, "secret leaked to logs!"
                assert "locked_secret" not in logs, "secret leaked to logs!"

                machine.log("All underwrap VM tests passed!")
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
