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
    ${pkgs.coreutils}/bin/cat "$HOME/.local/share/underwrap-dir-b/meta"
    ${pkgs.coreutils}/bin/printf '\n'
  '';
  dirWriteScript = pkgs.writeShellScript "dir-write-target" ''
    set -eu
    mode="''${1:-seed}"
    ${pkgs.coreutils}/bin/mkdir -p "$HOME/.config/underwrap-dir-a/sub"
    ${pkgs.coreutils}/bin/mkdir -p "$HOME/.local/share/underwrap-dir-b"
    case "$mode" in
      seed)
        printf 'token-seed' > "$HOME/.config/underwrap-dir-a/token"
        printf 'nested-seed' > "$HOME/.config/underwrap-dir-a/sub/nested"
        printf 'meta-seed' > "$HOME/.local/share/underwrap-dir-b/meta"
        printf 'old-file' > "$HOME/.local/share/underwrap-dir-b/old"
        ;;
      update)
        printf 'token-updated' > "$HOME/.config/underwrap-dir-a/token"
        printf 'nested-updated' > "$HOME/.config/underwrap-dir-a/sub/nested"
        printf 'meta-updated' > "$HOME/.local/share/underwrap-dir-b/meta"
        ${pkgs.coreutils}/bin/rm -f "$HOME/.local/share/underwrap-dir-b/old"
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
    ${pkgs.coreutils}/bin/cat "$HOME/.local/share/underwrap-dir-b/meta" >/dev/null
    exec ${pkgs.coreutils}/bin/sleep "''${1:-300}"
  '';
  dirSymlinkScript = pkgs.writeShellScript "dir-symlink-target" ''
    set -eu
    ${pkgs.coreutils}/bin/mkdir -p "$HOME/.config/underwrap-dir-a"
    exec ${pkgs.coreutils}/bin/ln -s /etc/passwd "$HOME/.config/underwrap-dir-a/evil"
  '';
