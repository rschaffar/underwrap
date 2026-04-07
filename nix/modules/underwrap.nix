{ config, lib, pkgs, ... }:

let
  cfg = config.services.underwrap;

  # Per-instance helper functions. All names are prefixed with the instance name
  # to ensure full isolation between instances.
  mkInstance = instName: inst: let
    # Naming convention: underwrap-<instance>-<item>
    daemonUser = "underwrap-${instName}";
    daemonGroup = "underwrap-${instName}";
    tunnelUser = "underwrap-${instName}-tunnel";
    wrapperGroupName = name: "underwrap-${instName}-${name}";
    runDir = "/run/underwrap-${instName}";
    tunnelDir = "/run/underwrap-${instName}/tunnel";
    stateDir = "/var/lib/underwrap-${instName}";

    runtimeConfig = {
      sockets = {
        read = "${runDir}/read.sock";
        control = "${runDir}/control.sock";
        read_mode = "0666";
        control_mode = "0600";
      };
      storage = if inst.remote.enable then {
        backend = "remote";
        socket = "${tunnelDir}/backend.sock";
      } else if inst.encryption.enable then {
        backend = "encrypted";
        dir = "${stateDir}/secrets";
        key_file = inst.encryption.keyFile;
      } else {
        backend = "plain";
        dir = "${stateDir}/secrets";
      };
      wrappers = lib.mapAttrs (name: w: {
        target = w.target;
        secret = w.secret;
        injection = w.injection;
        require_unlock = w.requireUnlock;
        group = wrapperGroupName name;
        libsecret = w.libsecret;
      } // lib.optionalAttrs (w.derivation != null) {
        derivation = w.derivation;
      }) inst.wrappers;
      secrets = lib.mapAttrs (_name: s: {
        inherit (s) mutable;
      } // lib.optionalAttrs (s.labels != {}) {
        inherit (s) labels;
      }) inst.secrets;
      # Note: libsecret-enabled wrappers store items via the secretsd channel,
      # which writes directly to the StorageBackend with a scoped prefix
      # (<wrapper>/libsecret/*). These keys do not need to be pre-declared
      # in the secrets map — the channel handler enforces access control.
    };

    configFile = pkgs.writeText "underwrap-${instName}-config.yaml"
      (builtins.toJSON runtimeConfig);

    nsBinaryPath = "/run/wrappers/bin/underwrap-ns";

    wrapperConfigs = lib.mapAttrs (name: w:
      pkgs.writeText "${instName}-${name}.yaml" (builtins.toJSON ({
        target = w.target;
        env_variable = w.injection.variable or "UNDERWRAP_SECRET";
        read_socket = "${runDir}/read.sock";
        ns_binary = nsBinaryPath;
      } // lib.optionalAttrs w.libsecret {
        libsecret = true;
        dbus_daemon_binary = "${pkgs.dbus}/bin/dbus-daemon";
        secretsd_binary = "${cfg.package}/bin/underwrap-secretsd";
      }))
    ) inst.wrappers;

    wrapperBinaries = pkgs.runCommand "underwrap-${instName}-wrappers" {} (
      "mkdir -p $out/bin\n" +
      lib.concatStringsSep "\n" (lib.mapAttrsToList (name: _w:
        "cp ${cfg.package}/bin/underwrap-exec $out/bin/${instName}-${name}"
      ) inst.wrappers)
    );

  in {
    # Users and groups — fully isolated per instance.
    users = {
      "${daemonUser}" = {
        isSystemUser = true;
        group = daemonGroup;
        home = stateDir;
        description = "underwrap daemon (${instName})";
      };
    } // lib.optionalAttrs inst.remote.enable {
      "${tunnelUser}" = {
        isSystemUser = true;
        group = daemonGroup;
        home = "/var/lib/${tunnelUser}";
        createHome = true;
        description = "underwrap tunnel (${instName})";
      };
    };

    groups = { "${daemonGroup}" = {}; } // lib.mapAttrs' (name: _w:
      lib.nameValuePair (wrapperGroupName name) {}
    ) inst.wrappers;

    # security.wrappers: per-wrapper setgid binaries.
    # Wrapper binary names are prefixed: <instance>-<wrapper>.
    securityWrappers = lib.mapAttrs' (name: _w:
      lib.nameValuePair "${instName}-${name}" {
        source = "${wrapperBinaries}/bin/${instName}-${name}";
        owner = "root";
        setgid = true;
        group = wrapperGroupName name;
      }
    ) inst.wrappers;

    # Sidecar configs.
    etcFiles = lib.mapAttrs' (name: _w:
      lib.nameValuePair "underwrap/wrappers/${instName}-${name}.yaml" {
        source = wrapperConfigs.${name};
      }
    ) inst.wrappers;

    # Tmpfiles rules.
    # runDir is 0755 (not group-writable) — protects read.sock and control.sock.
    # tunnelDir is 0770 group-writable — only for the backend tunnel socket.
    tmpfiles = [
      "d ${runDir} 0755 ${daemonUser} ${daemonGroup} -"
      "d ${stateDir} 0700 ${daemonUser} ${daemonGroup} -"
      "d ${stateDir}/secrets 0700 ${daemonUser} ${daemonGroup} -"
    ] ++ lib.optionals inst.remote.enable [
      "d ${tunnelDir} 0770 ${daemonUser} ${daemonGroup} -"
    ];

    # Systemd service.
    service = {
      description = "underwrap daemon (${instName})";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      serviceConfig = {
        Type = "simple";
        User = daemonUser;
        Group = daemonGroup;
        ExecStart = "${cfg.package}/bin/underwrapd --config ${configFile}";
        Restart = "on-failure";
        RestartSec = "2s";
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        NoNewPrivileges = true;
        LimitCORE = 0;
        ReadWritePaths = [ runDir stateDir ];
      };
    };

    # SSH settings for tunnel (if remote).
    sshSettings = lib.optionalAttrs inst.remote.enable {
      StreamLocalBindUnlink = "yes";
    };
  };

  # Build all instance configs.
  instances = lib.mapAttrs mkInstance cfg.instances;

  # Submodule types.
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
      derivation = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Derivation mode.";
      };
      requireUnlock = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Require explicit unlock before reads.";
      };
      libsecret = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Enable libsecret mode: provide a private D-Bus session with a
          Secret Service provider inside the PID namespace. Enables tools
          using libsecret (e.g. gh) to store/retrieve credentials via
          org.freedesktop.secrets, with storage going through the underwrap
          backend (encrypted/remote).
        '';
      };
    };
  };

  secretSubmodule = lib.types.submodule {
    options = {
      mutable = lib.mkOption {
        type = lib.types.bool;
        default = false;
      };
      labels = lib.mkOption {
        type = lib.types.attrsOf lib.types.str;
        default = {};
      };
    };
  };

  instanceSubmodule = lib.types.submodule {
    options = {
      wrappers = lib.mkOption {
        type = lib.types.attrsOf wrapperSubmodule;
        default = {};
      };
      secrets = lib.mkOption {
        type = lib.types.attrsOf secretSubmodule;
        default = {};
      };
      encryption = {
        enable = lib.mkEnableOption "encrypted storage";
        keyFile = lib.mkOption {
          type = lib.types.path;
          default = "/dev/null";
        };
      };
      remote = {
        enable = lib.mkEnableOption "remote backend over SSH tunnel";
      };
    };
  };

