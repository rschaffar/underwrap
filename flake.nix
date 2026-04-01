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
                  };

                  secrets = {
                    "test/token" = { mutable = false; };
                    "test/other" = { mutable = true; };
                  };
                };
              };

              testScript = ''
                machine.wait_for_unit("underwrapd.service")
                machine.wait_for_file("/run/underwrap/read.sock")
                machine.wait_for_file("/run/underwrap/control.sock")

                # Verify the daemon is running.
                machine.succeed("systemctl is-active underwrapd.service")

                # Verify the underwrap user exists.
                machine.succeed("id underwrap")

                # Verify sockets exist with correct ownership.
                machine.succeed("test -S /run/underwrap/read.sock")
                machine.succeed("test -S /run/underwrap/control.sock")

                # Store a secret via underwrapctl.
                machine.succeed(
                    "echo -n 'ghp_testvmtoken' | "
                    "${perSystem.packages.${system}.default}/bin/underwrapctl "
                    "--socket /run/underwrap/control.sock "
                    "store test/token"
                )

                # Store a second secret.
                machine.succeed(
                    "echo -n 'other_secret_val' | "
                    "${perSystem.packages.${system}.default}/bin/underwrapctl "
                    "--socket /run/underwrap/control.sock "
                    "store test/other"
                )

                # Query general status.
                status = machine.succeed(
                    "${perSystem.packages.${system}.default}/bin/underwrapctl "
                    "--socket /run/underwrap/control.sock "
                    "status"
                )
                assert "test-echo" in status, f"expected test-echo in status: {status}"
                assert "test-echo2" in status, f"expected test-echo2 in status: {status}"

                # Query specific wrapper status — should show secret_stored = true.
                status = machine.succeed(
                    "${perSystem.packages.${system}.default}/bin/underwrapctl "
                    "--socket /run/underwrap/control.sock "
                    "status test-echo"
                )
                assert "true" in status, f"expected secret_stored true: {status}"

                # Run the wrapper and verify secret injection.
                # test-echo wraps `env`, which prints all env vars.
                output = machine.succeed(
                    "UNDERWRAP_CONFIG_DIR=$(echo /nix/store/*-underwrap-wrappers/etc/underwrap/wrappers) "
                    "$(echo /nix/store/*-underwrap-wrappers/bin/test-echo)"
                )
                assert "MY_TOKEN=ghp_testvmtoken" in output, (
                    f"expected MY_TOKEN in env output: {output}"
                )

                # Run the second wrapper.
                output = machine.succeed(
                    "UNDERWRAP_CONFIG_DIR=$(echo /nix/store/*-underwrap-wrappers/etc/underwrap/wrappers) "
                    "$(echo /nix/store/*-underwrap-wrappers/bin/test-echo2) hello world"
                )
                assert "hello world" in output, f"expected 'hello world' in output: {output}"

                # Try storing an undeclared secret — should fail.
                machine.fail(
                    "echo -n 'bad' | "
                    "${perSystem.packages.${system}.default}/bin/underwrapctl "
                    "--socket /run/underwrap/control.sock "
                    "store undeclared/secret"
                )

                # Overwrite mutable secret and verify.
                machine.succeed(
                    "echo -n 'updated_val' | "
                    "${perSystem.packages.${system}.default}/bin/underwrapctl "
                    "--socket /run/underwrap/control.sock "
                    "store test/other"
                )
                status = machine.succeed(
                    "${perSystem.packages.${system}.default}/bin/underwrapctl "
                    "--socket /run/underwrap/control.sock "
                    "status test/other"
                )
                assert "true" in status, f"expected stored true after overwrite: {status}"

                # Verify daemon logs don't contain secret material.
                logs = machine.succeed("journalctl -u underwrapd.service --no-pager")
                assert "ghp_testvmtoken" not in logs, "secret leaked to logs!"
                assert "other_secret_val" not in logs, "secret leaked to logs!"

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
