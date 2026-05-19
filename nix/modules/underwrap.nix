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
    lib.concatLists
      (lib.mapAttrsToList (_resource: use: if use.files == null then [] else use.files.paths)
        wrapper.resources);

  # Per-instance helper functions. All names are prefixed with the instance name
  # to ensure full isolation between instances.
  mkInstance = instName: inst: let
    daemonUser = "underwrap-${instName}";
    daemonGroup = "underwrap-${instName}";
    tunnelUser = "underwrap-${instName}-tunnel";
    fullWrapperName = name: "${instName}-${name}";
    fullDebugWrapperName = name: "debug-${fullWrapperName name}";
    wrapperGroupName = name: "underwrap-${instName}-${name}";
    wrapperSocketPath = name: "${runDir}/wrapper-${name}.sock";
    runDir = "/run/underwrap-${instName}";
    stateDir = "/var/lib/underwrap-${instName}";
    # Per-instance mTLS client credential. Generated locally by
    # `underwrap-keygen --instance-config /etc/underwrap/instances/${instName}.yaml`;
    # never wired in via the module (so two instances on the same host can't
    # collide, and the operator can't paste a stale cert from one host into
    # another's enrollment).
    clientCertPath = "${stateDir}/client-${instName}.pem";
    clientKeyPath = "${stateDir}/client-${instName}.key";
    nsBinaryPath = "/run/wrappers/bin/underwrap-ns";
    debugToolTarget = "${cfg.package}/bin/underwrap-debug-tool";

    wrapperUsesLibsecret = w:
      lib.any (use: use.libsecret) (lib.attrValues w.resources);

    mkWrapperConfig = { configName, target, wrapperName, wrapper, envSet ? wrapper.envSet }:
      pkgs.writeText "${configName}.yaml" (builtins.toJSON ({
        inherit target;
        wrapper_socket = wrapperSocketPath wrapperName;
        ns_binary = nsBinaryPath;
        env_passthrough = wrapper.envPassthrough;
        env_set = envSet;
      } // lib.optionalAttrs (wrapperUsesLibsecret wrapper) {
        libsecret = true;
        dbus_daemon_binary = "${pkgs.dbus}/bin/dbus-daemon";
        secretsd_binary = "${cfg.package}/bin/underwrap-secretsd";
      }));

    wrapperConfigs =
      lib.foldl' lib.recursiveUpdate {} (lib.mapAttrsToList (name: w: {
        "${fullWrapperName name}" = mkWrapperConfig {
          configName = fullWrapperName name;
          target = w.target;
          wrapperName = name;
          wrapper = w;
        };
      } // lib.optionalAttrs w.addUnsafeDebugTool {
        "${fullDebugWrapperName name}" = mkWrapperConfig {
          configName = fullDebugWrapperName name;
          target = debugToolTarget;
          wrapperName = name;
          wrapper = w;
          envSet = w.envSet // {
            UNDERWRAP_DEBUG_INSTANCE = instName;
            UNDERWRAP_DEBUG_LOGICAL_WRAPPER = name;
            UNDERWRAP_DEBUG_WRAPPER = fullWrapperName name;
            UNDERWRAP_DEBUG_ORIGINAL_TARGET = w.target;
            UNDERWRAP_DEBUG_SECRETSD_TRACE_METADATA = "1";
          };
        };
      }) inst.wrappers);

    wrapperBinaries = pkgs.runCommand "underwrap-${instName}-wrappers" {} (
      "mkdir -p $out/bin\n" +
      lib.concatStringsSep "\n" (lib.concatLists (lib.mapAttrsToList (name: w:
        [
          "cp ${cfg.package}/bin/underwrap-exec $out/bin/${fullWrapperName name}"
        ] ++ lib.optional w.addUnsafeDebugTool
          "cp ${cfg.package}/bin/underwrap-exec $out/bin/${fullDebugWrapperName name}"
      ) inst.wrappers))
    );

    mkSecurityWrapper = executableName: groupName: {
      source = "${wrapperBinaries}/bin/${executableName}";
      owner = "root";
      setgid = true;
      group = groupName;
    };

    runtimeConfig = {
      backend = {
        url = inst.backend.url;
        ca_file = inst.backend.caFile;
        client_cert_file = clientCertPath;
        client_key_file = clientKeyPath;
      };
      wrappers = lib.mapAttrs' (name: w:
        lib.nameValuePair (fullWrapperName name) {
          target = w.target;
          logical_name = name;
          resources = lib.mapAttrs (_resource: use:
            ({ inherit (use) env libsecret; }
              // lib.optionalAttrs (use.files != null) {
                files = { inherit (use.files) paths; };
              })
          ) w.resources;
          socket = wrapperSocketPath name;
          group = wrapperGroupName name;
        }
      ) inst.wrappers;
    };

    configFile = pkgs.writeText "underwrap-${instName}-config.yaml"
      (builtins.toJSON runtimeConfig);

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

    securityWrappers =
      lib.mapAttrs' (name: _w:
        lib.nameValuePair (fullWrapperName name)
          (mkSecurityWrapper (fullWrapperName name) (wrapperGroupName name))
      ) inst.wrappers
      // lib.foldl' lib.recursiveUpdate {} (lib.mapAttrsToList (name: w:
        lib.optionalAttrs w.addUnsafeDebugTool {
          "${fullDebugWrapperName name}" =
            mkSecurityWrapper (fullDebugWrapperName name) (wrapperGroupName name);
        }
      ) inst.wrappers);

    etcFiles = (lib.mapAttrs' (configName: source:
      lib.nameValuePair "underwrap/wrappers/${configName}.yaml" {
        inherit source;
      }
    ) wrapperConfigs) // {
      "underwrap/instances/${instName}.yaml".source = configFile;
    };

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

  # Per-user wrapper namespace.
  #
  # `security.wrappers` is system-wide: every entry lands in /run/wrappers/bin
  # with a globally-unique name. We keep the instance-prefixed entries for
  # isolation, then layer per-user PATH integration on top so authorized users
  # can invoke wrappers by their bare attribute name without each operator
  # wiring up a parallel security.wrappers alias list.
  #
  # Each authorized user gets a derivation in their `users.users.<u>.packages`
  # whose `bin/` contains bare-named symlinks pointing at the prefixed setgid
  # binaries. NixOS's standard per-user profile machinery puts
  # /etc/profiles/per-user/<u>/bin on that user's PATH automatically.
  allAuthorizedUsers =
    lib.unique (lib.concatMap (i: i.allowedUsers) (lib.attrValues cfg.instances));

  # For each authorized user: the list of commands that should appear on
  # their PATH. Normal wrappers are exposed as `<wrapper>` and opted-in unsafe
  # debug wrappers as `debug-<wrapper>`.
  userBareEntries = lib.genAttrs allAuthorizedUsers (user:
    lib.concatMap (instName:
      let inst = cfg.instances.${instName}; in
      if lib.elem user inst.allowedUsers
      then lib.concatLists (lib.mapAttrsToList (wn: w:
        [
          {
            alias = wn;
            fullName = "${instName}-${wn}";
            instance = instName;
          }
        ] ++ lib.optional w.addUnsafeDebugTool {
          alias = "debug-${wn}";
          fullName = "debug-${instName}-${wn}";
          instance = instName;
        }
      ) inst.wrappers)
      else []
    ) (builtins.attrNames cfg.instances));

  mkUserBin = user: entries:
    pkgs.linkFarm "underwrap-userbin-${user}" (map (e: {
      name = "bin/${e.alias}";
      path = "/run/wrappers/bin/${e.fullName}";
    }) entries);

  # Submodule types.
  filesUseSubmodule = lib.types.submodule {
    options = {
      paths = lib.mkOption {
        type = lib.types.nonEmptyListOf lib.types.str;
        description = ''
          Target-home-relative mount paths for the resource files slot.
          The same backend resource is bind-mounted at every path in the
          list and they all share a single daemon channel, so reads
          observe identical content and writes can never race against
          themselves regardless of which mount the target writes to.
        '';
        example = [ ".config/gws" ".local/share/gws" ];
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
        description = ''
          Mount this resource's files slot at one or more target-home-relative
          paths. See <option>files.paths</option>.
        '';
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
      addUnsafeDebugTool = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Install an additional wrapper named `debug-<instance>-<wrapper>`
          plus a per-user `debug-<wrapper>` PATH alias. It reuses the real
          wrapper's GID and resource projections but replaces the target with
          `underwrap-debug-tool`, which can print secrets, spawn shells, and
          otherwise inspect the wrapped environment. Use only for local
          development.
        '';
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
    };
  };

  instanceSubmodule = lib.types.submodule {
    options = {
      allowedUsers = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [];
        description = ''
          Local users who get bare-name wrapper aliases (`<wrapper>`,
          `debug-<wrapper>`) on their PATH for this instance. This is a
          NixOS-side convenience only — wrapper execution itself is
          gated by the setgid group on `/run/wrappers/bin/<instance>-<wrapper>`,
          and authorization happens entirely on the backend via grants
          keyed on the mTLS-bound runtime client instance.
        '';
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
      ++ (lib.mapAttrsToList (user: entries:
        let
          byAlias = lib.foldl' (acc: e:
            acc // { ${e.alias} = (acc.${e.alias} or []) ++ [ e.instance ]; }
          ) {} entries;
          collisions = lib.filterAttrs (_: insts: lib.length insts > 1) byAlias;
        in {
          assertion = collisions == {};
          message = "services.underwrap: user '${user}' is authorized in multiple "
            + "instances that expose the same command name: "
            + lib.concatStringsSep "; " (lib.mapAttrsToList (alias: insts:
                "'${alias}' (in ${lib.concatStringsSep ", " insts})"
              ) collisions)
            + ". Rename one of the wrappers, disable one debug alias, or remove the user from one of the instances.";
        }
      ) userBareEntries)
      ++ lib.optionals cfg.hardenTargetHost [
        {
          assertion = swapDevicesAreHardened;
          message = "services.underwrap.hardenTargetHost is enabled but at least one swapDevice is not encrypted. Tokens paged out of a wrapped target's memory would surface as plaintext on disk. Enable randomEncryption or LUKS on every swapDevice, remove swap, or set services.underwrap.hardenTargetHost = false (and accept the weaker claim).";
        }
      ];

    # Merge all instance users, plus per-user wrapper bin packages.
    users.users = lib.mkMerge (
      (lib.mapAttrsToList (_: i: i.users) instances)
      ++ (lib.mapAttrsToList (user: entries:
        if entries == [] then {}
        else { ${user}.packages = [ (mkUserBin user entries) ]; }
      ) userBareEntries)
    );
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

    # Operator-facing helpers on PATH. `underwrap-keygen` generates a
    # self-signed client credential locally; an admin then pastes the
    # cert PEM into the backend admin UI. Exposed as a small symlink
    # derivation so the operator gets the keygen helper without also
    # pulling underwrap-debug-tool / underwrap-backendd onto every
    # user's PATH.
    environment.systemPackages = [
      (pkgs.runCommandLocal "underwrap-keygen-bin" { } ''
        mkdir -p $out/bin
        ln -s ${cfg.package}/bin/underwrap-keygen $out/bin/underwrap-keygen
      '')
    ];

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
