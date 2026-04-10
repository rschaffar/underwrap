{ config, lib, pkgs, ... }:

let
  cfg = config.services.underwrap;

  isCleanRelativePath = path:
    path != null
    && path != ""
    && !(lib.hasPrefix "/" path)
    && lib.all (segment: segment != "" && segment != "." && segment != "..")
      (lib.splitString "/" path);

  pathsOverlap = left: right:
    left == right
    || lib.hasPrefix (left + "/") right
    || lib.hasPrefix (right + "/") left;

  wrapperDirectoryPaths = inst: wrapper:
    map (ref: inst.credentials.${ref}.path)
      (lib.filter (ref: inst.credentials.${ref}.mode == "directory")
        (builtins.attrNames wrapper.use));

  # Per-instance helper functions. All names are prefixed with the instance name
  # to ensure full isolation between instances.
  mkInstance = instName: inst: let
    # Naming convention: underwrap-<instance>-<item>
    daemonUser = "underwrap-${instName}";
    daemonGroup = "underwrap-${instName}";
    tunnelUser = "underwrap-${instName}-tunnel";
    wrapperGroupName = name: "underwrap-${instName}-${name}";
    wrapperSocketPath = name: "${runDir}/wrapper-${name}.sock";
    runDir = "/run/underwrap-${instName}";
    backendDir = "${runDir}/backend";
    backendSocket = "${backendDir}/backend.sock";
    stateDir = "/var/lib/underwrap-${instName}";

    runtimeConfig = {
      sockets = {
        control = "${runDir}/control.sock";
        control_mode = "0600";
      };
      backend = {
        socket = backendSocket;
      };
      allowed_users = inst.allowedUsers;
      credentials = lib.mapAttrs (_ref: c:
        if c.mode == "env" then {
          mode = "env";
          inherit (c) variable;
        } else if c.mode == "directory" then {
          mode = "directory";
          inherit (c) path;
        } else {
          mode = "libsecret";
        }
      ) inst.credentials;
      wrappers = lib.mapAttrs' (name: w:
        lib.nameValuePair "${instName}-${name}" ({
          target = w.target;
          use = w.use;
          unlock = {
            mode = w.unlock.mode;
            maxSeconds = w.unlock.maxSeconds;
          };
          socket = wrapperSocketPath name;
          group = wrapperGroupName name;
        } // lib.optionalAttrs (w.unlockKey != null) {
          unlock_key = w.unlockKey;
        })
      ) inst.wrappers;
    };

    configFile = pkgs.writeText "underwrap-${instName}-config.yaml"
      (builtins.toJSON runtimeConfig);

    nsBinaryPath = "/run/wrappers/bin/underwrap-ns";

    backendConfigFile = pkgs.writeText "underwrap-${instName}-backend.yaml"
      (builtins.toJSON {
        storage = if inst.encryption.enable then {
          backend = "encrypted";
          dir = "${stateDir}/secrets";
          key_file = inst.encryption.keyFile;
        } else {
          backend = "plain";
          dir = "${stateDir}/secrets";
        };
        bindings.${instName} = {
          socket = backendSocket;
          prefix = null;
        };
      });

    wrapperConfigs = lib.mapAttrs (name: w:
      let
        usesLibsecret = lib.any (ref: inst.credentials.${ref}.mode == "libsecret")
          (builtins.attrNames w.use);
      in pkgs.writeText "${instName}-${name}.yaml" (builtins.toJSON ({
        target = w.target;
        wrapper_socket = wrapperSocketPath name;
        ns_binary = nsBinaryPath;
        env_passthrough = w.envPassthrough;
        env_set = w.envSet;
      } // lib.optionalAttrs usesLibsecret {
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
        extraGroups = lib.mapAttrsToList (name: _w: wrapperGroupName name) inst.wrappers;
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
    # runDir is 0755 (not group-writable) — protects wrapper-*.sock and control.sock.
    # backendDir is 0770 group-writable so either the local backend daemon or a
    # remote-forward provider can create the backend socket where underwrapd expects it.
    tmpfiles = [
      "d ${runDir} 0755 ${daemonUser} ${daemonGroup} -"
      "d ${backendDir} 0770 ${daemonUser} ${daemonGroup} -"
      "d ${stateDir} 0700 ${daemonUser} ${daemonGroup} -"
      "d ${stateDir}/secrets 0700 ${daemonUser} ${daemonGroup} -"
    ];

    # Frontend daemon.
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
        ReadWritePaths = [ runDir ];
      };
    };

    backendService = lib.mkIf (!inst.remote.enable) {
      description = "underwrap backend daemon (${instName})";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      serviceConfig = {
        Type = "simple";
        User = daemonUser;
        Group = daemonGroup;
        ExecStart = "${cfg.package}/bin/underwrap-backendd --config ${backendConfigFile}";
        Restart = "on-failure";
        RestartSec = "2s";
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
        ReadWritePaths = [ backendDir stateDir ];
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
  credentialSubmodule = lib.types.submodule {
    options = {
      mode = lib.mkOption {
        type = lib.types.enum [ "env" "libsecret" "directory" ];
      };
      variable = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Environment variable name for env credentials.";
      };
      path = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Target-home-relative path for directory credentials.";
      };
    };
  };

  credentialUseSubmodule = lib.types.submodule {
    options = {
      access = lib.mkOption {
        type = lib.types.enum [ "read-only" "read-write" ];
        default = "read-only";
      };
    };
  };

  unlockSubmodule = lib.types.submodule {
    options = {
      mode = lib.mkOption {
        type = lib.types.enum [ "always-unlocked" "always" "optional" ];
        default = "always-unlocked";
      };
      maxSeconds = lib.mkOption {
        type = lib.types.nullOr lib.types.ints.positive;
        default = null;
      };
    };
  };

  wrapperSubmodule = lib.types.submodule {
    options = {
      target = lib.mkOption {
        type = lib.types.str;
        description = "Absolute path to the target binary.";
      };
      use = lib.mkOption {
        type = lib.types.attrsOf credentialUseSubmodule;
        default = {};
        description = "Credential usage bindings for this wrapper.";
      };
      unlock = lib.mkOption {
        type = unlockSubmodule;
        default = {};
      };
      unlockKey = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "SSH public key used for optional unlock challenge flow.";
      };
      envPassthrough = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [];
        description = ''
          Additional environment variables to pass through from the user's
          environment to the target process. These are added to the default
          safe whitelist (HOME, USER, TERM, LANG, etc.). PATH is intentionally
          not inherited; set envSet.PATH explicitly if needed.
        '';
        example = [ "GITHUB_TOKEN" "EDITOR" ];
      };
      envSet = lib.mkOption {
        type = lib.types.attrsOf lib.types.str;
        default = {};
        description = ''
          Fixed environment variables to set on the target process.
          These override any inherited values.
        '';
        example = { NO_COLOR = "1"; };
      };
    };
  };

  instanceSubmodule = lib.types.submodule {
    options = {
      allowedUsers = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [];
        description = "Usernames allowed to execute wrappers in this instance. Empty = allow all.";
      };
      credentials = lib.mkOption {
        type = lib.types.attrsOf credentialSubmodule;
        default = {};
      };
      wrappers = lib.mkOption {
        type = lib.types.attrsOf wrapperSubmodule;
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
    assertions = lib.flatten (lib.mapAttrsToList (instName: inst:
      (lib.mapAttrsToList (ref: c: {
        assertion = if c.mode == "env" then c.variable != null && c.path == null else if c.mode == "directory" then c.path != null && c.variable == null && isCleanRelativePath c.path else c.variable == null && c.path == null;
        message = if c.mode == "env" then "services.underwrap.instances.${instName}.credentials.\"${ref}\": env credentials require variable and must not set path" else if c.mode == "directory" then "services.underwrap.instances.${instName}.credentials.\"${ref}\": directory credentials require a clean relative path and must not set variable" else "services.underwrap.instances.${instName}.credentials.\"${ref}\": libsecret credentials must not set variable or path";
      }) inst.credentials)
      ++ (lib.mapAttrsToList (wrapperName: wrapper:
        let
          paths = wrapperDirectoryPaths inst wrapper;
          pairs = lib.concatMap (left:
            map (right: { inherit left right; })
              (lib.filter (right: right != left) paths)
          ) paths;
        in {
          assertion = lib.length paths == lib.length (lib.unique paths)
            && lib.all (pair: !pathsOverlap pair.left pair.right) pairs;
          message = "services.underwrap.instances.${instName}.wrappers.${wrapperName}: directory credential target paths must not overlap";
        }
      ) inst.wrappers)
    ) cfg.instances);

    # Merge all instance users.
    users.users = lib.mkMerge (lib.mapAttrsToList (_: i: i.users) instances);
    users.groups = lib.mkMerge (lib.mapAttrsToList (_: i: i.groups) instances);

    # Shared underwrap-ns (one binary, all instances use it).
    security.wrappers = lib.mkMerge ([{
      underwrap-ns = {
        source = "${cfg.package}/bin/underwrap-ns";
        owner = "root";
        group = "root";
        capabilities = "cap_sys_admin,cap_setpcap+ep";
      };
    }] ++ lib.mapAttrsToList (_: i: i.securityWrappers) instances);

    # Sidecar configs.
    environment.etc = lib.mkMerge (lib.mapAttrsToList (_: i: i.etcFiles) instances);

    # Tmpfiles.
    systemd.tmpfiles.rules = lib.mkMerge (lib.mapAttrsToList (_: i: i.tmpfiles) instances);

    # One frontend service per instance, plus a local backend daemon when the
    # instance is not configured to use a remote provider.
    systemd.services =
      (lib.mapAttrs' (name: _:
        lib.nameValuePair "underwrapd-${name}" instances.${name}.service
      ) cfg.instances)
      // lib.filterAttrs (_: v: v != {}) (lib.mapAttrs' (name: _:
        lib.nameValuePair "underwrap-backendd-${name}" instances.${name}.backendService
      ) cfg.instances);

    # SSH settings (merged from all remote instances).
    services.openssh.settings = lib.mkMerge (lib.mapAttrsToList (_: i: i.sshSettings) instances);
  };
}
