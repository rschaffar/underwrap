{ config, lib, pkgs, ... }:

let
  cfg = config.services.underwrap;

  # Build the runtime YAML config from Nix options.
  runtimeConfig = {
    sockets = {
      read = "/run/underwrap/read.sock";
      control = "/run/underwrap/control.sock";
    };
    storage = {
      backend = "plain";
      dir = "/var/lib/underwrap/secrets";
    };
    wrappers = lib.mapAttrs (_name: w: {
      target = w.target;
      secret = w.secret;
      injection = w.injection;
      require_unlock = w.requireUnlock;
    } // lib.optionalAttrs (w.derivation != null) {
      derivation = w.derivation;
    }) cfg.wrappers;
    secrets = lib.mapAttrs (_name: s: {
      inherit (s) mutable;
    } // lib.optionalAttrs (s.labels != {}) {
      inherit (s) labels;
    }) cfg.secrets;
  };

  configFile = pkgs.writeText "underwrap-config.yaml"
    (builtins.toJSON runtimeConfig);

  # Generate wrapper symlinks: each wrapper name -> underwrap-exec
  # Plus a sidecar YAML config for each wrapper.
  wrapperConfigs = lib.mapAttrs (name: w:
    pkgs.writeText "${name}.yaml" (builtins.toJSON {
      target = w.target;
      env_variable = w.envVariable;
      read_socket = "/run/underwrap/read.sock";
    })
  ) cfg.wrappers;

  wrappersDir = pkgs.runCommand "underwrap-wrappers" {} (
    ''
      mkdir -p $out/bin $out/etc/underwrap/wrappers
    ''
    + lib.concatStringsSep "\n" (lib.mapAttrsToList (name: _w: ''
      ln -s ${cfg.package}/bin/underwrap-exec $out/bin/${name}
      cp ${wrapperConfigs.${name}} $out/etc/underwrap/wrappers/${name}.yaml
    '') cfg.wrappers)
  );

  wrapperSubmodule = lib.types.submodule {
    options = {
      target = lib.mkOption {
        type = lib.types.str;
        description = "Absolute path to the target binary.";
      };
      secret = lib.mkOption {
        type = lib.types.str;
        description = "Secret reference (must match a key in secrets).";
      };
      injection = lib.mkOption {
        type = lib.types.attrs;
        default = { mode = "env"; variable = "UNDERWRAP_SECRET"; };
        description = "Injection mode configuration.";
      };
      envVariable = lib.mkOption {
        type = lib.types.str;
        default = "UNDERWRAP_SECRET";
        description = "Environment variable name for the secret (used in wrapper sidecar config).";
      };
      derivation = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Derivation mode (e.g. 'none').";
      };
      requireUnlock = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Whether this wrapper requires an explicit unlock before reads.";
      };
    };
  };

  secretSubmodule = lib.types.submodule {
    options = {
      mutable = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Whether this secret can be updated at runtime.";
      };
      labels = lib.mkOption {
        type = lib.types.attrsOf lib.types.str;
        default = {};
        description = "Metadata labels.";
      };
    };
  };

in {
  options.services.underwrap = {
    enable = lib.mkEnableOption "underwrap secret broker";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The underwrap package to use.";
    };

    wrappers = lib.mkOption {
      type = lib.types.attrsOf wrapperSubmodule;
      default = {};
      description = "Wrapper definitions.";
    };

    secrets = lib.mkOption {
      type = lib.types.attrsOf secretSubmodule;
      default = {};
      description = "Secret definitions.";
    };
  };

  config = lib.mkIf cfg.enable {
    # Create the underwrap system user and group.
    users.users.underwrap = {
      isSystemUser = true;
      group = "underwrap";
      home = "/var/lib/underwrap";
      description = "underwrap secret broker";
    };
    users.groups.underwrap = {};

    # Systemd tmpfiles: create runtime directory.
    # /run/underwrap is 0750 so only underwrap user and group can access sockets.
    systemd.tmpfiles.rules = [
      "d /run/underwrap 0750 underwrap underwrap -"
      "d /var/lib/underwrap 0700 underwrap underwrap -"
      "d /var/lib/underwrap/secrets 0700 underwrap underwrap -"
    ];

    # The daemon service.
    systemd.services.underwrapd = {
      description = "underwrap secret broker daemon";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];

      serviceConfig = {
        Type = "simple";
        User = "underwrap";
        Group = "underwrap";
        ExecStart = "${cfg.package}/bin/underwrapd --config ${configFile}";
        Restart = "on-failure";
        RestartSec = "2s";

        # Hardening.
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        NoNewPrivileges = true;
        ReadWritePaths = [
          "/run/underwrap"
          "/var/lib/underwrap"
        ];
      };
    };

    # Add wrapper binaries and sidecar configs to system path.
    environment.systemPackages = [ wrappersDir ];

    # Set UNDERWRAP_CONFIG_DIR so underwrap-exec finds sidecar configs.
    environment.sessionVariables.UNDERWRAP_CONFIG_DIR =
      "${wrappersDir}/etc/underwrap/wrappers";
  };
}