in {
  options.services.underwrap = {
    enable = lib.mkEnableOption "underwrap secret broker";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The underwrap package.";
    };

    instances = lib.mkOption {
      type = lib.types.attrsOf instanceSubmodule;
      default = {};
      description = "Named underwrap instances, fully isolated from each other.";
    };
  };

  config = lib.mkIf cfg.enable {
    # Merge all instance users.
    users.users = lib.mkMerge (lib.mapAttrsToList (_: i: i.users) instances);
    users.groups = lib.mkMerge (lib.mapAttrsToList (_: i: i.groups) instances);

    # Shared underwrap-ns (one binary, all instances use it).
    security.wrappers = lib.mkMerge ([{
      underwrap-ns = {
        source = "${cfg.package}/bin/underwrap-ns";
        owner = "root";
        group = "root";
        capabilities = "cap_sys_admin+ep";
      };
    }] ++ lib.mapAttrsToList (_: i: i.securityWrappers) instances);

    # Sidecar configs.
    environment.etc = lib.mkMerge (lib.mapAttrsToList (_: i: i.etcFiles) instances);

    # Tmpfiles.
    systemd.tmpfiles.rules = lib.mkMerge (lib.mapAttrsToList (_: i: i.tmpfiles) instances);

    # One systemd service per instance.
    systemd.services = lib.mapAttrs' (name: _:
      lib.nameValuePair "underwrapd-${name}" instances.${name}.service
    ) cfg.instances;

    # SSH settings (merged from all remote instances).
    services.openssh.settings = lib.mkMerge (lib.mapAttrsToList (_: i: i.sshSettings) instances);
  };
}
