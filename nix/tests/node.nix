# VM test node: both local and remote instances on one machine.
{ pkgs, underwrapPackage, goKeyringProbePackage, nixosModule, ... }:

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
  # `crates/backend-backendd/src/runtime_clients.rs::cert_fingerprint_from_pem`).
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

  # The backend now reads its store registry from a single JSON
  # document at a fixed path; we seed one document per daemon.
  remoteStoresJson = pkgs.writeText "underwrap-backend-remote-stores.json" (builtins.toJSON {
    stores = [
      {
        id = "remote";
        name = "remote";
        backend = "plain";
        dir = "/var/lib/underwrap-backend/secrets";
        description = "remote-mode test store";
      }
    ];
  });
  localStoresJson = pkgs.writeText "underwrap-backend-local-stores.json" (builtins.toJSON {
    stores = [
      {
        id = "local";
        name = "local";
        backend = "plain";
        dir = "/var/lib/underwrap-backend-local/secrets";
        description = "local-mode test store";
      }
    ];
  });

  # The backend likewise reads its runtime client-instance registry
  # from one JSON document. The cert fingerprint embedded here is what
  # `validate_runtime_client_mappings` will match incoming mTLS
  # connections against.
  remoteClientInstancesJson = pkgs.writeText "underwrap-backend-remote-client-instances.json" (builtins.toJSON {
    instances = [
      {
        id = "inst_underwrap-tunnel";
        display_name = "underwrap-tunnel";
        current_store_id = "remote";
        status = "active";
        last_seen_at = null;
        current_credential = {
          cert_fingerprint = tunnelClientFingerprint;
          cert_pem = builtins.readFile ../../testdata/pki/client-tunnel.pem;
          issued_at = null;
        };
        auto_create_wrapper_assets = false;
      }
    ];
  });
  localClientInstancesJson = pkgs.writeText "underwrap-backend-local-client-instances.json" (builtins.toJSON {
    instances = [
      {
        id = "inst_underwrap-local";
        display_name = "underwrap-local";
        current_store_id = "local";
        status = "active";
        last_seen_at = null;
        current_credential = {
          cert_fingerprint = localClientFingerprint;
          cert_pem = builtins.readFile ../../testdata/pki/client-local.pem;
          issued_at = null;
        };
        auto_create_wrapper_assets = false;
      }
    ];
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

  # Normal user who runs wrappers.
  users.users.testuser = {
    isNormalUser = true;
    uid = 1099;
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
  systemd.services.underwrap-backendd = {
    description = "underwrap backend daemon";
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "simple";
      User = "underwrap-backend";
      Group = "underwrap-backend";
      ExecStart = let
        cfg = pkgs.writeText "backend.yaml" (builtins.toJSON {
          stores_file = "/var/lib/underwrap-backend/stores.json";
          client_instances_file = "/var/lib/underwrap-backend/client-instances.json";
          users_file = "/var/lib/underwrap-backend/users.json";
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
      in "${underwrapPackage}/bin/underwrap-backendd --config ${cfg}";
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
    # The backend opens these files at boot and uses them as the
    # authoritative source for which stores exist and which mTLS
    # client certs map to which store.
    "C /var/lib/underwrap-backend/stores.json 0600 underwrap-backend underwrap-backend - ${remoteStoresJson}"
    "C /var/lib/underwrap-backend/client-instances.json 0600 underwrap-backend underwrap-backend - ${remoteClientInstancesJson}"
    "C /var/lib/underwrap-backend-local/stores.json 0600 underwrap-local underwrap-local - ${localStoresJson}"
    "C /var/lib/underwrap-backend-local/client-instances.json 0600 underwrap-local underwrap-local - ${localClientInstancesJson}"
  ];

  # SSH server for the tunnel.
  services.openssh = {
    enable = true;
    settings.StreamLocalBindUnlink = "yes";
  };

  # Local backend daemon serving the local frontend instance.
  systemd.services.underwrap-backendd-local = {
    description = "underwrap backend daemon (local)";
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "simple";
      User = "underwrap-local";
      Group = "underwrap-local";
      ExecStart = let
        cfg = pkgs.writeText "underwrap-local-backend.yaml" (builtins.toJSON {
          stores_file = "/var/lib/underwrap-backend-local/stores.json";
          client_instances_file = "/var/lib/underwrap-backend-local/client-instances.json";
          users_file = "/var/lib/underwrap-backend-local/users.json";
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
      in "${underwrapPackage}/bin/underwrap-backendd --config ${cfg}";
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
          test-keyring-probe = {
            target = "${goKeyringProbePackage}/bin/underwrap-go-keyring-probe";
            resource = "test/libsecret-store";
            libsecret = true;
          };
          test-ns-pids = {
            target = "${listPidsScript}";
            resource = "test/ns-pids-store";
            libsecret = true;
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
          };
          test-dir-read = {
            target = "${dirReadScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
          };
          test-dir-readonly-write = {
            target = "${dirWriteScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
          };
          test-dir-sleep = {
            target = "${dirSleepScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
          };
          test-dir-symlink = {
            target = "${dirSymlinkScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
          };
          # Used by the atomic-writeback grant-flip scenario.
          test-dir-write-then-sleep = {
            target = "${dirWriteThenSleepScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
          };
          # Used by the always-writable namespace mount scenario.
          test-dir-touch = {
            target = "${dirTouchScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
          };
          # Used by the suppression-env-var-target-isolation scenario.
          test-dir-env-probe = {
            target = "${dirEnvProbeScript}";
            resource = "test/dir-a";
            files.paths = [ ".config/underwrap-dir-a" ];
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
