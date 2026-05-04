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

  wrapperDirectoryPaths = wrapper:
    lib.filter (path: path != null)
      (lib.mapAttrsToList (_resource: use: if use.files == null then null else use.files.path)
        wrapper.resources);

  # Per-instance helper functions. All names are prefixed with the instance name
  # to ensure full isolation between instances.
  mkInstance = instName: inst: let
    daemonUser = "underwrap-${instName}";
    daemonGroup = "underwrap-${instName}";
    tunnelUser = "underwrap-${instName}-tunnel";
    wrapperGroupName = name: "underwrap-${instName}-${name}";
    wrapperSocketPath = name: "${runDir}/wrapper-${name}.sock";
    runDir = "/run/underwrap-${instName}";
    stateDir = "/var/lib/underwrap-${instName}";

    runtimeConfig = {
      backend = {
        url = inst.backend.url;
        ca_file = inst.backend.caFile;
        client_cert_file = inst.backend.clientCertFile;
        client_key_file = inst.backend.clientKeyFile;
      };
      catalog_host = config.networking.hostName;
      catalog_instance = instName;
      allowed_users = inst.allowedUsers;
      wrappers = lib.mapAttrs' (name: w:
        lib.nameValuePair "${instName}-${name}" {
          target = w.target;
          logical_name = name;
          resources = lib.mapAttrs (_resource: use:
            ({ inherit (use) env libsecret; }
              // lib.optionalAttrs (use.files != null) {
                files = { inherit (use.files) path; };
              })
          ) w.resources;
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
        usesLibsecret = lib.any (use: use.libsecret) (lib.attrValues w.resources);
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
  filesUseSubmodule = lib.types.submodule {
    options = {
      path = lib.mkOption {
        type = lib.types.str;
        description = "Target-home-relative mount path for the resource files slot.";
      };
    };
  };

  resourceUseSubmodule = lib.types.submodule {
    options = {
      env = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Inject all backend-configured env variables from this resource.";
      };
      files = lib.mkOption {
        type = lib.types.nullOr filesUseSubmodule;
        default = null;
        description = "Mount this resource's files slot at the given target-home-relative path.";
      };
      libsecret = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Expose this resource's libsecret slot through a private Secret Service.";
      };
    };
  };

  wrapperSubmodule = lib.types.submodule {
    options = {
      target = lib.mkOption {
        type = lib.types.str;
        description = "Absolute path to the target binary.";
      };
      resources = lib.mkOption {
        type = lib.types.attrsOf resourceUseSubmodule;
        default = {};
        description = ''
          Resource slot projections for this wrapper. Backend policy, not
          NixOS config, decides whether projected mutable slots are read-only
          or read-write for a particular invocation.
        '';
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
        example = [ "EDITOR" ];
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
      url = lib.mkOption {
        type = lib.types.str;
        description = "Backend service URL, e.g. https://backend.example.internal:8443.";
      };
      caFile = lib.mkOption {
        type = lib.types.either lib.types.path lib.types.str;
        description = "CA certificate file used to verify the backend service certificate.";
      };
      clientCertFile = lib.mkOption {
        type = lib.types.either lib.types.path lib.types.str;
        description = "Client certificate file used for backend mTLS.";
      };
      clientKeyFile = lib.mkOption {
        type = lib.types.either lib.types.path lib.types.str;
        description = "Client private key file used for backend mTLS.";
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
      wrappers = lib.mkOption {
        type = lib.types.attrsOf wrapperSubmodule;
        default = {};
      };
    };
  };

  # Environment-variable names that must never appear in envPassthrough or
  # envSet, because allowing the caller (or the configuration) to control
  # them re-introduces dynamic-linker redirection against the target. The
  # wrapper chain already enforces NO_NEW_PRIVS + env_clear(); this assertion
  # prevents an operator from undoing it by accident.
  linkerLikeEnvPrefix = name:
    name == "LD_PRELOAD" || name == "LD_LIBRARY_PATH" || name == "LD_AUDIT"
    || lib.hasPrefix "LD_" name;

  swapDevicesAreHardened =
    lib.all (dev:
      (dev.randomEncryption.enable or false)
      || (dev.encrypted.enable or false)
    ) config.swapDevices;

in {
  options.services.underwrap = {
    enable = lib.mkEnableOption "underwrap secret broker";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The underwrap package.";
    };

    hardenTargetHost = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Apply the target-host hardening posture that underwrap's strong
        non-caller-inspection claim depends on. When enabled:

        - `kernel.yama.ptrace_scope` is set to `2`, so only processes with
          `CAP_SYS_PTRACE` can ptrace or read `/proc/<pid>/{environ,mem}`.
          This blocks a same-UID caller from inspecting the wrapper chain
          or the target, at the cost of breaking `strace`/`gdb` from
          normal users.
        - The build asserts that every configured swap device is
          encrypted (random-key or LUKS), so a token paged out of the
          target's memory cannot later surface as plaintext on disk. If
          you deliberately run without swap, this is satisfied trivially.

        Leave enabled unless you are certain the deployment does not
        need this posture.
      '';
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
        (lib.mapAttrsToList (wrapperName: wrapper:
          let
            paths = wrapperDirectoryPaths wrapper;
            pairs = lib.concatMap (left:
              map (right: { inherit left right; })
                (lib.filter (right: right != left) paths)
            ) paths;
            projections = lib.mapAttrsToList (_resource: use:
              use.env || use.files != null || use.libsecret
            ) wrapper.resources;
          in {
            assertion = wrapper.resources != {}
              && lib.all (v: v) projections
              && lib.length paths == lib.length (lib.unique paths)
              && lib.all (path: isCleanRelativePath path) paths
              && lib.all (pair: !pathsOverlap pair.left pair.right) pairs;
            message = "services.underwrap.instances.${instName}.wrappers.${wrapperName}: resources must project at least one slot and files target paths must be clean, unique, and non-overlapping";
          }
        ) inst.wrappers)
        ++ (lib.mapAttrsToList (wrapperName: wrapper: {
          assertion = lib.length (lib.filter (use: use.libsecret) (lib.attrValues wrapper.resources)) <= 1;
          message = "services.underwrap.instances.${instName}.wrappers.${wrapperName}: at most one libsecret slot may be projected";
        }) inst.wrappers)
        ++ (lib.mapAttrsToList (wrapperName: wrapper: {
          assertion = !(lib.any linkerLikeEnvPrefix wrapper.envPassthrough);
          message = "services.underwrap.instances.${instName}.wrappers.${wrapperName}.envPassthrough: LD_* environment variables must not be passed through — they would let the caller redirect dynamic linking in the target process and defeat the NO_NEW_PRIVS + env_clear() hardening";
        }) inst.wrappers)
        ++ (lib.mapAttrsToList (wrapperName: wrapper: {
          assertion = !(lib.any linkerLikeEnvPrefix (builtins.attrNames wrapper.envSet));
          message = "services.underwrap.instances.${instName}.wrappers.${wrapperName}.envSet: LD_* environment variables must not be set on the target — they defeat the dynamic-linker hardening";
        }) inst.wrappers)
      ) cfg.instances))
      ++ lib.optionals cfg.hardenTargetHost [
        {
          assertion = swapDevicesAreHardened;
          message = "services.underwrap.hardenTargetHost is enabled but at least one swapDevice is not encrypted. Tokens paged out of a wrapped target's memory would surface as plaintext on disk. Enable randomEncryption or LUKS on every swapDevice, remove swap, or set services.underwrap.hardenTargetHost = false (and accept the weaker claim).";
        }
      ];

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

    # Target-host hardening: pin kernel.yama.ptrace_scope=2 so that a
    # same-UID caller cannot ptrace, strace, or read /proc/<pid>/{environ,mem}
    # on any process in the wrapper chain. mkDefault so operators who
    # genuinely need scope=1 for debugging workflows can lower it
    # explicitly, but every underwrap deployment defaults to 2.
    boot.kernel.sysctl = lib.mkIf cfg.hardenTargetHost {
      "kernel.yama.ptrace_scope" = lib.mkDefault 2;
    };
  };
}
