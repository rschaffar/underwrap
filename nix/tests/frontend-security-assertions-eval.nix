{
  nixpkgs,
  system,
  underwrapModule,
}:

let
  lib = nixpkgs.lib;
  pkgs = import nixpkgs { inherit system; };
  sorted = lib.sort builtins.lessThan;

  targetRuntimeAbsoluteMessage =
    "services.underwrap.targetRuntimeDirBase: must be an absolute path";
  targetRuntimeRunUserMessage =
    "services.underwrap.targetRuntimeDirBase: must not be under /run/user (that is logind's per-login-session tree); use a dedicated base such as the default /run/underwrap-runtime";
  targetUidCollisionMessage =
    "services.underwrap.instances: two (instance, wrapper) pairs produced the same truncated target-user hash; rename one pair so every target UID remains distinct";
  backendUrlMessage =
    "services.underwrap.instances.probe.backend.url: must use lowercase https://; plaintext runtime backend transport is not supported";
  wrapperStructureMessage = wrapperName:
    "services.underwrap.instances.probe.wrappers.${wrapperName}: must declare an absolute target path, a non-empty resource, enable at least one projection slot (env, files, or libsecret), and supply files paths that are clean, unique, and non-overlapping";
  passthroughLinkerMessage = wrapperName:
    "services.underwrap.instances.probe.wrappers.${wrapperName}.envPassthrough: LD_* environment variables must not be passed through — they would let the caller redirect dynamic linking in the target process and defeat the NO_NEW_PRIVS + env_clear() hardening";
  passthroughForcedMessage = wrapperName:
    "services.underwrap.instances.probe.wrappers.${wrapperName}.envPassthrough: HOME, XDG_RUNTIME_DIR, XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_CACHE_HOME, and XDG_STATE_HOME are forced by underwrap. Use envSet for HOME/base XDG overrides; XDG_RUNTIME_DIR is managed by underwrap (each isolated target gets its own provisioned runtime dir; relocate the base via services.underwrap.targetRuntimeDirBase)";
  envSetLinkerMessage = wrapperName:
    "services.underwrap.instances.probe.wrappers.${wrapperName}.envSet: LD_* environment variables must not be set on the target — they defeat the dynamic-linker hardening";
  envSetRuntimeMessage = wrapperName:
    "services.underwrap.instances.probe.wrappers.${wrapperName}.envSet: XDG_RUNTIME_DIR must not be set here; it is managed by underwrap (each isolated target gets its own provisioned runtime dir; relocate the base via services.underwrap.targetRuntimeDirBase)";
  envSetPathMessage = wrapperName:
    "services.underwrap.instances.probe.wrappers.${wrapperName}.envSet: HOME, XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_CACHE_HOME, and XDG_STATE_HOME must be absolute paths when set";
  debugWrapperMessage =
    "services.underwrap.instances.probe.wrappers: a wrapper may not be named 'debug' — it is the reserved debug subcommand token (underwrap-probe debug <wrapper>)";
  ordinarySwapMessage =
    "services.underwrap.hardenTargetHost is enabled but at least one swapDevice is not encrypted. Tokens paged out of a wrapped target's memory would surface as plaintext on disk. Enable randomEncryption or LUKS on every swapDevice, remove swap, or set services.underwrap.hardenTargetHost = false (and accept the weaker claim).";
  zramWritebackMessage =
    "services.underwrap.hardenTargetHost is enabled but the typed zram-generator settings either configure zram swap writeback or cannot be interpreted unambiguously. Use pure RAM-only zram or no swap, configure writeback only for a zram filesystem with an exact mount-point or canonical non-swap fs-type in canonical single-line typed settings, or set services.underwrap.hardenTargetHost = false (and accept the weaker claim).";
  kernelMessage = version:
    "services.underwrap.hardenTargetHost is enabled but the configured kernel (${version}) predates 6.5, which lacks SO_PEERPIDFD. Wrapper peer auth would use the racy SO_PEERCRED + /proc fallback. Use a >= 6.5 kernel, or set services.underwrap.hardenTargetHost = false (and accept the weaker claim).";

  baselineWrapper = {
    target = "/bin/true";
    resource = "probe/resource";
    env = true;
  };
  baselineInstance = {
    backend.url = "https://backend.example.invalid/runtime/";
    wrappers.probe = baselineWrapper;
  };
  baselineUnderwrap = {
    enable = true;
    package = pkgs.hello;
    hardenTargetHost = true;
    instances.probe = baselineInstance;
  };

  withProbeWrapper = wrapper:
    baselineUnderwrap
    // {
      instances.probe = baselineInstance // {
        wrappers.probe = baselineWrapper // wrapper;
      };
    };
  withNamedWrappers = wrappers:
    baselineUnderwrap
    // {
      instances.probe = baselineInstance // { inherit wrappers; };
    };
  completeConfig = underwrap: extraConfig:
    lib.recursiveUpdate {
      boot.isContainer = true;
      system.stateVersion = "25.11";
      services.underwrap = underwrap;
    } extraConfig;

  mkRow =
    {
      name,
      family,
      underwrap ? baselineUnderwrap,
      extraConfig ? { },
      message ? null,
      messageFor ? null,
      witness ? (_evaluated: true),
    }:
    {
      inherit name family message messageFor witness;
      config = completeConfig underwrap extraConfig;
    };

  structuralRows = [
    (mkRow {
      name = "target-relative";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { target = "bin/true"; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "resource-empty";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { resource = ""; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "projection-none";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { env = false; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "files-dirty-empty";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { files.paths = [ "" ]; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "files-dirty-absolute";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { files.paths = [ "/etc/underwrap" ]; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "files-dirty-empty-segment";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { files.paths = [ ".config//underwrap" ]; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "files-dirty-dot-segment";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { files.paths = [ ".config/./underwrap" ]; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "files-dirty-dotdot-segment";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { files.paths = [ ".config/../underwrap" ]; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "files-duplicate";
      family = "wrapper-structure";
      underwrap = withProbeWrapper {
        files.paths = [ ".config/underwrap" ".config/underwrap" ];
      };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "files-overlap";
      family = "wrapper-structure";
      underwrap = withProbeWrapper {
        files.paths = [ ".config" ".config/underwrap" ];
      };
      message = wrapperStructureMessage "probe";
    })
  ];

  linkerCases = [
    {
      name = "LD_PRELOAD";
      passthroughRow = "passthrough-ld-preload";
      envSetRow = "env-set-ld-preload";
    }
    {
      name = "LD_LIBRARY_PATH";
      passthroughRow = "passthrough-ld-library-path";
      envSetRow = "env-set-ld-library-path";
    }
    {
      name = "LD_AUDIT";
      passthroughRow = "passthrough-ld-audit";
      envSetRow = "env-set-ld-audit";
    }
    {
      name = "LD_DEBUG";
      passthroughRow = "passthrough-ld-generic";
      envSetRow = "env-set-ld-generic";
    }
  ];
  linkerRows = lib.concatLists (map (entry: [
    (mkRow {
      name = entry.passthroughRow;
      family = "env-passthrough-linker";
      underwrap = withProbeWrapper { envPassthrough = [ entry.name ]; };
      message = passthroughLinkerMessage "probe";
    })
    (mkRow {
      name = entry.envSetRow;
      family = "env-set-linker";
      underwrap = withProbeWrapper { envSet.${entry.name} = "1"; };
      message = envSetLinkerMessage "probe";
    })
  ]) linkerCases);

  forcedPassthroughNames = [
    "HOME"
    "XDG_RUNTIME_DIR"
    "XDG_CONFIG_HOME"
    "XDG_DATA_HOME"
    "XDG_CACHE_HOME"
    "XDG_STATE_HOME"
  ];
  forcedPassthroughRows = map (name: mkRow {
    name = "passthrough-forced-${lib.toLower name}";
    family = "env-passthrough-forced";
    underwrap = withProbeWrapper { envPassthrough = [ name ]; };
    message = passthroughForcedMessage "probe";
  }) forcedPassthroughNames;

  relativeEnvSetCases = [
    { name = "HOME"; row = "env-set-relative-home"; }
    { name = "XDG_CONFIG_HOME"; row = "env-set-relative-xdg_config_home"; }
    { name = "XDG_DATA_HOME"; row = "env-set-relative-xdg_data_home"; }
    { name = "XDG_CACHE_HOME"; row = "env-set-relative-xdg_cache_home"; }
    { name = "XDG_STATE_HOME"; row = "env-set-relative-xdg_state_home"; }
  ];
  relativeEnvSetRows = map (entry: mkRow {
    name = entry.row;
    family = "env-set-path";
    underwrap = withProbeWrapper { envSet.${entry.name} = "relative/path"; };
    message = envSetPathMessage "probe";
  }) relativeEnvSetCases;

  collisionInputs = [ "probe/w10544094" "probe/w10571720" ];
  collisionPrefix = value:
    builtins.substring 0 12 (builtins.hashString "sha256" value);

  rows = [
    (mkRow {
      name = "target-runtime-relative";
      family = "target-runtime-absolute";
      underwrap = baselineUnderwrap // { targetRuntimeDirBase = "run/underwrap"; };
      message = targetRuntimeAbsoluteMessage;
    })
    (mkRow {
      name = "target-runtime-run-user";
      family = "target-runtime-run-user";
      underwrap = baselineUnderwrap // { targetRuntimeDirBase = "/run/user"; };
      message = targetRuntimeRunUserMessage;
    })
    (mkRow {
      name = "target-runtime-run-user-descendant";
      family = "target-runtime-run-user";
      underwrap = baselineUnderwrap // { targetRuntimeDirBase = "/run/user/underwrap"; };
      message = targetRuntimeRunUserMessage;
    })
    (mkRow {
      name = "target-uid-hash-collision";
      family = "target-uid-collision";
      underwrap = withNamedWrappers {
        w10544094 = baselineWrapper;
        w10571720 = baselineWrapper;
      };
      message = targetUidCollisionMessage;
      witness = _evaluated:
        builtins.elemAt collisionInputs 0 != builtins.elemAt collisionInputs 1
        && collisionPrefix (builtins.elemAt collisionInputs 0) == "ef4af79465f1"
        && collisionPrefix (builtins.elemAt collisionInputs 1) == "ef4af79465f1";
    })
    (mkRow {
      name = "backend-url-plaintext";
      family = "backend-url";
      underwrap = baselineUnderwrap // {
        instances.probe = baselineInstance // {
          backend.url = "http://backend.example.invalid/runtime/";
        };
      };
      message = backendUrlMessage;
    })
  ]
  ++ structuralRows
  ++ linkerRows
  ++ forcedPassthroughRows
  ++ [
    (mkRow {
      name = "env-set-runtime-xdg-runtime-dir";
      family = "env-set-runtime";
      underwrap = withProbeWrapper { envSet.XDG_RUNTIME_DIR = "/run/target"; };
      message = envSetRuntimeMessage "probe";
    })
  ]
  ++ relativeEnvSetRows
  ++ [
    (mkRow {
      name = "wrapper-debug-reserved";
      family = "wrapper-name";
      underwrap = withNamedWrappers { debug = baselineWrapper; };
      message = debugWrapperMessage;
    })
    (mkRow {
      name = "swap-ordinary-plain";
      family = "ordinary-swap";
      extraConfig.swapDevices = [ { device = "/dev/underwrap-test-ordinary-swap"; } ];
      message = ordinarySwapMessage;
    })
    (mkRow {
      name = "zram-active-writeback";
      family = "zram-writeback";
      extraConfig.zramSwap = {
        enable = true;
        swapDevices = 1;
        writebackDevice = "/dev/underwrap-test-zram-writeback";
      };
      message = zramWritebackMessage;
    })
    (mkRow {
      name = "kernel-pre-6-5";
      family = "kernel";
      extraConfig.boot.kernelPackages = pkgs.linuxPackages_6_1;
      messageFor = evaluated: kernelMessage evaluated.config.boot.kernelPackages.kernel.version;
      witness = evaluated:
        !lib.versionAtLeast evaluated.config.boot.kernelPackages.kernel.version "6.5";
    })
  ];

  evaluate = moduleConfig:
    nixpkgs.lib.nixosSystem {
      inherit system;
      modules = [
        underwrapModule
        ({ ... }: moduleConfig)
      ];
    };

  underwrapAssertionsOf = evaluated:
    let
      definitions = lib.filter
        (definition: lib.hasSuffix "/nix/modules/underwrap.nix" definition.file)
        evaluated.options.assertions.definitionsWithLocations;
    in
    if lib.length definitions == 1
    then (builtins.head definitions).value
    else throw "frontend security assertion matrix: expected exactly one assertion definition sourced from nix/modules/underwrap.nix";

  expectedAssertionMessages = kernelVersion: sorted [
    targetRuntimeAbsoluteMessage
    targetRuntimeRunUserMessage
    targetUidCollisionMessage
    backendUrlMessage
    (wrapperStructureMessage "probe")
    (passthroughLinkerMessage "probe")
    (passthroughForcedMessage "probe")
    (envSetLinkerMessage "probe")
    (envSetRuntimeMessage "probe")
    (envSetPathMessage "probe")
    debugWrapperMessage
    ordinarySwapMessage
    zramWritebackMessage
    (kernelMessage kernelVersion)
  ];

  ensure = label: condition:
    if condition then true else throw "frontend security assertion matrix: ${label}";

  baseline = evaluate (completeConfig baselineUnderwrap { });
  baselineUnderwrapAssertions = underwrapAssertionsOf baseline;
  baselineExpectedMessages = expectedAssertionMessages baseline.config.boot.kernelPackages.kernel.version;
  baselineActualMessages = sorted (map (entry: entry.message) baselineUnderwrapAssertions);
  baselineToplevel = builtins.tryEval baseline.config.system.build.toplevel;
  checkedBaseline =
    assert ensure "baseline expected assertion inventory does not contain exactly 14 messages"
      (lib.length baselineExpectedMessages == 14);
    assert ensure "baseline production assertion inventory differs"
      (baselineActualMessages == baselineExpectedMessages);
    assert ensure "baseline underwrap assertion is false"
      (lib.all (entry: entry.assertion) baselineUnderwrapAssertions);
    assert ensure "baseline global assertion is false"
      (lib.all (entry: entry.assertion) baseline.config.assertions);
    assert ensure "baseline system.build.toplevel evaluation failed"
      baselineToplevel.success;
    true;

  checkRow = row:
    let
      evaluated = evaluate row.config;
      underwrapAssertions = underwrapAssertionsOf evaluated;
      falseUnderwrapMessages = sorted (
        map (entry: entry.message) (lib.filter (entry: !entry.assertion) underwrapAssertions)
      );
      falseAssertionCount = lib.length (
        lib.filter (entry: !entry.assertion) evaluated.config.assertions
      );
      expectedMessage =
        if row.messageFor != null then row.messageFor evaluated else row.message;
      toplevel = builtins.tryEval evaluated.config.system.build.toplevel;
    in
    assert checkedBaseline;
    assert ensure "row ${row.name}: exact singleton false underwrap message differs"
      (falseUnderwrapMessages == [ expectedMessage ]);
    assert ensure "row ${row.name}: false global assertion count differs"
      (falseAssertionCount == 1);
    assert ensure "row ${row.name}: system.build.toplevel unexpectedly evaluated"
      (!toplevel.success);
    assert ensure "row ${row.name}: independent witness failed"
      (row.witness evaluated);
    {
      inherit (row) name family;
      message = expectedMessage;
      toplevelSuccess = toplevel.success;
    };

  checkedRows = map checkRow rows;
  matrixRecord = lib.concatMapStringsSep "\n" (row:
    "${row.name}\t${row.family}\t${row.message}\t${lib.boolToString row.toplevelSuccess}"
  ) checkedRows;
in
pkgs.runCommand "underwrap-frontend-security-assertions-eval" {
  inherit matrixRecord;
} ''
  mkdir -p "$out"
  printf 'row\tfamily\tmessage\ttoplevel\n' > "$out/matrix.tsv"
  printf '%s\n' "$matrixRecord" >> "$out/matrix.tsv"
''
