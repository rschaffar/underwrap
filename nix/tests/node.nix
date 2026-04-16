# VM test node: both local and remote instances on one machine.
{ pkgs, underwrapPackage, nixosModule, ... }:

let
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

  environment.systemPackages = [ underwrapPackage pkgs.openssh pkgs.libsecret pkgs.python3 ];

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
          management.socket = "/run/underwrap-backend/mgmt.sock";
          stores.remote = {
            backend = "plain";
            dir = "/var/lib/underwrap-backend/secrets";
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

  systemd.services.underwrap-backend-export = {
    description = "export remote backend store socket";
    wantedBy = [ "multi-user.target" ];
    after = [ "underwrap-backendd.service" ];
    requires = [ "underwrap-backendd.service" ];
    serviceConfig = {
      Type = "oneshot";
      User = "underwrap-backend";
      Group = "underwrap-backend";
      ExecStart = pkgs.writeShellScript "underwrap-backend-export" ''
        set -eu
        count=0
        while [ "$count" -lt 200 ]; do
          if test -S /run/underwrap-backend/mgmt.sock; then
            exec ${underwrapPackage}/bin/underwrap-backend-ctl \
              --socket /run/underwrap-backend/mgmt.sock \
              export remote /run/underwrap-backend/backend.sock --mode 0600
          fi
          count=$((count + 1))
          ${pkgs.coreutils}/bin/sleep 0.05
        done
        echo "management socket never appeared" >&2
        exit 1
      '';
      RemainAfterExit = true;
    };
  };

  systemd.tmpfiles.rules = [
    "d /run/underwrap-backend 0700 underwrap-backend underwrap-backend -"
    "d /var/lib/underwrap-backend/secrets 0700 underwrap-backend underwrap-backend -"
    "d /var/lib/underwrap-backend-local 0700 underwrap-local underwrap-local -"
    "d /var/lib/underwrap-backend-local/secrets 0700 underwrap-local underwrap-local -"
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
          management.socket = "/run/underwrap-local/backend/mgmt.sock";
          stores.local = {
            backend = "plain";
            dir = "/var/lib/underwrap-backend-local/secrets";
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
      RestrictAddressFamilies = [ "AF_UNIX" ];
      NoNewPrivileges = true;
      LimitCORE = 0;
      ReadWritePaths = [ "/run/underwrap-local/backend" "/var/lib/underwrap-backend-local" ];
    };
  };

  systemd.services.underwrap-backend-export-local = {
    description = "export local backend store socket";
    wantedBy = [ "multi-user.target" ];
    after = [ "underwrap-backendd-local.service" ];
    requires = [ "underwrap-backendd-local.service" ];
    serviceConfig = {
      Type = "oneshot";
      User = "underwrap-local";
      Group = "underwrap-local";
      ExecStart = pkgs.writeShellScript "underwrap-backend-export-local" ''
        set -eu
        count=0
        while [ "$count" -lt 200 ]; do
          if test -S /run/underwrap-local/backend/mgmt.sock; then
            exec ${underwrapPackage}/bin/underwrap-backend-ctl \
              --socket /run/underwrap-local/backend/mgmt.sock \
              export local /run/underwrap-local/backend/backend.sock --mode 0660
          fi
          count=$((count + 1))
          ${pkgs.coreutils}/bin/sleep 0.05
        done
        echo "management socket never appeared" >&2
        exit 1
      '';
      RemainAfterExit = true;
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
        credentials = {
          "test/token" = { mode = "env"; variable = "MY_TOKEN"; };
          "test/other" = { mode = "env"; variable = "OTHER_SECRET"; };
          "test/locked-tok" = { mode = "env"; variable = "LOCKED_TOKEN"; };
          "test/dumpable-tok" = { mode = "env"; variable = "DUMPABLE_SECRET"; };
          "test/crash-tok" = { mode = "env"; variable = "CRASH_SECRET"; };
          "test/caps-tok" = { mode = "env"; variable = "CAPS_SECRET"; };
          "test/libsecret-store" = { mode = "libsecret"; };
          "test/ns-pids-store" = { mode = "libsecret"; };
          "test/dir-a" = {
            mode = "directory";
            path = ".config/underwrap-dir-a";
          };
          "test/dir-b" = {
            mode = "directory";
            path = ".local/share/underwrap-dir-b";
          };
        };

        wrappers = {
          test-echo = {
            target = "${pkgs.coreutils}/bin/env";
            use."test/token".access = "read-only";
          };
          test-echo2 = {
            target = "${pkgs.coreutils}/bin/echo";
            use."test/other".access = "read-only";
          };
          test-locked = {
            target = "${pkgs.coreutils}/bin/env";
            use."test/locked-tok".access = "read-only";
            unlock = { mode = "always"; maxSeconds = 60; };
          };
          test-sleep = {
            target = "${pkgs.coreutils}/bin/sleep";
            use."test/dumpable-tok".access = "read-only";
          };
          test-crash = {
            target = "${crashScript}";
            use."test/crash-tok".access = "read-only";
          };
          test-libsecret = {
            target = "${pkgs.libsecret}/bin/secret-tool";
            use."test/libsecret-store".access = "read-write";
          };
          test-ns-pids = {
            target = "${listPidsScript}";
            use."test/ns-pids-store".access = "read-only";
            envSet.PATH = pkgs.lib.makeBinPath [ pkgs.coreutils ];
          };
          test-caps = {
            target = "${capsScript}";
            use."test/caps-tok".access = "read-only";
            envSet.PATH = pkgs.lib.makeBinPath [ pkgs.gnugrep ];
          };
          test-dir-write = {
            target = "${dirWriteScript}";
            use."test/dir-a".access = "read-write";
            use."test/dir-b".access = "read-write";
          };
          test-dir-read = {
            target = "${dirReadScript}";
            use."test/dir-a".access = "read-only";
            use."test/dir-b".access = "read-only";
          };
          test-dir-readonly-write = {
            target = "${dirWriteScript}";
            use."test/dir-a".access = "read-only";
            use."test/dir-b".access = "read-only";
          };
          test-dir-sleep = {
            target = "${dirSleepScript}";
            use."test/dir-a".access = "read-only";
            use."test/dir-b".access = "read-only";
          };
          test-dir-symlink = {
            target = "${dirSymlinkScript}";
            use."test/dir-a".access = "read-write";
          };
        };
      };

      # Remote instance: same frontend contract, backend socket satisfied by SSH tunnel.
      tunnel = {
        allowedUsers = [ "testuser" ];

        credentials."remote/token" = {
          mode = "env";
          variable = "REMOTE_TOKEN";
        };

        wrappers.remote-echo = {
          target = "${pkgs.coreutils}/bin/env";
          use."remote/token".access = "read-only";
        };
      };
    };
  };
}
