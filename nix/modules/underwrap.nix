{ config, lib, pkgs, ... }:

let
  cfg = config.services.underwrap;

  # Per-wrapper group name.
  wrapperGroup = name: "underwrap-${name}";

  # Build the runtime YAML config from Nix options.
  runtimeConfig = {
    sockets = {
      read = "/run/underwrap/read.sock";
      control = "/run/underwrap/control.sock";
    };
    storage = if cfg.encryption.enable then {
      backend = "encrypted";
      dir = "/var/lib/underwrap/secrets";
      key_file = cfg.encryption.keyFile;
    } else {
      backend = "plain";
      dir = "/var/lib/underwrap/secrets";
    };
    wrappers = lib.mapAttrs (name: w: {
      target = w.target;
      secret = w.secret;
      injection = w.injection;
      require_unlock = w.requireUnlock;
      group = wrapperGroup name;
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

  # Sidecar YAML configs for underwrap-exec (one per wrapper).
  # Installed to /etc/underwrap/wrappers/<name>.yaml
  wrapperConfigs = lib.mapAttrs (name: w:
    pkgs.writeText "${name}.yaml" (builtins.toJSON {
      target = w.target;
      env_variable = w.envVariable;
      read_socket = "/run/underwrap/read.sock";
    })
  ) cfg.wrappers;

  # Per-wrapper launcher scripts. Each is a tiny script that execs
  # underwrap-exec with argv[0] set to the wrapper name so it loads
  # the correct sidecar config. This is needed because security.wrappers
  # passes the source binary path as argv[0], not the wrapper name.
  wrapperScripts = lib.mapAttrs (name: _w:
    pkgs.writeShellScript "underwrap-${name}" ''
      exec -a ${name} ${cfg.package}/bin/underwrap-exec "$@"
    ''
  ) cfg.wrappers;

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

    encryption = {
      enable = lib.mkEnableOption "encrypted storage backend";
      keyFile = lib.mkOption {
        type = lib.types.path;
        default = "/var/lib/underwrap/master.key";
        description = "Path to the master key file (32 raw bytes or 64 hex chars).";
      };
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
    users.groups = { underwrap = {}; } // lib.mapAttrs' (name: _w:
      lib.nameValuePair (wrapperGroup name) {}
    ) cfg.wrappers;

    # Install wrappers via security.wrappers (setgid, root-owned).
    # Each wrapper is the same underwrap-exec binary, installed with
    # a per-wrapper group. The kernel sets egid on exec; the daemon
    # uses SO_PEERCRED.gid to verify wrapper identity.
    security.wrappers = lib.mapAttrs (name: _w: {
      source = "${wrapperScripts.${name}}";
      owner = "root";
      setgid = true;
      group = wrapperGroup name;
    }) cfg.wrappers;

    # Sidecar configs in /etc/underwrap/wrappers/ (fixed trusted path).
    environment.etc = lib.mapAttrs' (name: _w:
      lib.nameValuePair "underwrap/wrappers/${name}.yaml" {
        source = wrapperConfigs.${name};
      }
    ) cfg.wrappers;

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
  };
}