in {
  imports = [ nixosModule ];

  # The VM test runs two backendds on one host (a contrived stand-in
  # for "two separate deployments"), so it seeds each daemon with its
  # own store material directly under a daemon-specific `/etc/` path.
  environment.etc."underwrap-backend-remote/stores.d/remote.yaml".text = builtins.toJSON {
    backend = "plain";
    dir = "/var/lib/underwrap-backend/secrets";
    description = "remote-mode test store";
  };
  environment.etc."underwrap-backend-local/stores.d/local.yaml".text = builtins.toJSON {
    backend = "plain";
    dir = "/var/lib/underwrap-backend-local/secrets";
    description = "local-mode test store";
  };

  environment.systemPackages = [ underwrapPackage pkgs.openssh pkgs.libsecret pkgs.python3 pkgs.curl ];

  # Operator bearer token for the backend admin API. World-readable is fine
  # in the VM test; real deployments would ship this via a secrets system.
  environment.etc."underwrap-admin-token" = {
    text = "vm-test-admin-token";
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
        # Each daemon points at its own `stores.d` so it only opens
        # stores it has filesystem access to. NotFound on the
        # dynamic dir is tolerated by the backend; this test has no
        # dynamic stores.
        cfg = pkgs.writeText "backend.yaml" (builtins.toJSON {
          stores_declarative_dir = "/etc/underwrap-backend-remote/stores.d";
          stores_dynamic_dir = "/var/lib/underwrap-backend/dynamic.d";
          service = {
            listen = "127.0.0.1:18444";
            tls = {
              cert_file = testServerPem;
              key_file = testServerKey;
            };
            clients = {
              underwrap-tunnel.store = "remote";
            };
          };
          admin = {
            listen = "127.0.0.1:19444";
            tokens.ops.token_file = "/etc/underwrap-admin-token";
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
    "d /var/lib/underwrap-backend-local 0700 underwrap-local underwrap-local -"
    "d /var/lib/underwrap-backend-local/secrets 0700 underwrap-local underwrap-local -"
    # Root the admin API carves dynamic stores into (ticket 262).
    "d /var/lib/underwrap-backend-local/data-root 0700 underwrap-local underwrap-local -"
    # Where encrypted-store master keys land (ticket 234).
    "d /var/lib/underwrap-backend-local/keys 0700 underwrap-local underwrap-local -"
    # Pre-stage per-instance client credentials. In production these are
    # generated by `underwrap-keygen --instance-config …` and the operator
    # pastes the resulting PEM into the admin UI; the VM test seeds them
    # directly from a baked-in test PKI so the daemon starts already enrolled.
    "C /var/lib/underwrap-local/client-local.pem 0400 underwrap-local underwrap-local - ${testLocalClientPem}"
    "C /var/lib/underwrap-local/client-local.key 0400 underwrap-local underwrap-local - ${testLocalClientKey}"
    "C /var/lib/underwrap-tunnel/client-tunnel.pem 0400 underwrap-tunnel underwrap-tunnel - ${testTunnelClientPem}"
    "C /var/lib/underwrap-tunnel/client-tunnel.key 0400 underwrap-tunnel underwrap-tunnel - ${testTunnelClientKey}"
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
          stores_declarative_dir = "/etc/underwrap-backend-local/stores.d";
          stores_dynamic_dir = "/var/lib/underwrap-backend-local/dynamic.d";
          service = {
            listen = "127.0.0.1:18443";
            tls = {
              cert_file = testServerPem;
              key_file = testServerKey;
            };
            clients = {
              underwrap-local.store = "local";
            };
          };
          admin = {
            listen = "127.0.0.1:19443";
            tokens.ops.token_file = "/etc/underwrap-admin-token";
            # SPA-aware admin surface (ticket 262): a session key is
            # required for the cookie-based login flow. The VM test
            # uses a deterministic 32-byte file under /etc/ so the
            # test bootstrap can drive cookie auth against the
            # backend's own admin listener. `allowed_origins`
            # mirrors the per-test origin curl announces.
            session_key_file = "/etc/underwrap-admin-session.key";
            allowed_origins = [
              "http://127.0.0.1:19443"
            ];
            stores_data_root = "/var/lib/underwrap-backend-local/data-root";
            keys_dir = "/var/lib/underwrap-backend-local/keys";
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

  # Pre-seeded 32-byte session AEAD key used by the local admin
  # listener for cookie-based session encryption. World-readable in
  # the test; real deployments would ship this via sops.
  environment.etc."underwrap-admin-session.key" = {
    text = "01234567890123456789012345678901";
    mode = "0644";
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
            resources."test/token".env = true;
          };
          test-echo2 = {
            target = "${pkgs.coreutils}/bin/echo";
            resources."test/other".env = true;
          };
          test-locked = {
            target = "${pkgs.coreutils}/bin/env";
            resources."test/locked-tok".env = true;
          };
          test-sleep = {
            target = "${pkgs.coreutils}/bin/sleep";
            resources."test/dumpable-tok".env = true;
          };
          test-crash = {
            target = "${crashScript}";
            resources."test/crash-tok".env = true;
          };
          test-libsecret = {
            target = "${pkgs.libsecret}/bin/secret-tool";
            resources."test/libsecret-store".libsecret = true;
          };
          test-keyring-probe = {
            target = "${goKeyringProbePackage}/bin/underwrap-go-keyring-probe";
            resources."test/libsecret-store".libsecret = true;
          };
          test-ns-pids = {
            target = "${listPidsScript}";
            resources."test/ns-pids-store".libsecret = true;
            envSet.PATH = pkgs.lib.makeBinPath [ pkgs.coreutils ];
          };
          test-caps = {
            target = "${capsScript}";
            resources."test/caps-tok".env = true;
            envSet.PATH = pkgs.lib.makeBinPath [ pkgs.gnugrep ];
          };
          test-dir-write = {
            target = "${dirWriteScript}";
            resources."test/dir-a".files.paths = [ ".config/underwrap-dir-a" ];
            resources."test/dir-b".files.paths = [ ".local/share/underwrap-dir-b" ];
          };
          test-dir-read = {
            target = "${dirReadScript}";
            resources."test/dir-a".files.paths = [ ".config/underwrap-dir-a" ];
            resources."test/dir-b".files.paths = [ ".local/share/underwrap-dir-b" ];
          };
          test-dir-readonly-write = {
            target = "${dirWriteScript}";
            resources."test/dir-a".files.paths = [ ".config/underwrap-dir-a" ];
            resources."test/dir-b".files.paths = [ ".local/share/underwrap-dir-b" ];
          };
          test-dir-sleep = {
            target = "${dirSleepScript}";
            resources."test/dir-a".files.paths = [ ".config/underwrap-dir-a" ];
            resources."test/dir-b".files.paths = [ ".local/share/underwrap-dir-b" ];
          };
          test-dir-symlink = {
            target = "${dirSymlinkScript}";
            resources."test/dir-a".files.paths = [ ".config/underwrap-dir-a" ];
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
          resources."remote/token".env = true;
        };
      };
    };
  };
}
