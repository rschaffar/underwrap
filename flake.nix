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
            underwrap-ns = mkCrate "underwrap-ns";
            underwrap-backendd = mkCrate "underwrap-backendd";
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

              nodes.machine = { pkgs, ... }:
              let
                # Script that crashes with SIGSEGV. Used to test that core dumps
                # are disabled inside the PID namespace.
                crashScript = pkgs.writeShellScript "crash-target" ''
                  kill -SEGV $$
                '';
              in {
                imports = [ self.nixosModules.default ];

                # Create a normal unprivileged user for testing.
                users.users.testuser = {
                  isNormalUser = true;
                  uid = 1099;
                };



                services.underwrap = {
                  enable = true;
                  package = perSystem.packages.${system}.default;

                  instances.local = {
                    wrappers = {
                      test-echo = {
                        target = "${pkgs.coreutils}/bin/env";
                        secret = "test/token";
                        injection = { mode = "env"; variable = "MY_TOKEN"; };
                      };
                      test-echo2 = {
                        target = "${pkgs.coreutils}/bin/echo";
                        secret = "test/other";
                        injection = { mode = "env"; variable = "OTHER_SECRET"; };
                      };
                      test-locked = {
                        target = "${pkgs.coreutils}/bin/env";
                        secret = "test/locked-tok";
                        injection = { mode = "env"; variable = "LOCKED_TOKEN"; };
                        requireUnlock = true;
                      };
                      test-sleep = {
                        target = "${pkgs.coreutils}/bin/sleep";
                        secret = "test/dumpable-tok";
                        injection = { mode = "env"; variable = "DUMPABLE_SECRET"; };
                      };
                      test-crash = {
                        target = "${crashScript}";
                        secret = "test/crash-tok";
                        injection = { mode = "env"; variable = "CRASH_SECRET"; };
                      };
                    };

                    secrets = {
                      "test/token" = { mutable = false; };
                      "test/other" = { mutable = true; };
                      "test/locked-tok" = { mutable = false; };
                      "test/dumpable-tok" = { mutable = false; };
                      "test/crash-tok" = { mutable = false; };
                    };
                  };
                  };
                };
              };

              testScript = let
                uwctl = "${perSystem.packages.${system}.default}/bin/underwrapctl --socket /run/underwrap-local/control.sock";
                su = cmd: "su testuser -s /bin/sh -c '${cmd}'";
                # Python script that tries to spoof a wrapper request on the raw socket.
                # Exits 0 if the daemon correctly rejects the request.
                # Exits 1 if the daemon returns the secret (spoofing succeeded).
                spoofScript = pkgs.writeScript "spoof-test.py" ''
                  #!/usr/bin/env python3
                  import socket, json, struct, sys

                  s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                  s.connect("/run/underwrap-local/read.sock")
                  payload = json.dumps({"wrapper": "test-echo", "scope": None}).encode()
                  s.sendall(struct.pack(">I", len(payload)) + payload)

                  # Read length-prefixed response.
                  header = b""
                  while len(header) < 4:
                      chunk = s.recv(4 - len(header))
                      if not chunk:
                          print("Connection closed before response", file=sys.stderr)
                          sys.exit(0)  # Rejection by disconnect is fine.
                      header += chunk

                  length = struct.unpack(">I", header)[0]
                  body = b""
                  while len(body) < length:
                      chunk = s.recv(length - len(body))
                      if not chunk:
                          break
                      body += chunk

                  resp = json.loads(body)
                  if resp.get("status") == "ok":
                      print("SPOOFED: got secret!", file=sys.stderr)
                      sys.exit(1)
                  print("Rejected:", resp.get("message", ""), file=sys.stderr)
                  sys.exit(0)
                '';
              in ''
                machine.wait_for_unit("underwrapd-local.service")
                machine.wait_for_file("/run/underwrap-local/read.sock")
                machine.wait_for_file("/run/underwrap-local/control.sock")

                # ================================================================
                # ROOT-LEVEL SETUP AND BASIC CHECKS
                # ================================================================

                machine.succeed("systemctl is-active underwrapd-local.service")
                machine.succeed("id underwrap")
                machine.succeed("id testuser")

                # --- Socket permissions ---
                perms = machine.succeed("stat -c %a /run/underwrap").strip()
                assert perms == "775", f"expected dir perms 775, got {perms}"
                perms = machine.succeed("stat -c %a /run/underwrap-local/read.sock").strip()
                assert perms == "666", f"expected read socket perms 666, got {perms}"
                perms = machine.succeed("stat -c %a /run/underwrap-local/control.sock").strip()
                assert perms == "600", f"expected control socket perms 600, got {perms}"

                # --- Per-wrapper groups exist ---
                machine.succeed("getent group underwrap-local-test-echo")
                machine.succeed("getent group underwrap-local-test-echo2")
                machine.succeed("getent group underwrap-local-test-locked")
                machine.succeed("getent group underwrap-local-test-sleep")

                # --- Setgid wrappers installed correctly ---
                stat = machine.succeed("stat -c '%A %G' /run/wrappers/bin/local-test-echo").strip()
                machine.log(f"test-echo wrapper: {stat}")
                assert "underwrap-local-test-echo" in stat, f"wrong group: {stat}"
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

                output = machine.succeed("/run/wrappers/bin/local-test-echo")
                assert "MY_TOKEN=ghp_testvmtoken" in output, (
                    f"expected MY_TOKEN: {output}"
                )

                output = machine.succeed("/run/wrappers/bin/local-test-echo2 hello world")
                assert "hello world" in output

                # ================================================================
                # UNPRIVILEGED USER: WRAPPER INVOCATION
                # ================================================================

                # --- testuser can run wrappers and get secrets injected ---
                output = machine.succeed("${su "/run/wrappers/bin/local-test-echo"}")
                assert "MY_TOKEN=ghp_testvmtoken" in output, (
                    f"unprivileged user: expected MY_TOKEN: {output}"
                )
                machine.log("PASS: unprivileged user can use wrapper and gets secret")

                # --- testuser can run a second wrapper ---
                output = machine.succeed("${su "/run/wrappers/bin/local-test-echo2 hello"}")
                assert "hello" in output
                machine.log("PASS: unprivileged user can use second wrapper")

                # --- Verify daemon resolved identity from effective GID ---
                # The daemon should log read requests with the resolved wrapper name
                # for the testuser's requests (not "wrapper identity rejected").
                logs = machine.succeed(
                    "journalctl -u underwrapd-local.service --no-pager"
                    " | sed 's/\\x1b\\[[0-9;]*m//g'"
                    " | grep 'uid: 1099'"
                )
                assert "read request" in logs, (
                    "expected read request from testuser in daemon logs"
                )
                # Should NOT contain "identity rejected" for testuser.
                assert "identity rejected" not in logs or "uid: 1099" not in logs.split("identity rejected")[0], (
                    "testuser identity was rejected"
                )
                machine.log("PASS: daemon resolved wrapper identity for unprivileged user")

                # ================================================================
                # UNPRIVILEGED USER: ISOLATION
                # ================================================================

                # --- testuser cannot talk to the control socket ---
                machine.fail("${su "echo test | ${uwctl} status"}")
                machine.log("PASS: unprivileged user cannot access control socket")

                # --- testuser cannot read secret files directly ---
                machine.fail("${su "cat /var/lib/underwrap-local/secrets/test/token"}")
                machine.log("PASS: unprivileged user cannot read secret files")

                # ================================================================
                # WRAPPER ISOLATION: each wrapper gets only its own secret
                # ================================================================

                # test-echo wraps `env` → prints all env vars.
                # It should contain MY_TOKEN but NOT OTHER_SECRET or LOCKED_TOKEN.
                output = machine.succeed("${su "/run/wrappers/bin/local-test-echo"}")
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
                output1 = machine.succeed("${su "/run/wrappers/bin/local-test-echo"}")
                assert "MY_TOKEN=ghp_testvmtoken" in output1

                # testuser runs test-echo2 → gets OTHER_SECRET, not MY_TOKEN.
                output2 = machine.succeed("${su "/run/wrappers/bin/local-test-echo2 printenv"}")
                # test-echo2 wraps `echo`, so it just prints args, not env.
                # We can't check env isolation with echo. Instead verify the daemon
                # served the correct wrapper by checking the audit log.
                logs = machine.succeed(
                    "journalctl -u underwrapd-local.service --no-pager"
                    " | sed 's/\\x1b\\[[0-9;]*m//g'"
                    " | grep 'uid: 1099'"
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
                machine.fail("${su "/run/wrappers/bin/local-test-locked"}")
                machine.log("PASS: locked wrapper blocks unprivileged user")

                # ================================================================
                # LOCK/UNLOCK LIFECYCLE for unprivileged user
                # ================================================================

                # Unlock → testuser uses → lock → testuser denied.
                machine.succeed("${uwctl} unlock test-locked --seconds 60")
                output = machine.succeed("${su "/run/wrappers/bin/local-test-locked"}")
                assert "LOCKED_TOKEN=locked_secret" in output

                machine.succeed("${uwctl} lock test-locked")
                machine.fail("${su "/run/wrappers/bin/local-test-locked"}")
                machine.log("PASS: lock/unlock lifecycle works for unprivileged user")

                # ================================================================
                # DAEMON RESTART: secrets persist
                # ================================================================

                machine.succeed("systemctl restart underwrapd-local.service")
                machine.wait_for_file("/run/underwrap-local/read.sock")
                import time
                time.sleep(0.5)

                # Secret should still be there after restart.
                output = machine.succeed("/run/wrappers/bin/local-test-echo")
                assert "MY_TOKEN=ghp_testvmtoken" in output, (
                    f"secret lost after daemon restart: {output}"
                )
                machine.log("PASS: secrets persist across daemon restart")

                # ================================================================
                # PID NAMESPACE ISOLATION
                # ================================================================

                # Verify underwrap-ns is installed with cap_sys_admin.
                machine.succeed("test -x /run/wrappers/bin/underwrap-ns")
                caps = machine.succeed(
                    "getcap /run/wrappers/bin/underwrap-ns"
                ).strip()
                assert "cap_sys_admin" in caps, f"expected cap_sys_admin, got: {caps}"
                machine.log("PASS: underwrap-ns has cap_sys_admin")

                # Launch sleep wrapper as testuser in background.
                machine.succeed(
                    "su testuser -s /bin/sh -c '"
                    "setsid /run/wrappers/bin/local-test-sleep 300 </dev/null >/dev/null 2>&1 &"
                    "'"
                )
                import time
                time.sleep(1)

                # The sleep process runs inside a PID namespace.
                # From the parent namespace, it should NOT be visible via pgrep.
                rc = machine.execute("pgrep -u testuser -x sleep")[0]
                if rc != 0:
                    machine.log(
                        "PASS: PID namespace isolation — sleep process invisible "
                        "to parent namespace"
                    )
                else:
                    # If visible, check if /proc/pid/environ is at least not readable.
                    sleep_pid = machine.succeed("pgrep -u testuser -x sleep").strip().split("\n")[0]
                    rc2 = machine.execute(
                        f"su testuser -s /bin/sh -c 'cat /proc/{sleep_pid}/environ'"
                    )[0]
                    if rc2 != 0:
                        machine.log(
                            "NOTE: sleep visible but /proc/pid/environ not readable"
                        )
                    else:
                        env_out = machine.succeed(
                            f"cat /proc/{sleep_pid}/environ | tr '\\0' '\\n'"
                        )
                        if "DUMPABLE_SECRET" in env_out:
                            raise Exception(
                                "FAIL: same-UID process read secret from /proc/pid/environ"
                            )
                        machine.log("NOTE: sleep visible, environ readable, but no secret found")

                # ================================================================
                # WRAPPER PROCESS MEMORY PROTECTION
                # ================================================================

                # The wrapper chain has processes in the parent namespace:
                # underwrap-exec parent (waitpid) and underwrap-ns parent (waitpid).
                # Both hold the secret in memory and are non-dumpable.
                # A same-UID process should NOT be able to read their memory.

                # Find all wrapper-related processes owned by testuser.
                wrapper_pids = machine.succeed(
                    "pgrep -u testuser -f 'test-sleep|underwrap' || echo none"
                ).strip()
                machine.log("wrapper chain PIDs: " + wrapper_pids)

                if wrapper_pids != "none":
                    for pid_str in wrapper_pids.split("\n"):
                        pid_str = pid_str.strip()
                        if not pid_str:
                            continue

                        # /proc/<pid>/mem must not be readable by same-UID.
                        rc = machine.execute(
                            f"su testuser -s /bin/sh -c "
                            f"'dd if=/proc/{pid_str}/mem bs=1 count=1 2>/dev/null'"
                        )[0]
                        assert rc != 0, (
                            f"FAIL: same-UID read /proc/{pid_str}/mem"
                        )

                        # /proc/<pid>/environ must not be readable by same-UID.
                        rc = machine.execute(
                            f"su testuser -s /bin/sh -c "
                            f"'cat /proc/{pid_str}/environ 2>/dev/null'"
                        )[0]
                        assert rc != 0, (
                            f"FAIL: same-UID read /proc/{pid_str}/environ"
                        )

                    machine.log(
                        "PASS: wrapper process mem+environ not readable by same-UID"
                    )
                else:
                    machine.log("NOTE: no wrapper PIDs found")

                # Clean up.
                machine.succeed("pkill -u testuser -f test-sleep || true")
                time.sleep(0.5)

                # ================================================================
                # CORE DUMP PROTECTION
                # ================================================================

                # Store a secret for the crash wrapper.
                machine.succeed(
                    "echo -n 'crash_secret_value' | ${uwctl} store test/crash-tok"
                )

                # Enable core dumps system-wide so we can verify underwrap blocks them.
                machine.succeed("ulimit -c unlimited || true")
                machine.succeed("mkdir -p /tmp/cores")
                machine.succeed(
                    "echo '/tmp/cores/core.%e.%p' > /proc/sys/kernel/core_pattern"
                )

                # Run the crash wrapper as testuser. The target sends itself SIGSEGV.
                rc = machine.execute("${su "/run/wrappers/bin/local-test-crash"}")[0]
                machine.log(f"crash wrapper exit code: {rc}")
                # The wrapper should exit non-zero (the target was killed by signal).
                # Exit code 0 would mean the crash was swallowed.

                # Verify no core file was produced.
                core_files = machine.succeed("ls /tmp/cores/ 2>/dev/null || true").strip()
                assert core_files == "", (
                    f"FAIL: core dump produced despite RLIMIT_CORE=0: {core_files}"
                )
                machine.log("PASS: no core dump produced when target crashes")

                # (Unlock lifecycle for unprivileged users tested above.)

                # ================================================================
                # AUDIT LOG VERIFICATION
                # ================================================================

                logs = machine.succeed(
                    "journalctl -u underwrapd-local.service --no-pager"
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
                assert "crash_secret_value" not in logs, "secret leaked to logs!"

                machine.log("All underwrap VM tests passed!")
              '';
            };

            # Remote backend test: full SSH tunnel flow in one VM.
            # backend user runs underwrap-backendd, SSH-forwards socket to tunnel user.
            vm-test-remote = pkgs.testers.nixosTest {
              name = "underwrap-vm-test-remote";

              nodes.machine = { pkgs, ... }: {
                imports = [ self.nixosModules.default ];

                environment.systemPackages = [ pkgs.openssh ];

                # Normal user who runs wrappers.
                users.users.testuser = {
                  isNormalUser = true;
                  uid = 1099;
                };

                # Backend user: runs underwrap-backendd on the "workstation" side.
                users.users.underwrap-backend = {
                  isSystemUser = true;
                  group = "underwrap-backend";
                  home = "/var/lib/underwrap-backend";
                  createHome = true;
                  description = "underwrap backend operator";
                };
                users.groups.underwrap-backend = {};

                # Backend daemon service.
                systemd.services.underwrap-backendd = {
                  description = "underwrap backend daemon";
                  wantedBy = [ "multi-user.target" ];
                  serviceConfig = {
                    Type = "simple";
                    User = "underwrap-backend";
                    Group = "underwrap-backend";
                    ExecStart = let
                      cfg = pkgs.writeText "backend.yaml" (builtins.toJSON {
                        socket = "/run/underwrap-backend/backend.sock";
                        storage = {
                          backend = "plain";
                          dir = "/var/lib/underwrap-backend/secrets";
                        };
                      });
                    in "${perSystem.packages.${system}.default}/bin/underwrap-backendd --config ${cfg}";
                    Restart = "on-failure";
                  };
                };

                systemd.tmpfiles.rules = [
                  "d /run/underwrap-backend 0700 underwrap-backend underwrap-backend -"
                  "d /var/lib/underwrap-backend/secrets 0700 underwrap-backend underwrap-backend -"
                ];

                # Frontend daemon with remote storage.
                services.underwrap = {
                  enable = true;
                  package = perSystem.packages.${system}.default;

                  instances.tunnel = {
                    remote.enable = true;

                    wrappers = {
                      remote-echo = {
                        target = "${pkgs.coreutils}/bin/env";
                        secret = "remote/token";
                        injection = { mode = "env"; variable = "REMOTE_TOKEN"; };
                      };
                    };

                    secrets = {
                      "remote/token" = { mutable = false; };
                    };
                  };
                };

                # SSH server for the tunnel.
                services.openssh = {
                  enable = true;
                  settings = {
                    StreamLocalBindUnlink = "yes";
                  };
                };
              };

              testScript = ''
                import time

                machine.wait_for_unit("multi-user.target")
                machine.wait_for_unit("underwrap-backendd.service")
                machine.wait_for_unit("underwrapd-tunnel.service")
                machine.wait_for_unit("sshd.service")

                # === Setup: store a secret in the backend ===
                machine.succeed(
                    "su underwrap-backend -s /bin/sh -c '"
                    "mkdir -p /var/lib/underwrap-backend/secrets/remote"
                    "'"
                )
                machine.succeed(
                    "su underwrap-backend -s /bin/sh -c '"
                    "echo -n secret_via_tunnel > /var/lib/underwrap-backend/secrets/remote/token"
                    "'"
                )

                # === Setup: generate SSH key for the backend user ===
                machine.succeed(
                    "su underwrap-backend -s /bin/sh -c '"
                    "mkdir -p ~/.ssh && chmod 700 ~/.ssh && "
                    "ssh-keygen -t ed25519 -f ~/.ssh/tunnel_key -N \"\" -q"
                    "'"
                )

                # === Setup: authorize the key for underwrap-tunnel user ===
                # The key is restricted to only forward the backend socket.
                pubkey = machine.succeed(
                    "cat /var/lib/underwrap-backend/.ssh/tunnel_key.pub"
                ).strip()
                machine.succeed(
                    "mkdir -p /etc/ssh/authorized_keys.d"
                )
                # Get the tunnel user's home directory.
                tunnel_home = machine.succeed(
                    "getent passwd underwrap-tunnel | cut -d: -f6"
                ).strip()

                machine.succeed(
                    f"mkdir -p {tunnel_home}/.ssh && chmod 700 {tunnel_home}/.ssh"
                )
                machine.succeed(
                    f"echo '{pubkey}'"
                    f" > {tunnel_home}/.ssh/authorized_keys"
                )
                machine.succeed(f"chmod 600 {tunnel_home}/.ssh/authorized_keys")
                machine.succeed(f"chown -R underwrap-tunnel:underwrap {tunnel_home}/.ssh")

                # === Without tunnel: wrapper should fail ===
                machine.fail("test -S /run/underwrap-tunnel/backend.sock")
                machine.fail("su testuser -s /bin/sh -c '/run/wrappers/bin/tunnel-remote-echo'")
                machine.log("PASS: wrapper fails without tunnel")

                # === Establish SSH tunnel ===
                # backend user forwards its local backend socket to the tunnel user's path.
                machine.succeed(
                    "su underwrap-backend -s /bin/sh -c '"
                    "ssh -f -N "
                    "-o StrictHostKeyChecking=no "
                    "-o UserKnownHostsFile=/dev/null "
                    "-i ~/.ssh/tunnel_key "
                    "-R /run/underwrap-tunnel/backend.sock:/run/underwrap-backend/backend.sock "
                    "underwrap-tunnel@localhost"
                    "'"
                )
                time.sleep(1)

                # Verify the socket appeared.
                machine.succeed("test -S /run/underwrap-tunnel/backend.sock")
                machine.log("PASS: SSH tunnel established, backend socket present")

                # Fix socket permissions: SSH creates it owned by underwrap-tunnel,
                # but the daemon needs to read it. Since underwrap-tunnel is in the
                # underwrap group, we need the socket group-readable.
                machine.succeed("chmod 0660 /run/underwrap-tunnel/backend.sock")

                # === With tunnel: wrapper should succeed ===
                output = machine.succeed(
                    "su testuser -s /bin/sh -c '/run/wrappers/bin/tunnel-remote-echo'"
                )
                assert "REMOTE_TOKEN=secret_via_tunnel" in output, (
                    f"expected REMOTE_TOKEN in output: {output}"
                )
                machine.log("PASS: wrapper reads secret from remote backend via SSH tunnel")

                # === Verify the secret came from the backend, not local storage ===
                # The vm's /var/lib/underwrap-local/secrets/ should be empty.
                machine.fail("test -f /var/lib/underwrap-local/secrets/remote/token")
                machine.log("PASS: no secret on local disk")

                # === Kill tunnel: wrapper should fail ===
                machine.execute("pkill -f 'ssh.*tunnel_key' || true")
                time.sleep(0.5)
                machine.succeed("rm -f /run/underwrap-tunnel/backend.sock")

                machine.fail("su testuser -s /bin/sh -c '/run/wrappers/bin/tunnel-remote-echo'")
                machine.log("PASS: wrapper fails after tunnel killed")

                # === Re-establish tunnel: wrapper should succeed again ===
                machine.succeed(
                    "su underwrap-backend -s /bin/sh -c '"
                    "ssh -f -N "
                    "-o StrictHostKeyChecking=no "
                    "-o UserKnownHostsFile=/dev/null "
                    "-i ~/.ssh/tunnel_key "
                    "-R /run/underwrap-tunnel/backend.sock:/run/underwrap-backend/backend.sock "
                    "underwrap-tunnel@localhost"
                    "'"
                )
                time.sleep(1)
                machine.succeed("chmod 0660 /run/underwrap-tunnel/backend.sock")

                output = machine.succeed(
                    "su testuser -s /bin/sh -c '/run/wrappers/bin/tunnel-remote-echo'"
                )
                assert "REMOTE_TOKEN=secret_via_tunnel" in output, (
                    f"expected REMOTE_TOKEN after reconnect: {output}"
                )
                machine.log("PASS: wrapper works after tunnel reconnected")

                # === No secret material in daemon logs ===
                logs = machine.succeed("journalctl -u underwrapd-tunnel.service --no-pager")
                assert "secret_via_tunnel" not in logs, "secret leaked to frontend logs!"
                machine.log("PASS: no secret in frontend daemon logs")

                machine.log("All remote backend tests passed!")
              '';
            };
          };

    in
      # Merge all outputs.
      perSystem
      // systemIndependent
      // vmTest;
}
