# VM test node: both local and remote instances on one machine.
{ pkgs, underwrapPackage, nixosModule, ... }:

let
  testCaPem = pkgs.writeText "underwrap-test-ca.pem" (builtins.readFile ../../testdata/pki/ca.pem);
  testServerPem = pkgs.writeText "underwrap-test-server.pem" (builtins.readFile ../../testdata/pki/server.pem);
  testServerKey = pkgs.writeText "underwrap-test-server.key" (builtins.readFile ../../testdata/pki/server.key);
  testLocalClientPem = pkgs.writeText "underwrap-test-client-local.pem" (builtins.readFile ../../testdata/pki/client-local.pem);
  testLocalClientKey = pkgs.writeText "underwrap-test-client-local.key" (builtins.readFile ../../testdata/pki/client-local.key);
  testTunnelClientPem = pkgs.writeText "underwrap-test-client-tunnel.pem" (builtins.readFile ../../testdata/pki/client-tunnel.pem);
  testTunnelClientKey = pkgs.writeText "underwrap-test-client-tunnel.key" (builtins.readFile ../../testdata/pki/client-tunnel.key);

  # SHA-256 of the DER-encoded client certificate — this is the
  # `cert_fingerprint` that the backend's mTLS layer matches against
  # `ClientInstanceRecord.current_credential.cert_fingerprint` to
  # identify a runtime client (see
  # `crates/underwrap-backend/src/runtime_clients.rs::cert_fingerprint_from_pem`).
  certFingerprint = name: pem:
    builtins.replaceStrings [ " " "\n" "-" ] [ "" "" "" ] (
      builtins.readFile (
        pkgs.runCommand "${name}-fingerprint" { nativeBuildInputs = [ pkgs.openssl ]; } ''
          openssl x509 -in ${pem} -outform DER | sha256sum | cut -d' ' -f1 | tr -d '\n' > $out
        ''
      )
    );
  localClientFingerprint = certFingerprint "client-local" testLocalClientPem;
  tunnelClientFingerprint = certFingerprint "client-tunnel" testTunnelClientPem;

  # The backend's store registry is a directory of `<id>.json` files;
  # the id lives in the filename, not the body. Pre-stage one file
  # per daemon's lone test store.
  remoteStoreJson = pkgs.writeText "remote.json" (builtins.toJSON {
    name = "remote";
    backend = "plain";
    dir = "/var/lib/underwrap-backend/secrets";
    description = "remote-mode test store";
  });
  localStoreJson = pkgs.writeText "local.json" (builtins.toJSON {
    name = "local";
    backend = "plain";
    dir = "/var/lib/underwrap-backend-local/secrets";
    description = "local-mode test store";
  });

  # The backend's runtime client-instance registry is a directory of
  # `<id>.json` files. The cert fingerprint embedded here is what
  # `validate_runtime_client_mappings` will match incoming mTLS
  # connections against. The id lives in the filename, not the body.
  remoteTunnelClientJson = pkgs.writeText "inst_underwrap-tunnel.json" (builtins.toJSON {
    display_name = "underwrap-tunnel";
    current_store_id = "remote";
    status = "active";
    last_seen_at = null;
    current_credential = {
      cert_fingerprint = tunnelClientFingerprint;
      cert_pem = builtins.readFile ../../testdata/pki/client-tunnel.pem;
    };
    auto_create_wrapper_assets = false;
  });
  localClientJson = pkgs.writeText "inst_underwrap-local.json" (builtins.toJSON {
    display_name = "underwrap-local";
    current_store_id = "local";
    status = "active";
    last_seen_at = null;
    current_credential = {
      cert_fingerprint = localClientFingerprint;
      cert_pem = builtins.readFile ../../testdata/pki/client-local.pem;
    };
    auto_create_wrapper_assets = false;
  });
  crashScript = pkgs.writeShellScript "crash-target" ''
    kill -SEGV $$
  '';
  # Print capabilities from /proc/self/status.
  capsScript = pkgs.writeShellScript "caps-target" ''
    grep '^Cap' /proc/self/status
  '';
  # List all PIDs visible in /proc (for PID namespace isolation test).
  listPidsScript = pkgs.writeShellScript "list-pids-target" ''
    ls -d /proc/[0-9]* 2>/dev/null | while read d; do
      pid=$(basename "$d")
      echo "$pid"
    done
  '';
  dirReadScript = pkgs.writeShellScript "dir-read-target" ''
    set -eu
    ${pkgs.coreutils}/bin/cat "$HOME/.config/underwrap-dir-a/token"
    ${pkgs.coreutils}/bin/printf '\n'
  '';
  dirWriteScript = pkgs.writeShellScript "dir-write-target" ''
    set -eu
    mode="''${1:-seed}"
    ${pkgs.coreutils}/bin/mkdir -p "$HOME/.config/underwrap-dir-a/sub"
    case "$mode" in
      seed)
        printf 'token-seed' > "$HOME/.config/underwrap-dir-a/token"
        printf 'nested-seed' > "$HOME/.config/underwrap-dir-a/sub/nested"
        ;;
      update)
        printf 'token-updated' > "$HOME/.config/underwrap-dir-a/token"
        printf 'nested-updated' > "$HOME/.config/underwrap-dir-a/sub/nested"
        ;;
      *)
        echo "unknown mode: $mode" >&2
        exit 1
        ;;
    esac
  '';
  dirSleepScript = pkgs.writeShellScript "dir-sleep-target" ''
    set -eu
    ${pkgs.coreutils}/bin/cat "$HOME/.config/underwrap-dir-a/token" >/dev/null
    exec ${pkgs.coreutils}/bin/sleep "''${1:-300}"
  '';
  # Writes a file, signals readiness via a marker file, then sleeps
  # for a fixed short window. The test driver flips the grant during
  # the sleep; once the script exits, ns dispatches Checkin against
  # the flipped grant. The window is small (4s) so the test runs fast
  # and predictable even on a loaded host.
  dirWriteThenSleepScript = pkgs.writeShellScript "dir-write-then-sleep-target" ''
    set -eu
    ${pkgs.coreutils}/bin/mkdir -p "$HOME/.config/underwrap-dir-a"
    printf 'flip-attempt' > "$HOME/.config/underwrap-dir-a/token"
    # Mark write completion so the test driver can flip the grant
    # before ns dispatches end-of-run writeback.
    ${pkgs.coreutils}/bin/touch /tmp/dir-write-then-sleep.ready
    exec ${pkgs.coreutils}/bin/sleep "''${1:-4}"
  '';
  # Always-writable namespace mount probe: touches a file under the
  # bound directory regardless of grant. The wrapper exits 0 if the
  # mount accepted the write — the backend may still drop it at
  # Checkin time, which is asserted separately.
  dirTouchScript = pkgs.writeShellScript "dir-touch-target" ''
    set -eu
    ${pkgs.coreutils}/bin/mkdir -p "$HOME/.config/underwrap-dir-a"
    ${pkgs.coreutils}/bin/touch "$HOME/.config/underwrap-dir-a/touch-probe"
  '';
  # Prints its environment so a test can assert that no internal
  # underwrap-* vars leak into the target.
  dirEnvProbeScript = pkgs.writeShellScript "dir-env-probe-target" ''
    set -eu
    ${pkgs.coreutils}/bin/env
  '';
  dirSymlinkScript = pkgs.writeShellScript "dir-symlink-target" ''
    set -eu
    ${pkgs.coreutils}/bin/mkdir -p "$HOME/.config/underwrap-dir-a"
    exec ${pkgs.coreutils}/bin/ln -s /etc/passwd "$HOME/.config/underwrap-dir-a/evil"
  '';
  f006MarkerScript = pkgs.writeShellScript "f006-marker-target" ''
    ${pkgs.coreutils}/bin/touch /tmp/f006-unroutable-target-ran
  '';
  f006Exit37Script = pkgs.writeShellScript "f006-exit-37-target" ''
    set -eu
    printf 'persisted-after-exit-37' > "$HOME/.config/f006-exit-37/token"
    exit 37
  '';
  f006SigkillScript = pkgs.writeShellScript "f006-sigkill-target" ''
    set -eu
    printf 'persisted-before-sigkill' > "$HOME/.config/f006-sigkill/token"
    ${pkgs.coreutils}/bin/touch /tmp/f006-sigkill-ready
    exec ${pkgs.coreutils}/bin/sleep 6060
  '';
  # Combined files+libsecret lifecycle target. Secret Service mutation bytes
  # travel on stdin, never argv. Long-running modes keep a direct target and a
  # descendant live so the VM can terminate each trusted supervisor stage and
  # bind teardown assertions to exact process/namespace identities.
  f007LifecycleScript = pkgs.writeShellScript "f007-lifecycle-target" ''
    set -eu
    mode="''${1:-success}"
    if [ "$mode" = unchanged ]; then
      ${pkgs.coreutils}/bin/cat "$HOME/.config/f007/token" >/dev/null
      exit 0
    fi
    ${pkgs.coreutils}/bin/mkdir -p "$HOME/.config/f007"
    printf 'f007-file-%s' "$mode" > "$HOME/.config/f007/token"
    printf 'f007-libsecret-%s' "$mode" \
      | ${pkgs.libsecret}/bin/secret-tool store --label='F007 lifecycle' purpose f007 \
      >/dev/null
    case "$mode" in
      seed|success) exit 0 ;;
      exit37) exit 37 ;;
      *)
        seconds="''${2:-6200}"
        ${pkgs.coreutils}/bin/sleep "$seconds" &
        ${pkgs.coreutils}/bin/touch "/tmp/f007-ready-$mode"
        wait
        ;;
    esac
  '';
  # env-mode target whose DIRECT process is non-dumpable (incidental, via the
  # wrapper cap/namespace chain) but which spawns a CLEAN child that resets to
  # dumpable and inherits the secret env; mandatory target UID isolation must
  # still deny same-caller /proc reads. The child uses a unique sleep duration so the test can locate it
  # unambiguously, and `wait` keeps the direct target (this shell) alive so the
  # background child is never exec-optimized into the shell's own process.
  descendantScript = pkgs.writeShellScript "descendant-target" ''
    ${pkgs.coreutils}/bin/sleep 4242 &
    wait
  '';
  # Env-mode target used to prove mandatory target UID isolation. The shell
  # itself is the direct target and runs as the dedicated sandbox uid X; it
  # spawns a clean child sleep (also X), so a single invocation lets
  # local_target_uid_isolation assert that BOTH the direct target and its
  # descendant are denied to a same-caller reader, and that the target's real uid
  # is the dedicated sandbox uid (never 0, never the caller). No $HOME access.
  isoScript = pkgs.writeShellScript "iso-target" ''
    ${pkgs.coreutils}/bin/sleep 5252 &
    wait
  '';
  # Target that exercises an idmapped directory: it runs as
  # the dedicated uid X but, through the idmapped `~/iso-data` mount, reads a
  # caller-owned file (proving X sees caller files as owner) and writes one back
  # (proving the write lands on disk owned by the caller, not X).
  isoHomeScript = pkgs.writeShellScript "iso-home-target" ''
    set -eu
    ${pkgs.coreutils}/bin/cat "$HOME/iso-data/from-caller" > "$HOME/iso-data/from-target"
    # If a nested (sub)mount with a caller-owned 0600 file is present, copy it
    # back out. The target runs as uid X, so it can read that 0600 caller file
    # only if the submount was idmapped caller<->X — i.e. this exercises the
    # recursive submount idmap (an idmappable submount under a 0700 idmapped root
    # must still map). Skipped when no such submount is set up.
    if [ -e "$HOME/iso-data/sub/from-caller-sub" ]; then
      ${pkgs.coreutils}/bin/cat "$HOME/iso-data/sub/from-caller-sub" \
        > "$HOME/iso-data/sub/from-target-sub"
    fi
  '';
  # Target that exercises its own provisioned runtime directory.
  # It mkdirs + writes a marker under XDG_RUNTIME_DIR (the dedicated uid's own
  # 0700 dir, so this just works with no idmap), then prints the runtime dir path
  # so the test can confirm it is the provisioned per-target-uid dir and that the
  # marker is owned by the target uid X, not the caller.
  runtimeDirScript = pkgs.writeShellScript "runtime-dir-target" ''
    set -eu
    probe="$XDG_RUNTIME_DIR/uw-runtime-probe"
    ${pkgs.coreutils}/bin/mkdir -p "$probe"
    ${pkgs.coreutils}/bin/printf '%s' runtime_dir_ok > "$probe/marker"
    ${pkgs.coreutils}/bin/printf '%s\n' "$XDG_RUNTIME_DIR"
  '';
  # Target projecting a files slot. The files-slot staging
  # is owned by the dedicated uid X and bound at `$HOME/iso-files/dir-a` under an
  # idmapped `~/iso-files`, so the uid-X target reads/writes it directly. `write`
  # seeds a file (persisted by the supervisor through the single setfsuid(X)
  # window); `read` prints it back (materialized from the backend blob on the
  # next run). `printf` is a shell builtin so no PATH is needed.
  isoDirScript = pkgs.writeShellScript "iso-dir-target" ''
    set -eu
    mode="''${1:-read}"
    case "$mode" in
      write)
        printf 'iso_files_secret_val' > "$HOME/iso-files/dir-a/token"
        ;;
      rewrite)
        # Write a DISTINCT value so the post-run blob differs from a
        # 'write'-seeded baseline — used to drive the read-only-grant drop
        # under mandatory target UID isolation (the supervisor reads the changed
        # X-owned staging through the setfsuid(X) window, then refuses to persist it).
        printf 'iso_files_rewritten' > "$HOME/iso-files/dir-a/token"
        ;;
      read)
        ${pkgs.coreutils}/bin/cat "$HOME/iso-files/dir-a/token"
        ${pkgs.coreutils}/bin/printf '\n'
        ;;
      sleep)
        printf 'iso_files_secret_val' > "$HOME/iso-files/dir-a/token"
        exec ${pkgs.coreutils}/bin/sleep 5353
        ;;
      *)
        echo "unknown mode: $mode" >&2
        exit 1
        ;;
    esac
  '';
  # Isolation is keyed on the target *path* (every wrapper running the same
  # binary shares its uid). Give this libsecret fixture a DISTINCT path — a thin
  # exec shim — so it gets a separate dedicated uid from direct secret-tool
  # wrappers. `exec` with an absolute path needs no PATH in the cleared env.
  isoSecretTool = pkgs.writeShellScript "iso-secret-tool" ''
    exec ${pkgs.libsecret}/bin/secret-tool "$@"
  '';
