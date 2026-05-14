{ config, lib, pkgs, ... }:

let
  cfg = config.services.underwrap-admin-ui;

  envAssignments = lib.mapAttrsToList (name: value: "${name}=${value}") cfg.environment;
in
{
  options.services.underwrap-admin-ui = {
    enable = lib.mkEnableOption "the Next.js underwrap admin UI";

    package = lib.mkOption {
      type = lib.types.package;
      description = ''
        Package providing the `underwrap-admin-ui` standalone Next.js server.
        When using the project flake, set this to
        `self.packages.${pkgs.system}.underwrap-admin-ui-nextjs`.
      '';
    };

    listenAddress = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      description = "Address the admin UI binds to via HOSTNAME.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 3000;
      description = "Port the admin UI binds to via PORT.";
    };

    environment = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = ''
        Extra environment variables to pass to the Next.js server.
        Future tickets will use this for config paths and related runtime wiring.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.underwrap-admin-ui = {
      description = "underwrap admin UI (Next.js)";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];

      serviceConfig = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/underwrap-admin-ui";
        DynamicUser = true;
        Restart = "on-failure";
        RestartSec = "2s";
        WorkingDirectory = "/";
        Environment = [
          "HOSTNAME=${cfg.listenAddress}"
          "PORT=${toString cfg.port}"
        ] ++ envAssignments;
        NoNewPrivileges = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectSystem = "strict";
        ProtectHome = true;
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
      };
    };
  };
}
