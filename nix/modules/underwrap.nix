{ config, lib, pkgs, ... }:

let
  cfg = config.services.underwrap;

  isAbsolutePathString = path:
    path != null && path != "" && lib.hasPrefix "/" path;

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
    if wrapper.files == null then [] else wrapper.files.paths;

  wrapperHasProjection = wrapper:
    wrapper.env || wrapper.files != null || wrapper.libsecret;

  # Per-instance helper functions. All names are prefixed with the instance name
  # to ensure full isolation between instances.
  #
  # Single-instance wrapper model: ONE setgid binary (`underwrap-<instance>`), ONE
  # gate group (`underwrap-<instance>`), and ONE wrapper socket per instance.
  # The wrapper name is a client-supplied selector carried over IPC and
  # validated by the daemon — it is no longer encoded in a per-wrapper binary,
  # group, or socket. The debug tool is reached via the `debug` subcommand
  # (`underwrap-<instance> debug <wrapper>`), gated by the backend policy, not by a
  # separate binary or NixOS opt-in.
  mkInstance = instName: inst: let
    daemonUser = "underwrap-${instName}";
    # The daemon's primary group doubles as the wrapper gate group: the
    # socket (mode 0660) inherits this group from the daemon's egid at bind
    # time (the daemon does not chown — startup.rs verifies the kernel-
    # assigned gid instead), and authorized users acquire it transiently
    # via setgid on `underwrap-<instance>`. underwrap-exec derives the instance
    # name by stripping the `underwrap-` prefix from its effective GID
    # group, so this name is load-bearing — it must stay
    # `underwrap-<instance>`.
    daemonGroup = "underwrap-${instName}";
    instanceGroup = daemonGroup;
    instanceBinaryName = "underwrap-${instName}";
    socketPath = "${runDir}/wrapper.sock";
    runDir = "/run/underwrap-${instName}";
    stateDir = "/var/lib/underwrap-${instName}";
    # Per-instance mTLS client credential. Generated locally by
    # `underwrap-keygen --instance-config /etc/underwrap/daemon/${instName}.yaml`;
    # never wired in via the module (so two instances on the same host can't
    # collide, and the operator can't paste a stale cert from one host into
    # another's enrollment).
    clientCertPath = "${stateDir}/client-${instName}.pem";
    clientKeyPath = "${stateDir}/client-${instName}.key";
    nsBinaryPath = "/run/wrappers/bin/underwrap-ns";

    # Minimal trusted sidecar read by `underwrap-exec` (underwrap-exec) from
    # the fixed path `/etc/underwrap/instances/<instance>.yaml`. It carries
    # only the socket and helper-binary paths — never the backend section or
    # wrapper definitions — and underwrap-exec parses it with
    # `deny_unknown_fields`, so it must stay a strict subset.
    instanceSidecar = {
      socket = socketPath;
      ns_binary = nsBinaryPath;
      dbus_daemon_binary = "${pkgs.dbus}/bin/dbus-daemon";
      secretsd_binary = "${cfg.package}/bin/underwrap-secretsd";
    };
    instanceSidecarFile = pkgs.writeText "underwrap-${instName}-sidecar.yaml"
      (builtins.toJSON instanceSidecar);

    # Full `DaemonConfig` consumed by `underwrapd --config` (and by
    # `underwrap-keygen --instance-config`, which extracts only the `backend`
    # section). Wrappers are keyed by their bare name — the daemon registers
    # those names with the backend and validates the client-supplied selector
    # against this map.
    runtimeConfig = {
      backend = {
        url = inst.backend.url;
        client_cert_file = clientCertPath;
        client_key_file = clientKeyPath;
      } // lib.optionalAttrs (inst.backend.caFile != null) {
        ca_file = inst.backend.caFile;
      };
      instance = instName;
      socket = socketPath;
      group = instanceGroup;
      ns_binary = nsBinaryPath;
      debug_tool_target = "${cfg.package}/bin/underwrap-debug-tool";
      target_runtime_dir_base = cfg.targetRuntimeDirBase;
      wrappers = lib.mapAttrs (_name: w: ({
        target = w.target;
        resource = w.resource;
        env = w.env;
        libsecret = w.libsecret;
        env_passthrough = w.envPassthrough;
        env_set = w.envSet;
        access_paths = w.accessPaths;
      } // lib.optionalAttrs (w.files != null) {
        files = { inherit (w.files) paths; };
      })) inst.wrappers;
    };

    configFile = pkgs.writeText "underwrap-${instName}-config.yaml"
      (builtins.toJSON runtimeConfig);

  in {
    users = {
      "${daemonUser}" = {
        isSystemUser = true;
        group = daemonGroup;
        home = stateDir;
        description = "underwrap daemon (${instName})";
      };
    };

    # One group per instance, doubling as the daemon group and the wrapper
    # gate group. No per-wrapper groups any more.
    groups = { "${instanceGroup}" = {}; };

    # One setgid entrypoint per instance. Identity comes from this group's
    # egid, not from argv[0] or the binary name, so the source can be the
    # plain `underwrap-exec` binary (no per-wrapper renamed copies).
    securityWrappers = {
      "${instanceBinaryName}" = {
        source = "${cfg.package}/bin/underwrap-exec";
        owner = "root";
        setgid = true;
        group = instanceGroup;
      };
    };

    etcFiles = {
      # Minimal sidecar read by underwrap-exec (fixed path, deny_unknown_fields).
      "underwrap/instances/${instName}.yaml".source = instanceSidecarFile;
      # Full DaemonConfig consumed by underwrapd and underwrap-keygen.
      "underwrap/daemon/${instName}.yaml".source = configFile;
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
        ProtectKernelLogs = true;
        ProtectClock = true;
        ProtectHostname = true;
        # NOTE: ProtectProc must NOT be "invisible"/"noaccess" here. The
        # daemon's wrapper peer-auth reads the *connecting* process's
        # /proc/<pid>/status to resolve its effective GID, and that peer
        # runs as a different (unprivileged) user. Hiding other users'
        # /proc entries would break the kernel-rooted identity check.
        # ProcSubset=pid is safe: it only hides non-process /proc files
        # (/proc/sys, /proc/kcore, …), which the daemon does not need.
        ProcSubset = "pid";
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        RestrictSUIDSGID = true;
        RestrictRealtime = true;
        RestrictNamespaces = true;
        SystemCallArchitectures = "native";
        # Remove the privileged syscalls. The daemon does NOT chown its
        # wrapper socket — it runs with the instance group as its primary
        # group, so the bound socket inherits that group, and `bind_socket`
        # verifies the gid rather than setting it (startup.rs). So @chown is
        # not re-allowed. @resources is NOT removed — the daemon calls
        # setrlimit(RLIMIT_CORE, 0) at startup (main.rs), which lives in
        # @resources; stripping it would SIGSYS the daemon on boot.
        SystemCallFilter = [ "@system-service" "~@privileged" ];
        RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_INET6" ];
        NoNewPrivileges = true;
        # The daemon holds secrets and the mTLS client identity but needs
        # no capabilities of its own — it binds an already-created socket
        # dir and connects outbound. Drop the entire bounding set.
        CapabilityBoundingSet = "";
        AmbientCapabilities = "";
        LimitCORE = 0;
        # `${stateDir}` holds the mTLS cert/key (read-only at runtime; only
        # keygen, run separately as the operator, writes there). `${runDir}`
        # holds the wrapper socket the daemon binds. /etc/underwrap is the
        # config, already read-only under ProtectSystem=strict.
        ReadWritePaths = [ runDir ];
        ReadOnlyPaths = [ stateDir "/etc/underwrap" ];
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
  # `security.wrappers` is system-wide: the single `underwrap-<instance>` setgid
  # entrypoint lands in /run/wrappers/bin, which is already on every user's
  # PATH. On top of that we layer per-user PATH integration so authorized
  # users get (a) a bare `underwrap` alias for the default instance and (b) the
  # familiar per-tool command names (`<wrapper>` / `debug-<wrapper>`) without
  # having to type the `underwrap-<instance> <wrapper>` form.
  #
  # The default instance is the one literally named `default`; only its
  # authorized users get the bare `underwrap` alias.
  #
  # Each authorized user gets a derivation in their `users.users.<u>.packages`
  # whose `bin/` holds these aliases. NixOS's standard per-user profile
  # machinery puts /etc/profiles/per-user/<u>/bin on that user's PATH.
  allAuthorizedUsers =
    lib.unique (lib.concatMap (i: i.allowedUsers) (lib.attrValues cfg.instances));

  defaultInstanceName = "default";

  # Convenience shim: a plain, UNPRIVILEGED POSIX shell script that just
  # forwards to the real `underwrap-<instance>` setgid entrypoint. It carries no
  # setgid bit and no file capabilities — all privilege and policy live
  # exclusively in `underwrap-<instance>` and the daemon. `exec` preserves argv,
  # exit status, and signals. The absolute path keeps the shim immune to the
  # caller's PATH (a moot point for an unprivileged script, but unambiguous).
  mkShim = instName: wname: isDebug:
    pkgs.writeShellScript "underwrap-shim-${instName}-${lib.optionalString isDebug "debug-"}${wname}"
      (if isDebug
       then ''exec /run/wrappers/bin/underwrap-${instName} debug ${lib.escapeShellArg wname} "$@"''
       else ''exec /run/wrappers/bin/underwrap-${instName} ${lib.escapeShellArg wname} "$@"'');

  # linkFarm entries ({ name = "bin/<alias>"; path = <store path or
  # /run/wrappers/bin link>; }) for one user across every instance they are
  # authorized in.
  #
  # Naming collisions across instances (two instances declaring the same
  # wrapper name, or a `debug-`-prefixed name clashing with another wrapper)
  # are the OPERATOR's responsibility — exactly as in the previous design,
  # where each wrapper already produced a binary literally named `<wrapper>`
  # on PATH. The module adds no collision-avoidance logic; a genuine clash
  # surfaces as a linkFarm build error, which the operator resolves by
  # renaming a wrapper or splitting the user across instances.
  userBinEntries = lib.genAttrs allAuthorizedUsers (user:
    lib.concatMap (instName:
      let inst = cfg.instances.${instName}; in
      if !(lib.elem user inst.allowedUsers)
      then []
      else
        (lib.optional (instName == defaultInstanceName) {
          name = "bin/underwrap";
          path = "/run/wrappers/bin/underwrap-${instName}";
        })
        ++ lib.concatLists (lib.mapAttrsToList (wname: _w: [
          { name = "bin/${wname}"; path = "${mkShim instName wname false}"; }
          { name = "bin/debug-${wname}"; path = "${mkShim instName wname true}"; }
        ]) inst.wrappers)
    ) (builtins.attrNames cfg.instances));

  mkUserBin = user: entries:
    pkgs.linkFarm "underwrap-userbin-${user}" entries;

  # Submodule types.
  filesUseSubmodule = lib.types.submodule {
    options = {
      paths = lib.mkOption {
        type = lib.types.nonEmptyListOf lib.types.str;
        description = ''
          Target-home-relative mount paths for the resource files slot.
          Each path gets its own staging directory inside the namespace,
          and every key in the resource's `files` section is routed to
          the mount whose path is a prefix of the key (e.g. a key
          `.config/gh/hosts.yml` lands under a `.config/gh` mount). The
          paths must be non-overlapping — one path being a prefix of
          another is rejected — so routing is unambiguous. Blob keys
          that match no configured mount abort the wrapper.
        '';
        example = [ ".config/gh" ".config/git" ];
      };
    };
  };

  wrapperSubmodule = lib.types.submodule {
    options = {
      target = lib.mkOption {
        type = lib.types.str;
        description = "Absolute path to the target binary.";
      };
      resource = lib.mkOption {
        type = lib.types.str;
        description = ''
          Backend resource id this wrapper projects into the target. Each
          wrapper projects exactly one resource; opt into the projection
          slots with <option>env</option>, <option>files</option>, and
          <option>libsecret</option>. Backend policy, not NixOS config,
          decides whether projected mutable slots are read-only or
          read-write for a particular invocation.
        '';
        example = "github/token";
      };
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
      accessPaths = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [];
        description = ''
          Directories to make available to the target via idmapped bind mounts
          that map ONLY the calling user's uid/gid to the target's dedicated
          uid/gid. Inside each listed directory, files owned by the caller
          appear owned by (and are writable as) the target, and the target's
          writes land back on disk owned by the caller; everything else is left
          untouched. Each entry is either <literal>~</literal> /
          <literal>~/&lt;rel&gt;</literal> (resolved against the calling user's home
          directory) or an absolute path. Use this to give a target access to
          just the caller-owned config/state directories it needs (e.g.
          <literal>[ "~/.config/gh" ]</literal>) instead of its whole home.
        '';
        example = [ "~/.config/gh" "~/.config/git" ];
      };
      envPassthrough = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [];
        description = ''
          Additional environment variables to pass through from the user's
          environment to the target process. These are added to the default
          safe whitelist for non-home, non-XDG-dir variables (USER, TERM,
          LANG, etc.). HOME, XDG_RUNTIME_DIR, and the base XDG directory
          variables are forced by underwrap and must not appear here. PATH is
          intentionally not inherited; set envSet.PATH explicitly if needed.
        '';
        example = [ "EDITOR" ];
      };
      envSet = lib.mkOption {
        type = lib.types.attrsOf lib.types.str;
        default = {};
        description = ''
          Fixed environment variables to set on the target process.
          These override any inherited values. HOME and the base XDG directory
          variables may be set here intentionally; XDG_RUNTIME_DIR may not — it
          is managed by underwrap (each isolated target gets its own provisioned
          runtime dir; relocate the base via
          <option>services.underwrap.targetRuntimeDirBase</option>).
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
        type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
        default = null;
        description = ''
          CA certificate file used to verify the backend service
          certificate. Leave as `null` when the backend uses a
          publicly-trusted (WebPKI) certificate; the system root
          store will be used in that case.
        '';
      };
    };
  };

  instanceSubmodule = lib.types.submodule {
    options = {
      allowedUsers = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [];
        description = ''
          Local users who get this instance's convenience aliases on their
          PATH: per declared wrapper a plain `<wrapper>` and `debug-<wrapper>`
          shim, plus a bare `underwrap` for the instance named `default`. The shims
          are unprivileged scripts that forward to the `underwrap-<instance>` setgid
          entrypoint; this is a NixOS-side convenience only. The kernel gate
          is acquiring the single `underwrap-<instance>` group via setgid on
          `/run/wrappers/bin/underwrap-<instance>`, and authorization happens
          entirely on the backend via grants keyed on the mTLS-bound runtime
          client instance.
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

  forcedTargetEnvNames = [
    "HOME"
    "XDG_RUNTIME_DIR"
    "XDG_CONFIG_HOME"
    "XDG_DATA_HOME"
    "XDG_CACHE_HOME"
    "XDG_STATE_HOME"
  ];

  swapDevicesAreHardened =
    lib.all (dev:
      (dev.randomEncryption.enable or false)
      || (dev.encrypted.enable or false)
    ) config.swapDevices;

  # --- Per-target uid isolation (story 2600 / 2670) ---
  # Distinct absolute target binary paths used by wrappers across every
  # instance. Each gets its own dedicated system user/group (a sandbox uid X,
  # distinct from any caller); underwrap-ns switches the target (and thus its
  # whole subtree) to X. Wrappers sharing a target binary share its uid (same
  # tool = same trust domain); finer granularity is future work.
  targetPaths = lib.unique (
    (lib.concatMap (inst:
      map (w: w.target) (lib.attrValues inst.wrappers)
    ) (lib.attrValues cfg.instances))
    ++ lib.optional (lib.any (inst: inst.wrappers != {}) (lib.attrValues cfg.instances))
      "${cfg.package}/bin/underwrap-debug-tool"
  );

  # Stable, collision-free sandbox user name per target path: a hash of the
  # absolute path (NOT a sequential index, so adding/removing isolated targets
  # never reshuffles which uid a path maps to — NixOS persists uids by name).
  # "underwrap-target-" (17) + 12 hex = 29 chars, under the 32-char limit.
  targetUserName = path:
    "underwrap-target-" + builtins.substring 0 12 (builtins.hashString "sha256" path);

  # Per-target sandbox users/groups. isSystemUser => /sbin/nologin, no home. Each
  # is a member of ONLY its own primary group — NEVER an instance group — so a
  # uid-X target cannot satisfy a wrapper-socket egid gate and pull further
  # secrets.
  targetUsers = lib.listToAttrs (map (path:
    let n = targetUserName path; in lib.nameValuePair n {
      isSystemUser = true;
      group = n;
      description = "underwrap target sandbox uid for ${path}";
    }) targetPaths);
  targetGroups = lib.listToAttrs (map (path:
    lib.nameValuePair (targetUserName path) {}) targetPaths);

  # Root-owned authority table: one row per target,
  # "<abs target path>\t<user>\t<group>". underwrap-ns reads this directly (keyed
  # on the path it is about to exec) and resolves the names to a numeric uid/gid
  # via getpwnam/getgrnam at runtime, then validates (reject 0). Line-based TSV
  # (not JSON) so the privileged, cap_setuid-holding parser needs no JSON library
  # — minimal attack surface. Storing NAMES (not numbers) keeps the table fully
  # declarative and lets NixOS auto-allocate + persist the uids stably by name;
  # root-ownership is the trust anchor — a compromised unprivileged stage cannot
  # influence it, so there is no uid-0 injection and no confused deputy. Absolute
  # / nix-store paths contain no tab, so a split on '\t' is unambiguous. See
  # docs/security.md and story 2600 "Authority for the uid".
  targetUidTableText = lib.concatMapStrings (path:
    "${path}\t${targetUserName path}\t${targetUserName path}\n"
  ) targetPaths;

  # Provision each isolated target its OWN runtime directory. A target runs as a
  # dedicated nologin uid X with no logind session, so logind never makes it a
  # /run/user/<uid>, and underwrap-ns cannot create one at runtime (it lacks
  # CAP_DAC_OVERRIDE/CAP_CHOWN, and /run/user is root-owned and logind's tree).
  # So the system provisions X its own dir here, keyed on the target user NAME
  # (known at eval; underwrap-ns derives the same path from X via getpwuid).
  # /run is a tmpfs, so tmpfiles recreates these every boot, before the first
  # wrapper invocation. The base is 0755 root-owned; each per-target dir is 0700
  # and self-owned (mode mirrors logind's /run/user/<uid>).
  targetRuntimeTmpfiles =
    lib.optionals (targetPaths != [])
      ([ "d ${cfg.targetRuntimeDirBase} 0755 root root -" ]
       ++ map (path:
         let n = targetUserName path; in
         "d ${cfg.targetRuntimeDirBase}/${n} 0700 ${n} ${n} -"
       ) targetPaths);

in {
  options.services.underwrap = {
    enable = lib.mkEnableOption "underwrap secret broker";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The underwrap package.";
    };

    targetRuntimeDirBase = lib.mkOption {
      type = lib.types.str;
      default = "/run/underwrap-runtime";
      description = ''
        Base directory under which underwrap provisions each isolated target's
        OWN runtime directory (one <literal>0700</literal>, self-owned dir per
        target sandbox user). A wrapped target runs as a dedicated system uid
        with no logind session, so it never gets a
        <literal>/run/user/&lt;uid&gt;</literal>; underwrap provisions it one here
        instead, and underwrap-ns exports it as the default
        <literal>XDG_RUNTIME_DIR</literal>
        (<literal>&lt;base&gt;/&lt;target-user-name&gt;</literal>). Must be an
        absolute path, and must NOT be <literal>/run/user</literal> (that is
        logind's per-login-session tree). This is system-wide rather than
        per-instance because the target sandbox users are shared across
        instances (keyed by target binary path).
      '';
    };

    hardenTargetHost = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Apply the target-host hardening posture underwrap relies on. When
        enabled:

        - `kernel.yama.ptrace_scope` is pinned to `2`. This gates the
          **ATTACH-mode** inspection vectors — `/proc/<pid>/mem`,
          `ptrace(PTRACE_ATTACH)`, and `process_vm_readv` — behind
          `CAP_SYS_PTRACE`, blocking `strace`/`gdb`/`perf` and direct memory
          reads from same-UID callers. It does **not** gate the **READ-mode**
          files `/proc/<pid>/{environ,maps,smaps,auxv}`: Yama acts only on
          `PTRACE_MODE_ATTACH`, never on `PTRACE_MODE_READ`. Those READ-mode
          files are protected by underwrap's mandatory dedicated per-target UID:
          wrapped targets and their descendants do not run as the caller, so the
          same-UID `/proc` access check fails for the caller's other processes.
          `ptrace_scope = 2` remains defense-in-depth for the ATTACH vectors.
        - The build asserts that every configured swap device is encrypted
          (random-key or LUKS), so a token paged out of the target's memory
          cannot later surface as plaintext on disk. If you deliberately run
          without swap, this is satisfied trivially.

        Leave enabled unless you are certain the deployment does not need this
        posture.
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
      [
        {
          assertion = isAbsolutePathString cfg.targetRuntimeDirBase;
          message = "services.underwrap.targetRuntimeDirBase: must be an absolute path";
        }
        {
          # /run/user is logind's per-login-session tree; provisioning the
          # target sandbox dirs there would collide with real user sessions.
          assertion = cfg.targetRuntimeDirBase != "/run/user"
            && !(lib.hasPrefix "/run/user/" cfg.targetRuntimeDirBase);
          message = "services.underwrap.targetRuntimeDirBase: must not be under /run/user (that is logind's per-login-session tree); use a dedicated base such as the default /run/underwrap-runtime";
        }
      ]
      ++ (lib.flatten (lib.mapAttrsToList (instName: inst:
        (lib.mapAttrsToList (wrapperName: wrapper:
          let
            paths = wrapperDirectoryPaths wrapper;
            pairs = lib.concatMap (left:
              map (right: { inherit left right; })
                (lib.filter (right: right != left) paths)
            ) paths;
          in {
            assertion = isAbsolutePathString wrapper.target
              && wrapper.resource != ""
              && wrapperHasProjection wrapper
              && lib.length paths == lib.length (lib.unique paths)
              && lib.all (path: isCleanRelativePath path) paths
              && lib.all (pair: !pathsOverlap pair.left pair.right) pairs;
            message = "services.underwrap.instances.${instName}.wrappers.${wrapperName}: must declare an absolute target path, a non-empty resource, enable at least one projection slot (env, files, or libsecret), and supply files paths that are clean, unique, and non-overlapping";
          }
        ) inst.wrappers)
        ++ (lib.mapAttrsToList (wrapperName: wrapper: {
          assertion = !(lib.any linkerLikeEnvPrefix wrapper.envPassthrough);
          message = "services.underwrap.instances.${instName}.wrappers.${wrapperName}.envPassthrough: LD_* environment variables must not be passed through — they would let the caller redirect dynamic linking in the target process and defeat the NO_NEW_PRIVS + env_clear() hardening";
        }) inst.wrappers)
        ++ (lib.mapAttrsToList (wrapperName: wrapper: {
          assertion = !(lib.any (name: lib.elem name forcedTargetEnvNames) wrapper.envPassthrough);
          message = "services.underwrap.instances.${instName}.wrappers.${wrapperName}.envPassthrough: HOME, XDG_RUNTIME_DIR, XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_CACHE_HOME, and XDG_STATE_HOME are forced by underwrap. Use envSet for HOME/base XDG overrides; XDG_RUNTIME_DIR is managed by underwrap (each isolated target gets its own provisioned runtime dir; relocate the base via services.underwrap.targetRuntimeDirBase)";
        }) inst.wrappers)
        ++ (lib.mapAttrsToList (wrapperName: wrapper: {
          assertion = !(lib.any linkerLikeEnvPrefix (builtins.attrNames wrapper.envSet));
          message = "services.underwrap.instances.${instName}.wrappers.${wrapperName}.envSet: LD_* environment variables must not be set on the target — they defeat the dynamic-linker hardening";
        }) inst.wrappers)
        ++ (lib.mapAttrsToList (wrapperName: wrapper: {
          assertion = !(wrapper.envSet ? XDG_RUNTIME_DIR);
          message = "services.underwrap.instances.${instName}.wrappers.${wrapperName}.envSet: XDG_RUNTIME_DIR must not be set here; it is managed by underwrap (each isolated target gets its own provisioned runtime dir; relocate the base via services.underwrap.targetRuntimeDirBase)";
        }) inst.wrappers)
        ++ (lib.mapAttrsToList (wrapperName: wrapper:
          let
            pathLikeEnvSetNames = [
              "HOME"
              "XDG_CONFIG_HOME"
              "XDG_DATA_HOME"
              "XDG_CACHE_HOME"
              "XDG_STATE_HOME"
            ];
            invalid = lib.filter (name:
              (builtins.hasAttr name wrapper.envSet)
              && !isAbsolutePathString wrapper.envSet.${name}
            ) pathLikeEnvSetNames;
          in {
            assertion = invalid == [];
            message = "services.underwrap.instances.${instName}.wrappers.${wrapperName}.envSet: HOME, XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_CACHE_HOME, and XDG_STATE_HOME must be absolute paths when set";
          }
        ) inst.wrappers)
        ++ (lib.mapAttrsToList (wrapperName: _wrapper: {
          # `debug` is the subcommand token (`underwrap-<instance> debug <wrapper>`),
          # so a wrapper may not be named `debug`. The daemon rejects this at
          # load time too; this surfaces it at eval time with a clear message.
          assertion = wrapperName != "debug";
          message = "services.underwrap.instances.${instName}.wrappers: a wrapper may not be named 'debug' — it is the reserved debug subcommand token (underwrap-${instName} debug <wrapper>)";
        }) inst.wrappers)
      ) cfg.instances))
      ++ lib.optionals cfg.hardenTargetHost [
        {
          assertion = swapDevicesAreHardened;
          message = "services.underwrap.hardenTargetHost is enabled but at least one swapDevice is not encrypted. Tokens paged out of a wrapped target's memory would surface as plaintext on disk. Enable randomEncryption or LUKS on every swapDevice, remove swap, or set services.underwrap.hardenTargetHost = false (and accept the weaker claim).";
        }
        {
          # The daemon's wrapper peer-auth gate prefers the race-free
          # SO_PEERPIDFD path (Linux 6.5+). On older kernels it falls back
          # to SO_PEERCRED + /proc, which is theoretically vulnerable to
          # PID recycling. Under the hardened posture, require 6.5+ so the
          # sole kernel-enforced identity factor is race-free.
          assertion = lib.versionAtLeast config.boot.kernelPackages.kernel.version "6.5";
          message = "services.underwrap.hardenTargetHost is enabled but the configured kernel (${config.boot.kernelPackages.kernel.version}) predates 6.5, which lacks SO_PEERPIDFD. Wrapper peer auth would use the racy SO_PEERCRED + /proc fallback. Use a >= 6.5 kernel, or set services.underwrap.hardenTargetHost = false (and accept the weaker claim).";
        }
      ];

    # Merge all instance users, plus per-user wrapper bin packages, plus the
    # per-target sandbox users (story 2600).
    users.users = lib.mkMerge (
      (lib.mapAttrsToList (_: i: i.users) instances)
      ++ (lib.mapAttrsToList (user: entries:
        if entries == [] then {}
        else { ${user}.packages = [ (mkUserBin user entries) ]; }
      ) userBinEntries)
      ++ [ targetUsers ]
    );
    users.groups = lib.mkMerge (
      (lib.mapAttrsToList (_: i: i.groups) instances)
      ++ [ targetGroups ]
    );

    # Shared underwrap-ns (one binary, all instances use it). Targets are always
    # switched to dedicated per-target uids, so cap_setuid/cap_setgid are part of
    # the mandatory execution model. They are needed transiently to switch the
    # target to its dedicated uid (cap_setuid: setresuid; cap_setgid:
    # setresgid+setgroups, and the idmap gid_map write in 2640) and are dropped
    # before the target exec. The authority for *which* uid comes from the
    # root-owned table, never from a credential underwrap-ns holds.
    security.wrappers = lib.mkMerge ([{
      underwrap-ns = {
        source = "${cfg.package}/bin/underwrap-ns";
        owner = "root";
        group = "root";
        capabilities = "cap_sys_admin,cap_setpcap,cap_setuid,cap_setgid+ep";
      };
    }] ++ lib.mapAttrsToList (_: i: i.securityWrappers) instances);

    # Operator-facing helpers on PATH. `underwrap-keygen` generates a
    # self-signed client credential locally; an admin then pastes the
    # cert PEM into the backend admin UI. Exposed as a small symlink
    # derivation so the operator gets the keygen helper without also
    # pulling underwrap-debug-tool / underwrap-backend onto every
    # user's PATH.
    environment.systemPackages = [
      (pkgs.runCommandLocal "underwrap-keygen-bin" { } ''
        mkdir -p $out/bin
        ln -s ${cfg.package}/bin/underwrap-keygen $out/bin/underwrap-keygen
      '')
    ];

    # Sidecar configs, plus the root-owned target->uid authority table (story
    # 2600). The table is non-secret (absolute paths + system user/group names),
    # root:root 0644, and is the SOLE source of truth for which uid a target
    # runs as — read directly by underwrap-ns, never relayed over the wire.
    environment.etc = lib.mkMerge (
      (lib.mapAttrsToList (_: i: i.etcFiles) instances)
      ++ lib.optional (targetPaths != []) {
        "underwrap/target-uids" = {
          text = targetUidTableText;
          mode = "0644";
          user = "root";
          group = "root";
        };
      }
    );

    # Tmpfiles: per-instance run/state dirs, plus each isolated target's own
    # provisioned runtime directory (story 1000).
    systemd.tmpfiles.rules = lib.mkMerge (
      (lib.mapAttrsToList (_: i: i.tmpfiles) instances)
      ++ [ targetRuntimeTmpfiles ]
    );

    # One frontend service per instance.
    systemd.services = lib.mapAttrs' (name: _:
      lib.nameValuePair "underwrapd-${name}" instances.${name}.service
    ) cfg.instances;

    # SSH settings.
    services.openssh.settings = lib.mkMerge (lib.mapAttrsToList (_: i: i.sshSettings) instances);

    # Target-host hardening: pin kernel.yama.ptrace_scope=2. NOTE this gates
    # only the ATTACH-mode vectors (/proc/<pid>/mem, ptrace, process_vm_readv);
    # Yama never consults PTRACE_MODE_READ, so it does NOT gate the READ-mode
    # files /proc/<pid>/{environ,maps,smaps,auxv}. Those are protected by the
    # mandatory per-target UID boundary instead. The pin stays as
    # defense-in-depth for ATTACH-mode reads. mkDefault so operators who
    # genuinely need scope=1 for debugging workflows can lower it explicitly.
    boot.kernel.sysctl = lib.mkIf cfg.hardenTargetHost {
      "kernel.yama.ptrace_scope" = lib.mkDefault 2;
    };
  };
}
