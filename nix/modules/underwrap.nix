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
    daemonUser = "underwrap-${instName}";
    daemonGroup = "underwrap-${instName}";
    tunnelUser = "underwrap-${instName}-tunnel";
    wrapperGroupName = name: "underwrap-${instName}-${name}";
    wrapperSocketPath = name: "${runDir}/wrapper-${name}.sock";
    runDir = "/run/underwrap-${instName}";
    backendDir = "${runDir}/backend";
    backendSocket = "${backendDir}/backend.sock";
    stateDir = "/var/lib/underwrap-${instName}";
    backendNamespace = null;

    runtimeConfig = {
      sockets = {
        control = "${runDir}/control.sock";
        control_mode = "0600";
      };
      backend = if inst.backend.mode == "service" then {
        mode = "service";
        url = inst.backend.url;
        ca_file = inst.backend.caFile;
        client_cert_file = inst.backend.clientCertFile;
        client_key_file = inst.backend.clientKeyFile;
      } else {
        mode = "unix-socket";
        socket = if inst.backend.socket != null then inst.backend.socket else backendSocket;
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
        lib.nameValuePair "${instName}-${name}" {
          target = w.target;
          use = w.use;
          socket = wrapperSocketPath name;
          group = wrapperGroupName name;
        }
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
    backendNamespace = backendNamespace;
    backendSocket = backendSocket;
    backendDir = backendDir;

    users = {
      "${daemonUser}" = {
        isSystemUser = true;
        group = daemonGroup;
        extraGroups = lib.mapAttrsToList (name: _w: wrapperGroupName name) inst.wrappers;
        home = stateDir;
        description = "underwrap daemon (${instName})";
      };
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
      "d ${backendDir} 0770 ${daemonUser} ${daemonGroup} -"
      "d ${stateDir} 0700 ${daemonUser} ${daemonGroup} -"
    ];

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

    sshSettings = {
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
      mode = lib.mkOption {
        type = lib.types.enum [ "unix-socket" "service" ];
        default = "unix-socket";
      };
      socket = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Backend Unix socket path when mode = unix-socket. Defaults to the instance backend socket path.";
      };
      url = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Backend service URL when mode = service.";
      };
      caFile = lib.mkOption {
        type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
        default = null;
        description = "CA certificate file used to verify the backend service when mode = service.";
      };
      clientCertFile = lib.mkOption {
        type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
        default = null;
        description = "Client certificate file used for backend mTLS when mode = service.";
      };
      clientKeyFile = lib.mkOption {
        type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
        default = null;
        description = "Client private key file used for backend mTLS when mode = service.";
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
        type = backendSubmodule;
        default = {};
        description = "Frontend backend client configuration for this instance.";
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

    instances = lib.mkOption {
      type = lib.types.attrsOf instanceSubmodule;
      default = {};
      description = "Named underwrap instances, fully isolated from each other on the frontend side.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions =
      (lib.flatten (lib.mapAttrsToList (instName: inst:
        [ {
            assertion = if inst.backend.mode == "service"
              then inst.backend.url != null && inst.backend.caFile != null && inst.backend.clientCertFile != null && inst.backend.clientKeyFile != null && inst.backend.socket == null
              else inst.backend.url == null && inst.backend.caFile == null && inst.backend.clientCertFile == null && inst.backend.clientKeyFile == null;
            message = if inst.backend.mode == "service"
              then "services.underwrap.instances.${instName}.backend: service mode requires url, caFile, clientCertFile, clientKeyFile and must not set socket"
              else "services.underwrap.instances.${instName}.backend: unix-socket mode must not set service URL or mTLS files";
          }
        ] ++ (lib.mapAttrsToList (ref: c: {
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
      ;

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

    # One frontend service per instance.
    systemd.services = lib.mapAttrs' (name: _:
      lib.nameValuePair "underwrapd-${name}" instances.${name}.service
    ) cfg.instances;

    # SSH settings.
    services.openssh.settings = lib.mkMerge (lib.mapAttrsToList (_: i: i.sshSettings) instances);
  };
}
