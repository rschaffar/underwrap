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
    backendName = inst.backend;
    backend = cfg.backends.${backendName};
    daemonUser = "underwrap-${instName}";
    daemonGroup = "underwrap-${instName}";
    backendUser = "underwrap-backend-${backendName}";
    backendGroup = "underwrap-backend-${backendName}";
    tunnelUser = "underwrap-${instName}-tunnel";
    wrapperGroupName = name: "underwrap-${instName}-${name}";
    wrapperSocketPath = name: "${runDir}/wrapper-${name}.sock";
    runDir = "/run/underwrap-${instName}";
    backendDir = "${runDir}/backend";
    backendSocket = "${backendDir}/backend.sock";
    stateDir = "/var/lib/underwrap-${instName}";
    backendNamespace =
      if inst.backendNamespace != null then inst.backendNamespace
      else if lib.length (lib.filter (other: other.backend == backendName) (lib.attrValues cfg.instances)) > 1 then instName
      else null;

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
    backendName = backendName;
    backendNamespace = backendNamespace;
    backendSocket = backendSocket;
    backendDir = backendDir;

    users = {
      "${daemonUser}" = {
        isSystemUser = true;
        group = daemonGroup;
        extraGroups = [ backendGroup ] ++ lib.mapAttrsToList (name: _w: wrapperGroupName name) inst.wrappers;
        home = stateDir;
        description = "underwrap daemon (${instName})";
      };
    } // lib.optionalAttrs (backend.provider == "remote") {
      "${tunnelUser}" = {
        isSystemUser = true;
        group = backendGroup;
        home = "/var/lib/${tunnelUser}";
        createHome = true;
        description = "underwrap tunnel (${instName})";
      };
    };

    groups = { "${daemonGroup}" = {}; } // lib.mapAttrs' (name: _w:
      lib.nameValuePair (wrapperGroupName name) {}
    ) inst.wrappers;

    securityWrappers = lib.mapAttrs' (name: _w:
      lib.nameValuePair "${instName}-${name}" {
        source = "${wrapperBinaries}/bin/${instName}-${name}";
        owner = "root";
        setgid = true;
        group = wrapperGroupName name;
      }
    ) inst.wrappers;

    etcFiles = lib.mapAttrs' (name: _w:
      lib.nameValuePair "underwrap/wrappers/${instName}-${name}.yaml" {
        source = wrapperConfigs.${name};
      }
    ) inst.wrappers;

    tmpfiles = [
      "d ${runDir} 0755 ${daemonUser} ${daemonGroup} -"
      "d ${backendDir} 0770 ${if backend.provider == "local" then backendUser else "root"} ${backendGroup} -"
      "d ${stateDir} 0700 ${daemonUser} ${daemonGroup} -"
    ];

    service = {
      description = "underwrap daemon (${instName})";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ] ++ lib.optionals (backend.provider == "local") [ "underwrap-backendd-${backendName}.service" ];
      requires = lib.optionals (backend.provider == "local") [ "underwrap-backendd-${backendName}.service" ];
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

    sshSettings = lib.optionalAttrs (backend.provider == "remote") {
      StreamLocalBindUnlink = "yes";
    };
  };

  mkBackend = backendName: backend: let
    backendUser = "underwrap-backend-${backendName}";
    backendGroup = "underwrap-backend-${backendName}";
    stateDir = "/var/lib/underwrap-backend-${backendName}";
    boundInstances = lib.filterAttrs (_: inst: inst.backend == backendName) cfg.instances;
    backendConfigFile = pkgs.writeText "underwrap-${backendName}-backend.yaml"
      (builtins.toJSON {
        storage = if backend.encryption.enable then {
          backend = "encrypted";
          dir = "${stateDir}/secrets";
          key_file = backend.encryption.keyFile;
        } else {
          backend = "plain";
          dir = "${stateDir}/secrets";
        };
        bindings = lib.mapAttrs (instName: _:
          let instance = instances.${instName};
          in {
            socket = instance.backendSocket;
            mode = 432;
            prefix = instance.backendNamespace;
          }
        ) boundInstances;
      });
  in {
    users = lib.optionalAttrs (backend.provider == "local") {
      "${backendUser}" = {
        isSystemUser = true;
        group = backendGroup;
        home = stateDir;
        description = "underwrap backend (${backendName})";
      };
    };

    groups = { "${backendGroup}" = {}; };

    tmpfiles = lib.optionals (backend.provider == "local") [
      "d ${stateDir} 0700 ${backendUser} ${backendGroup} -"
      "d ${stateDir}/secrets 0700 ${backendUser} ${backendGroup} -"
    ];

    service = lib.mkIf (backend.provider == "local") {
      description = "underwrap backend daemon (${backendName})";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      serviceConfig = {
        Type = "simple";
        User = backendUser;
        Group = backendGroup;
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
        ReadWritePaths = [ stateDir ] ++ map (instName: instances.${instName}.backendDir) (builtins.attrNames boundInstances);
      };
    };
  };

  # Build all instance configs.
  instances = lib.mapAttrs mkInstance cfg.instances;
  backends = lib.mapAttrs mkBackend cfg.backends;

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

  backendSubmodule = lib.types.submodule {
    options = {
      provider = lib.mkOption {
        type = lib.types.enum [ "local" "remote" ];
        default = "local";
      };
      encryption = {
        enable = lib.mkEnableOption "encrypted backend storage";
        keyFile = lib.mkOption {
          type = lib.types.path;
          default = "/dev/null";
        };
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
      backend = lib.mkOption {
        type = lib.types.str;
        description = "Name of the backend this instance uses.";
      };
      backendNamespace = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Optional namespace/prefix inside the backend storage. Defaults to the instance name when a backend is shared by multiple instances.";
      };
      credentials = lib.mkOption {
        type = lib.types.attrsOf credentialSubmodule;
        default = {};
      };
      wrappers = lib.mkOption {
        type = lib.types.attrsOf wrapperSubmodule;
        default = {};
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

    backends = lib.mkOption {
      type = lib.types.attrsOf backendSubmodule;
      default = {};
      description = "Reusable backend definitions. Multiple instances may point at the same backend.";
    };

    instances = lib.mkOption {
      type = lib.types.attrsOf instanceSubmodule;
      default = {};
      description = "Named underwrap instances, fully isolated from each other on the frontend side.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions =
      (lib.flatten (lib.mapAttrsToList (instName: inst:
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
      ) cfg.instances))
      ++ (lib.mapAttrsToList (instName: inst: {
        assertion = builtins.hasAttr inst.backend cfg.backends;
        message = "services.underwrap.instances.${instName}.backend refers to an undefined backend '${inst.backend}'";
      }) cfg.instances);

    # Merge frontend instance users plus backend users/groups.
    users.users = lib.mkMerge (
      (lib.mapAttrsToList (_: i: i.users) instances)
      ++ (lib.mapAttrsToList (_: b: b.users) backends)
    );
    users.groups = lib.mkMerge (
      (lib.mapAttrsToList (_: i: i.groups) instances)
      ++ (lib.mapAttrsToList (_: b: b.groups) backends)
    );

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
    systemd.tmpfiles.rules = lib.mkMerge (
      (lib.mapAttrsToList (_: i: i.tmpfiles) instances)
      ++ (lib.mapAttrsToList (_: b: b.tmpfiles) backends)
    );

    # One frontend service per instance, plus one local backend daemon per backend.
    systemd.services =
      (lib.mapAttrs' (name: _:
        lib.nameValuePair "underwrapd-${name}" instances.${name}.service
      ) cfg.instances)
      // lib.filterAttrs (_: v: v != {}) (lib.mapAttrs' (name: _:
        lib.nameValuePair "underwrap-backendd-${name}" backends.${name}.service
      ) cfg.backends);

    # SSH settings (enabled when any referenced backend is remote).
    services.openssh.settings = lib.mkMerge (lib.mapAttrsToList (_: i: i.sshSettings) instances);
  };
}
