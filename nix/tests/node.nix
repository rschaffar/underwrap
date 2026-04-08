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

  environment.systemPackages = [ pkgs.openssh pkgs.libsecret ];

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
          test-libsecret = {
            target = "${pkgs.libsecret}/bin/secret-tool";
            secret = "test/libsecret-tok";
            injection = { mode = "env"; variable = "LIBSECRET_UNUSED"; };
            libsecret = true;
          };
          test-ns-pids = {
            target = "${listPidsScript}";
            secret = "test/ns-pids-tok";
            injection = { mode = "env"; variable = "NS_UNUSED"; };
            libsecret = true;  # libsecret mode creates mount namespace with fresh /proc
          };
          test-caps = {
            target = "${capsScript}";
            secret = "test/caps-tok";
            injection = { mode = "env"; variable = "CAPS_SECRET"; };
          };
        };

        secrets = {
          "test/token" = { mutable = false; };
          "test/other" = { mutable = true; };
          "test/locked-tok" = { mutable = false; };
          "test/dumpable-tok" = { mutable = false; };
          "test/crash-tok" = { mutable = false; };
          "test/libsecret-tok" = { mutable = false; };
          "test/caps-tok" = { mutable = false; };
          "test/ns-pids-tok" = { mutable = false; };
        };
      };

      # Remote instance: secrets served over SSH tunnel.
      tunnel = {
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
  };
}