in {
  imports = [ nixosModule ];

  environment.systemPackages = [ underwrapPackage pkgs.openssh pkgs.libsecret pkgs.python3 pkgs.curl pkgs.jq ];

  # Pre-seeded 32-byte session AEAD keys (one per daemon). The local
  # CA cert at a stable path so the test driver's curl invocations can
  # validate the daemon's HTTPS listeners. Backends already trust this
  # CA for mTLS client cert validation; this exposes the cert itself
  # for clients that want server-cert verification.
  environment.etc."underwrap-test-ca.pem" = {
    text = builtins.readFile ../../testdata/pki/ca.pem;
    mode = "0644";
  };

  # Normal user who runs wrappers. Home is 0755 so an isolated (dedicated-uid)
  # target can *traverse* the home to reach an idmapped subdirectory (the idmap
  # makes the leaf accessible, but path resolution still needs search permission
  # on each ancestor); see test-iso-home / local_target_uid_idmapped_home.
  users.users.testuser = {
    isNormalUser = true;
    uid = 1099;
    homeMode = "0755";
  };

  # Backend operator user (simulates workstation side for remote test).
  users.users.underwrap-backend = {
    isSystemUser = true;
    group = "underwrap-backend";
    home = "/var/lib/underwrap-backend";
    createHome = true;
    description = "underwrap backend operator";
  };
  users.groups.underwrap-backend = {};

  # Backend daemon service (for remote test).
  systemd.services.underwrap-backend = {
    description = "underwrap backend daemon";
    serviceConfig = {
      Type = "simple";
      User = "underwrap-backend";
      Group = "underwrap-backend";
      ExecStart = let
        cfg = pkgs.writeText "backend.yaml" (builtins.toJSON {
          stores_dir = "/var/lib/underwrap-backend/stores";
          clients_dir = "/var/lib/underwrap-backend/clients";
          users_dir = "/var/lib/underwrap-backend/users";
          bootstrap_file = "/var/lib/underwrap-backend/bootstrap.json";
          https = {
            listen = "127.0.0.1:18444";
            tls = {
              cert_file = testServerPem;
              key_file = testServerKey;
            };
          };
          admin = {
            allowed_origins = [ "https://127.0.0.1:18444" ];
            stores_data_root = "/var/lib/underwrap-backend/data-root";
          };
        });
      in "${underwrapPackage}/bin/underwrap-backend --config ${cfg}";
      Restart = "on-failure";
      UMask = "0077";
      ProtectSystem = "strict";
      ProtectHome = true;
      PrivateTmp = true;
      PrivateDevices = true;
      ProtectControlGroups = true;
      ProtectKernelModules = true;
      ProtectKernelTunables = true;
      ProtectClock = true;
      ProtectHostname = true;
      LockPersonality = true;
      MemoryDenyWriteExecute = true;
      RestrictSUIDSGID = true;
      RestrictRealtime = true;
      SystemCallArchitectures = "native";
      RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_INET6" ];
      NoNewPrivileges = true;
      LimitCORE = 0;
      ReadWritePaths = [ "/run/underwrap-backend" "/var/lib/underwrap-backend" ];
    };
  };


  systemd.tmpfiles.rules = [
    "d /run/underwrap-backend 0700 underwrap-backend underwrap-backend -"
    "d /var/lib/underwrap-backend/secrets 0700 underwrap-backend underwrap-backend -"
    "d /var/lib/underwrap-backend/data-root 0700 underwrap-backend underwrap-backend -"
    "d /var/lib/underwrap-backend-local 0700 underwrap-local underwrap-local -"
    "d /var/lib/underwrap-backend-local/secrets 0700 underwrap-local underwrap-local -"
    # Root the admin API carves dynamic stores into.
    "d /var/lib/underwrap-backend-local/data-root 0700 underwrap-local underwrap-local -"
    # Pre-stage per-instance client credentials. In production these are
    # generated by `underwrap-keygen --instance-config …` and the operator
    # pastes the resulting PEM into the admin UI; the VM test seeds them
    # directly from a baked-in test PKI so the daemon starts already enrolled.
    "C /var/lib/underwrap-local/client-local.pem 0400 underwrap-local underwrap-local - ${testLocalClientPem}"
    "C /var/lib/underwrap-local/client-local.key 0400 underwrap-local underwrap-local - ${testLocalClientKey}"
    "C /var/lib/underwrap-tunnel/client-tunnel.pem 0400 underwrap-tunnel underwrap-tunnel - ${testTunnelClientPem}"
    "C /var/lib/underwrap-tunnel/client-tunnel.key 0400 underwrap-tunnel underwrap-tunnel - ${testTunnelClientKey}"
    # Pre-stage the backend-managed store + client-instance registries.
    # Both are directories of `<id>.json` files now; the id is the
    # filename. The backend opens these at boot as the authoritative
    # source for which stores exist and which mTLS client certs map to
    # which store.
    "d /var/lib/underwrap-backend/stores 0700 underwrap-backend underwrap-backend -"
    "C /var/lib/underwrap-backend/stores/remote.json 0600 underwrap-backend underwrap-backend - ${remoteStoreJson}"
    "d /var/lib/underwrap-backend/clients 0700 underwrap-backend underwrap-backend -"
    "C /var/lib/underwrap-backend/clients/inst_underwrap-tunnel.json 0600 underwrap-backend underwrap-backend - ${remoteTunnelClientJson}"
    "d /var/lib/underwrap-backend-local/stores 0700 underwrap-local underwrap-local -"
    "C /var/lib/underwrap-backend-local/stores/local.json 0600 underwrap-local underwrap-local - ${localStoreJson}"
    "d /var/lib/underwrap-backend-local/clients 0700 underwrap-local underwrap-local -"
    "C /var/lib/underwrap-backend-local/clients/inst_underwrap-local.json 0600 underwrap-local underwrap-local - ${localClientJson}"
  ];

  # SSH server for the tunnel.
  services.openssh = {
    enable = true;
    settings.StreamLocalBindUnlink = "yes";
  };

  # Local backend daemon serving the local frontend instance.
  systemd.services.underwrap-backend-local = {
    description = "underwrap backend daemon (local)";
    serviceConfig = {
      Type = "simple";
      User = "underwrap-local";
      Group = "underwrap-local";
      ExecStart = let
        cfg = pkgs.writeText "underwrap-local-backend.yaml" (builtins.toJSON {
          stores_dir = "/var/lib/underwrap-backend-local/stores";
          clients_dir = "/var/lib/underwrap-backend-local/clients";
          users_dir = "/var/lib/underwrap-backend-local/users";
          bootstrap_file = "/var/lib/underwrap-backend-local/bootstrap.json";
          https = {
            listen = "127.0.0.1:18443";
            tls = {
              cert_file = testServerPem;
              key_file = testServerKey;
            };
          };
          admin = {
            allowed_origins = [ "https://127.0.0.1:18443" ];
            stores_data_root = "/var/lib/underwrap-backend-local/data-root";
          };
        });
      in "${underwrapPackage}/bin/underwrap-backend --config ${cfg}";
      Restart = "on-failure";
      UMask = "0077";
      ProtectSystem = "strict";
      ProtectHome = true;
      PrivateTmp = true;
      PrivateDevices = true;
      ProtectControlGroups = true;
      ProtectKernelModules = true;
      ProtectKernelTunables = true;
      ProtectClock = true;
      ProtectHostname = true;
      LockPersonality = true;
      MemoryDenyWriteExecute = true;
      RestrictSUIDSGID = true;
      RestrictRealtime = true;
      SystemCallArchitectures = "native";
      RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_INET6" ];
      NoNewPrivileges = true;
      LimitCORE = 0;
      ReadWritePaths = [ "/var/lib/underwrap-backend-local" ];
    };
  };


  # Two underwrap frontend instances: one locally satisfied, one remote-forwarded.
  services.underwrap = {
    enable = true;
    package = underwrapPackage;
    # Each isolated target gets its own provisioned runtime dir under this base
    # (<base>/<target-user-name>). Custom base keeps the test off /run/user.
    targetRuntimeDirBase = "/run/underwrap-test-runtime";

    instances = {
      # Local instance: frontend talks to a local backend socket.
      local = {
        allowedUsers = [ "testuser" ];
        backend = {
          url = "https://127.0.0.1:18443";
          caFile = testCaPem;
        };
        wrappers = {
          test-echo = {
            target = "${pkgs.coreutils}/bin/env";
            resource = "test/token";
            env = true;
          };
          test-echo2 = {
            target = "${pkgs.coreutils}/bin/echo";
            resource = "test/other";
            env = true;
          };
          test-locked = {
            target = "${pkgs.coreutils}/bin/env";
            resource = "test/locked-tok";
            env = true;
          };
          test-sleep = {
            target = "${pkgs.coreutils}/bin/sleep";
            resource = "test/dumpable-tok";
            env = true;
          };
          # Spawns a clean child (sleep 4242) that inherits the secret env; used
          # by local_pid_namespace_proc_isolation to prove the mandatory UID
          # boundary covers descendants after a clean exec.
          test-descendant = {
            target = "${descendantScript}";
            resource = "test/descendant-tok";
            env = true;
          };
          # Target-UID isolation: target and descendant run as a
          # dedicated uid X, so a same-caller process is denied the whole
          # subtree's /proc secret surface.
          test-iso = {
            target = "${isoScript}";
            resource = "test/iso-tok";
            env = true;
          };
          # Target with an idmapped caller-owned directory.
          test-iso-home = {
            target = "${isoHomeScript}";
            resource = "test/iso-home-tok";
            env = true;
            accessPaths = [ "~/iso-data" ];
          };
          # A target gets its own provisioned runtime directory as
          # XDG_RUNTIME_DIR (<targetRuntimeDirBase>/<target-user-name>), owned by
          # the dedicated UID rather than the caller. No wrapper flag is needed.
          test-runtime-dir = {
            target = "${runtimeDirScript}";
            resource = "test/runtime-tok";
            env = true;
          };
          # Target projecting a files slot. The staging is
          # owned by uid X and bound under an idmapped `~/iso-files`; the uid-X
          # target reads/writes it and the supervisor persists the writeback
          # through the single setfsuid(X) window.
          test-iso-dir = {
            target = "${isoDirScript}";
            resource = "test/iso-dir-tok";
            files.paths = [ "iso-files/dir-a" ];
            accessPaths = [ "~/iso-files" ];
          };
          # Libsecret target: dbus-daemon and secretsd run as
          # the dedicated uid X on an X-owned private bus tmpfs; the supervisor
          # reads the X-owned state file at writeback inside the setfsuid(X)
          # window.
          test-iso-libsecret = {
            target = "${isoSecretTool}";
            resource = "test/iso-libsecret-store";
            libsecret = true;
          };
          test-crash = {
            target = "${crashScript}";
            resource = "test/crash-tok";
            env = true;
          };
          test-libsecret = {
            target = "${pkgs.libsecret}/bin/secret-tool";
            resource = "test/libsecret-store";
            libsecret = true;
          };
          test-ns-pids = {
            target = "${listPidsScript}";
            resource = "test/ns-pids-store";
            libsecret = true;
            envSet.PATH = pkgs.lib.makeBinPath [ pkgs.coreutils ];
          };
          test-ns-pids-env = {
            target = "${listPidsScript}";
            resource = "test/ns-pids-env-store";
            env = true;
            envSet.PATH = pkgs.lib.makeBinPath [ pkgs.coreutils ];
          };
          test-caps = {
            target = "${capsScript}";
            resource = "test/caps-tok";
            env = true;
            envSet.PATH = pkgs.lib.makeBinPath [ pkgs.gnugrep ];
          };
          test-dir-write = {
            target = "${dirWriteScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
            accessPaths = [ "~/.config" ];
          };
          test-dir-read = {
            target = "${dirReadScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
            accessPaths = [ "~/.config" ];
          };
          test-dir-readonly-write = {
            target = "${dirWriteScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
            accessPaths = [ "~/.config" ];
          };
          test-dir-sleep = {
            target = "${dirSleepScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
            accessPaths = [ "~/.config" ];
          };
          test-dir-symlink = {
            target = "${dirSymlinkScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
            accessPaths = [ "~/.config" ];
          };
          # Used by the atomic-writeback grant-flip scenario.
          test-dir-write-then-sleep = {
            target = "${dirWriteThenSleepScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
            accessPaths = [ "~/.config" ];
          };
          # Used by the always-writable namespace mount scenario.
          test-dir-touch = {
            target = "${dirTouchScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
            accessPaths = [ "~/.config" ];
          };
          # Used by the suppression-env-var-target-isolation scenario.
          test-dir-env-probe = {
            target = "${dirEnvProbeScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
            accessPaths = [ "~/.config" ];
          };
          # Used by the HOME/XDG forcing scenario.
          test-dir-env-override-probe = {
            target = "${dirEnvProbeScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
            accessPaths = [ "~/.config" ];
            envSet = {
              HOME = "/tmp/underwrap-home-override";
              XDG_CONFIG_HOME = "/tmp/underwrap-config-override";
            };
          };
          test-f006-unroutable = {
            target = "${f006MarkerScript}";
            resource = "test/f006-unroutable";
            files.paths = [ ".config/f006-unroutable" ];
            accessPaths = [ "~/.config" ];
          };
          test-f006-exec-missing = {
            target = "/run/underwrap-f006-missing-target";
            resource = "test/f006-exec-missing";
            files.paths = [ ".config/f006-exec-missing" ];
            accessPaths = [ "~/.config" ];
          };
          test-f006-exit-37 = {
            target = "${f006Exit37Script}";
            resource = "test/f006-exit-37";
            files.paths = [ ".config/f006-exit-37" ];
            accessPaths = [ "~/.config" ];
          };
          test-f006-sigkill = {
            target = "${f006SigkillScript}";
            resource = "test/f006-sigkill";
            files.paths = [ ".config/f006-sigkill" ];
            accessPaths = [ "~/.config" ];
          };
          test-f007-lifecycle = {
            target = "${f007LifecycleScript}";
            resource = "test/f007-lifecycle";
            files.paths = [ ".config/f007" ];
            libsecret = true;
            accessPaths = [ "~/.config" ];
          };
          test-f007-filecap = {
            # The VM test installs a real copied sleep image and applies its
            # harmless file-capability xattr before invoking this target.
            target = "/run/underwrap-f007-filecap/sleep";
            resource = "test/f007-filecap";
            env = true;
          };
        };
      };

      # Remote instance: same frontend contract, backend socket satisfied by SSH tunnel.
      tunnel = {
        allowedUsers = [ "testuser" ];
        backend = {
          url = "https://127.0.0.1:18444";
          caFile = testCaPem;
        };

        wrappers.remote-echo = {
          target = "${pkgs.coreutils}/bin/env";
          resource = "remote/token";
          env = true;
        };
      };
    };
  };
}
