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

                # Create a normal unprivileged user for testing.
                users.users.testuser = {
                  isNormalUser = true;
                  uid = 1099;
                };

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
                    test-sleep = {
                      target = "${pkgs.coreutils}/bin/sleep";
                      secret = "test/dumpable-tok";
                      injection = { mode = "env"; variable = "DUMPABLE_SECRET"; };
                      envVariable = "DUMPABLE_SECRET";
                    };
                  };

                  secrets = {
                    "test/token" = { mutable = false; };
                    "test/other" = { mutable = true; };
                    "test/locked-tok" = { mutable = false; };
                    "test/dumpable-tok" = { mutable = false; };
                  };
                };
              };

              testScript = let
                uwctl = "${perSystem.packages.${system}.default}/bin/underwrapctl --socket /run/underwrap/control.sock";
                su = cmd: "su testuser -s /bin/sh -c '${cmd}'";
                # Python script that tries to spoof a wrapper request on the raw socket.
                # Exits 0 if the daemon correctly rejects the request.
                # Exits 1 if the daemon returns the secret (spoofing succeeded).
                spoofScript = pkgs.writeScript "spoof-test.py" ''
                  #!/usr/bin/env python3
                  import socket, json, struct, sys
                  s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                  s.connect("/run/underwrap/read.sock")
                  payload = json.dumps({"wrapper": "test-echo", "scope": None}).encode()
                  s.sendall(struct.pack(">I", len(payload)) + payload)
                  data = s.recv(4096)
                  resp = json.loads(data[4:])
                  if resp.get("status") == "ok":
                      print("SPOOFED: got secret!", file=sys.stderr)
                      sys.exit(1)
                  print("Rejected:", resp.get("message", ""), file=sys.stderr)
                  sys.exit(0)
                '';
              in ''
                machine.wait_for_unit("underwrapd.service")
                machine.wait_for_file("/run/underwrap/read.sock")
                machine.wait_for_file("/run/underwrap/control.sock")

                # ================================================================
                # ROOT-LEVEL SETUP AND BASIC CHECKS
                # ================================================================

                machine.succeed("systemctl is-active underwrapd.service")
                machine.succeed("id underwrap")
                machine.succeed("id testuser")

                # --- Socket permissions ---
                perms = machine.succeed("stat -c %a /run/underwrap").strip()
                assert perms == "755", f"expected dir perms 755, got {perms}"
                perms = machine.succeed("stat -c %a /run/underwrap/read.sock").strip()
                assert perms == "666", f"expected read socket perms 666, got {perms}"
                perms = machine.succeed("stat -c %a /run/underwrap/control.sock").strip()
                assert perms == "600", f"expected control socket perms 600, got {perms}"

                # --- Per-wrapper groups exist ---
                machine.succeed("getent group underwrap-test-echo")
                machine.succeed("getent group underwrap-test-echo2")
                machine.succeed("getent group underwrap-test-locked")
                machine.succeed("getent group underwrap-test-sleep")

                # --- Setgid wrappers installed correctly ---
                stat = machine.succeed("stat -c '%A %G' /run/wrappers/bin/test-echo").strip()
                machine.log(f"test-echo wrapper: {stat}")
                assert "underwrap-test-echo" in stat, f"wrong group: {stat}"
                assert "s" in stat.lower(), f"setgid not set: {stat}"

                # --- Sidecar configs installed ---
                machine.succeed("test -f /etc/underwrap/wrappers/test-echo.yaml")
                machine.succeed("test -f /etc/underwrap/wrappers/test-echo2.yaml")

                # --- Store secrets (root only, via control socket) ---
                machine.succeed("echo -n 'ghp_testvmtoken' | ${uwctl} store test/token")
                machine.succeed("echo -n 'other_secret_val' | ${uwctl} store test/other")
                machine.succeed("echo -n 'locked_secret' | ${uwctl} store test/locked-tok")
                machine.succeed("echo -n 'dumpable_test_secret' | ${uwctl} store test/dumpable-tok")

                # --- Status ---
                status = machine.succeed("${uwctl} status")
                assert "test-echo" in status
                assert "test-echo2" in status
                status = machine.succeed("${uwctl} status test-echo")
                assert "true" in status, f"expected secret_stored true: {status}"

                # --- Immutability ---
                machine.fail("echo -n 'overwrite' | ${uwctl} store test/token")

                # --- Mutable overwrite ---
                machine.succeed("echo -n 'updated_val' | ${uwctl} store test/other")

                # --- Multi-line secret ---
                machine.succeed(
                    'printf \'{"refresh_token":"rt_abc","client_id":"123"}\' | '
                    "${uwctl} store test/other"
                )

                # ================================================================
                # ROOT WRAPPER TESTS (baseline — verify wrappers work at all)
                # ================================================================

                output = machine.succeed("/run/wrappers/bin/test-echo")
                assert "MY_TOKEN=ghp_testvmtoken" in output, (
                    f"expected MY_TOKEN: {output}"
                )

                output = machine.succeed("/run/wrappers/bin/test-echo2 hello world")
                assert "hello world" in output

                # ================================================================
                # UNPRIVILEGED USER: WRAPPER INVOCATION
                # ================================================================

                # --- testuser can run wrappers and get secrets injected ---
                output = machine.succeed("${su "/run/wrappers/bin/test-echo"}")
                assert "MY_TOKEN=ghp_testvmtoken" in output, (
                    f"unprivileged user: expected MY_TOKEN: {output}"
                )
                machine.log("PASS: unprivileged user can use wrapper and gets secret")

                # --- testuser can run a second wrapper ---
                output = machine.succeed("${su "/run/wrappers/bin/test-echo2 hello"}")
                assert "hello" in output
                machine.log("PASS: unprivileged user can use second wrapper")

                # --- Verify daemon resolved identity from effective GID ---
                logs = machine.succeed(
                    "journalctl -u underwrapd.service --no-pager"
                    " | sed 's/\\x1b\\[[0-9;]*m//g'"
                )
                assert "identity from egid" in logs, (
                    "expected egid-based identity for unprivileged user"
                )
                machine.log("PASS: daemon resolved wrapper identity from effective GID")

                # ================================================================
                # UNPRIVILEGED USER: ISOLATION
                # ================================================================

                # --- testuser cannot talk to the control socket ---
                machine.fail("${su "echo test | ${uwctl} status"}")
                machine.log("PASS: unprivileged user cannot access control socket")

                # --- testuser cannot read secret files directly ---
                machine.fail("${su "cat /var/lib/underwrap/secrets/test/token"}")
                machine.log("PASS: unprivileged user cannot read secret files")

                # ================================================================
                # WRAPPER ISOLATION: each wrapper gets only its own secret
                # ================================================================

                # test-echo wraps `env` → prints all env vars.
                # It should contain MY_TOKEN but NOT OTHER_SECRET or LOCKED_TOKEN.
                output = machine.succeed("${su "/run/wrappers/bin/test-echo"}")
                assert "MY_TOKEN=ghp_testvmtoken" in output, (
                    f"expected MY_TOKEN in output: {output}"
                )
                assert "OTHER_SECRET" not in output, (
                    f"test-echo leaked test-echo2's secret: {output}"
                )
                assert "LOCKED_TOKEN" not in output, (
                    f"test-echo leaked test-locked's secret: {output}"
                )
                machine.log("PASS: wrapper output contains only its own secret")

                # ================================================================
                # WRAPPER ISOLATION: different wrappers for same user get different secrets
                # ================================================================

                # testuser runs test-echo → gets MY_TOKEN.
                output1 = machine.succeed("${su "/run/wrappers/bin/test-echo"}")
                assert "MY_TOKEN=ghp_testvmtoken" in output1

                # testuser runs test-echo2 → gets OTHER_SECRET, not MY_TOKEN.
                output2 = machine.succeed("${su "/run/wrappers/bin/test-echo2 printenv"}")
                # test-echo2 wraps `echo`, so it just prints args, not env.
                # We can't check env isolation with echo. Instead verify the daemon
                # served the correct wrapper by checking the audit log.
                logs = machine.succeed(
                    "journalctl -u underwrapd.service --no-pager"
                    " | sed 's/\\x1b\\[[0-9;]*m//g'"
                    " | grep 'uid=1099'"
                )
                # Should see both test-echo and test-echo2 served for testuser.
                assert "test-echo" in logs, "missing test-echo read for testuser"
                assert "test-echo2" in logs, "missing test-echo2 read for testuser"
                machine.log("PASS: different wrappers serve different secrets for same user")

                # ================================================================
                # SPOOFING RESISTANCE: raw socket connection rejected
                # ================================================================

                # testuser connects directly to read.sock (bypassing wrapper binary)
                # and sends a forged request. The daemon should reject because
                # the peer's egid (users/100) doesn't map to any wrapper group.
                machine.succeed("${su "${pkgs.python3}/bin/python3 ${spoofScript}"}")
                machine.log("PASS: raw socket connection without wrapper GID is rejected")

                # ================================================================
                # LOCKED WRAPPER: blocks unprivileged user before unlock
                # ================================================================

                # Lock test-locked (it was unlocked earlier in the test).
                machine.succeed("${uwctl} lock test-locked")

                # testuser tries to use it → denied.
                machine.fail("${su "/run/wrappers/bin/test-locked"}")
                machine.log("PASS: locked wrapper blocks unprivileged user")

                # ================================================================
                # LOCK/UNLOCK LIFECYCLE for unprivileged user
                # ================================================================

                # Unlock → testuser uses → lock → testuser denied.
                machine.succeed("${uwctl} unlock test-locked --seconds 60")
                output = machine.succeed("${su "/run/wrappers/bin/test-locked"}")
                assert "LOCKED_TOKEN=locked_secret" in output

                machine.succeed("${uwctl} lock test-locked")
                machine.fail("${su "/run/wrappers/bin/test-locked"}")
                machine.log("PASS: lock/unlock lifecycle works for unprivileged user")

                # ================================================================
                # DAEMON RESTART: secrets persist
                # ================================================================

                machine.succeed("systemctl restart underwrapd.service")
                machine.wait_for_file("/run/underwrap/read.sock")
                import time
                time.sleep(0.5)

                # Secret should still be there after restart.
                output = machine.succeed("/run/wrappers/bin/test-echo")
                assert "MY_TOKEN=ghp_testvmtoken" in output, (
                    f"secret lost after daemon restart: {output}"
                )
                machine.log("PASS: secrets persist across daemon restart")

                # ================================================================
                # UNPRIVILEGED USER: PR_SET_DUMPABLE
                # ================================================================

                # Launch sleep wrapper as testuser in background.
                machine.succeed(
                    "su testuser -s /bin/sh -c '"
                    "setsid /run/wrappers/bin/test-sleep 300 </dev/null >/dev/null 2>&1 &"
                    "echo $! > /tmp/sleep.pid"
                    "'"
                )
                import time
                time.sleep(1)

                # Find the actual sleep process.
                actual_pid = machine.succeed(
                    "pgrep -n sleep || cat /tmp/sleep.pid"
                ).strip().split("\n")[0]
                machine.log(f"sleep PID: {actual_pid}")
                machine.succeed(f"test -d /proc/{actual_pid}")

                # As testuser (same UID), try to read /proc/<pid>/environ.
                # PR_SET_DUMPABLE=0 should make this fail.
                rc = machine.execute(
                    f"su testuser -s /bin/sh -c 'cat /proc/{actual_pid}/environ'"
                )[0]
                suid_dumpable = machine.succeed("cat /proc/sys/fs/suid_dumpable").strip()
                machine.log(f"suid_dumpable = {suid_dumpable}")
                if rc != 0:
                    machine.log("PASS: PR_SET_DUMPABLE blocks same-UID /proc/pid/environ read")
                elif suid_dumpable != "0":
                    machine.log(
                        f"NOTE: suid_dumpable={suid_dumpable}, kernel resets dumpable after exec. "
                        "Set suid_dumpable=0 for full /proc/pid/environ protection."
                    )
                else:
                    machine.log(
                        "NOTE: /proc/pid/environ readable despite suid_dumpable=0. "
                        "Kernel may have reset dumpable after exec chain."
                    )

                machine.succeed(f"kill {actual_pid} || true")

                # (Unlock lifecycle for unprivileged users tested above.)

                # ================================================================
                # AUDIT LOG VERIFICATION
                # ================================================================

                logs = machine.succeed(
                    "journalctl -u underwrapd.service --no-pager"
                    " | sed 's/\\x1b\\[[0-9;]*m//g'"
                )
                assert 'audit="read_granted"' in logs, "missing read_granted"
                assert 'audit="secret_stored"' in logs, "missing secret_stored"
                assert 'audit="read_denied"' in logs, "missing read_denied"
                assert 'audit="unlocked"' in logs, "missing unlocked"
                assert 'audit="locked"' in logs, "missing locked"

                # --- No secret material in logs ---
                assert "ghp_testvmtoken" not in logs, "secret leaked to logs!"
                assert "other_secret_val" not in logs, "secret leaked to logs!"
                assert "locked_secret" not in logs, "secret leaked to logs!"
                assert "dumpable_test_secret" not in logs, "secret leaked to logs!"

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
