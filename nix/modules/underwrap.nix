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
      # Read socket is world-connectable; the daemon enforces access via
      # SO_PEERCRED GID checks, not filesystem permissions.
      read_mode = "0666";
      control_mode = "0600";
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

  # Per-wrapper copies of underwrap-exec, named after the wrapper.
  # security.wrappers needs a regular file as source (not a symlink),
  # and preserves the source binary's argv[0] behavior — so each copy
  # is named correctly for detect_wrapper_name() to work.
  wrapperBinaries = pkgs.runCommand "underwrap-wrapper-binaries" {} (
    "mkdir -p $out/bin\n" +
    lib.concatStringsSep "\n" (lib.mapAttrsToList (name: _w:
      "cp ${cfg.package}/bin/underwrap-exec $out/bin/${name}"
    ) cfg.wrappers)
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
    # Install wrappers via security.wrappers (setgid, root-owned).
    # Each wrapper is a copy of underwrap-exec named after the wrapper.
    # The NixOS security wrapper C helper preserves argv[0] as the
    # path to the source binary, which contains the wrapper name.
    # The setgid bit on the wrapper sets egid to the per-wrapper group.
    security.wrappers = lib.mapAttrs (name: _w: {
      source = "${wrapperBinaries}/bin/${name}";
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
    # /run/underwrap: world-traversable so any user can reach the read socket.
    # The read socket is world-connectable; authorization happens inside the
    # daemon via SO_PEERCRED GID verification, not filesystem permissions.
    # The control socket stays restricted to the daemon user.
    systemd.tmpfiles.rules = [
      "d /run/underwrap 0755 underwrap underwrap -"
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
