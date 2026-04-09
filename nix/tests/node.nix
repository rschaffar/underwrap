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
in {
  imports = [ nixosModule ];

  environment.systemPackages = [ pkgs.openssh pkgs.libsecret pkgs.python3 ];

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
          socket = "/run/underwrap-backend/backend.sock";
          storage = {
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

  systemd.tmpfiles.rules = [
    "d /run/underwrap-backend 0700 underwrap-backend underwrap-backend -"
    "d /var/lib/underwrap-backend/secrets 0700 underwrap-backend underwrap-backend -"
  ];

  # SSH server for the tunnel.
  services.openssh = {
    enable = true;
    settings.StreamLocalBindUnlink = "yes";
  };

  # Two underwrap instances: local storage + remote/tunnel.
  services.underwrap = {
    enable = true;
    package = underwrapPackage;

    instances = {
      # Local instance: plain storage on disk.
      local = {
        credentials = {
          "test/token" = { mode = "env"; variable = "MY_TOKEN"; };
          "test/other" = { mode = "env"; variable = "OTHER_SECRET"; };
          "test/locked-tok" = { mode = "env"; variable = "LOCKED_TOKEN"; };
          "test/dumpable-tok" = { mode = "env"; variable = "DUMPABLE_SECRET"; };
          "test/crash-tok" = { mode = "env"; variable = "CRASH_SECRET"; };
          "test/caps-tok" = { mode = "env"; variable = "CAPS_SECRET"; };
          "test/libsecret-store" = { mode = "libsecret"; };
          "test/ns-pids-store" = { mode = "libsecret"; };
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
        };
      };

      # Remote instance: secrets served over SSH tunnel.
      tunnel = {
        remote.enable = true;

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
