{
  nixpkgs,
  system,
  underwrapModule,
}:

let
  lib = nixpkgs.lib;
  pkgs = import nixpkgs { inherit system; };
  sorted = lib.sort builtins.lessThan;
  unicodeCharacter = codepoint:
    builtins.fromJSON ("\"\\u" + codepoint + "\"");
  nonBreakingSpace = unicodeCharacter "00A0";
  nextLineControl = unicodeCharacter "0085";

  targetRuntimeAbsoluteMessage =
    "services.underwrap.targetRuntimeDirBase: must be a canonical absolute path";
  targetRuntimeRunUserMessage =
    "services.underwrap.targetRuntimeDirBase: must not be under /run/user (that is logind's per-login-session tree); use a dedicated base such as the default /run/underwrap-runtime";
  targetUidCollisionMessage =
    "services.underwrap.instances: two (instance, wrapper) pairs produced the same truncated target-user hash; rename one pair so every target UID remains distinct";
  instanceNameMessage =
    "services.underwrap.instances: instance names must match [a-z][a-z0-9_-]{0,20}";
  backendUrlMessage =
    "services.underwrap.instances.probe.backend.url: must use lowercase https://; plaintext runtime backend transport is not supported";
  backendCaPathMessage =
    "services.underwrap.instances.probe.backend.caFile: must be a canonical absolute path when set";
  wrapperStructureMessage = wrapperName:
    "services.underwrap.instances.probe.wrappers.${wrapperName}: must declare a canonical absolute target path, a non-empty resource, enable at least one projection slot (env, files, or libsecret), and supply canonical relative files paths that are unique and non-overlapping";
  accessPathsMessage = wrapperName:
    "services.underwrap.instances.probe.wrappers.${wrapperName}.accessPaths: entries must be canonical absolute paths, exact '~', or canonical '~/...' paths, with duplicates and overlaps rejected within each path domain";
  passthroughLinkerMessage = wrapperName:
    "services.underwrap.instances.probe.wrappers.${wrapperName}.envPassthrough: LD_* environment variables must not be passed through — they would let the caller redirect dynamic linking in the target process and defeat the NO_NEW_PRIVS + env_clear() hardening";
  passthroughForcedMessage = wrapperName:
    "services.underwrap.instances.probe.wrappers.${wrapperName}.envPassthrough: HOME, XDG_RUNTIME_DIR, XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_CACHE_HOME, and XDG_STATE_HOME are forced by underwrap. Use envSet for HOME/base XDG overrides; XDG_RUNTIME_DIR is managed by underwrap (each isolated target gets its own provisioned runtime dir; relocate the base via services.underwrap.targetRuntimeDirBase)";
  envSetLinkerMessage = wrapperName:
    "services.underwrap.instances.probe.wrappers.${wrapperName}.envSet: LD_* environment variables must not be set on the target — they defeat the dynamic-linker hardening";
  envSetRuntimeMessage = wrapperName:
    "services.underwrap.instances.probe.wrappers.${wrapperName}.envSet: XDG_RUNTIME_DIR must not be set here; it is managed by underwrap (each isolated target gets its own provisioned runtime dir; relocate the base via services.underwrap.targetRuntimeDirBase)";
  envSetPathMessage = wrapperName:
    "services.underwrap.instances.probe.wrappers.${wrapperName}.envSet: HOME, XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_CACHE_HOME, and XDG_STATE_HOME must be canonical absolute paths when set";
  wrapperNameMessage =
    "services.underwrap.instances.probe.wrappers: wrapper names must match [A-Za-z0-9][A-Za-z0-9._+-]{0,127}";
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
    accessPaths = [ "/home/caller/.config" "~/.config" ];
    envSet = {
      HOME = "/home/caller";
      XDG_CONFIG_HOME = "/home/caller/.config";
      XDG_DATA_HOME = "/home/caller/.local/share";
      XDG_CACHE_HOME = "/home/caller/.cache";
      XDG_STATE_HOME = "/home/caller/.local/state";
      PATH = "relative path remains unrestricted";
    };
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
  withInstanceName = name:
    baselineUnderwrap
    // {
      instances = { "${name}" = baselineInstance; };
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
      name = "target-empty-component";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { target = "/bin//true"; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "target-dot-component";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { target = "/bin/./true"; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "target-trailing-slash";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { target = "/bin/true/"; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "target-whitespace";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { target = "/bin/bad target"; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "target-control";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { target = "/bin/bad\ttarget"; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "target-unicode-nbsp-whitespace";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { target = "/bin/bad${nonBreakingSpace}target"; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "target-unicode-u0085-control";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { target = "/bin/bad${nextLineControl}target"; };
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
    (mkRow {
      name = "files-trailing-slash";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { files.paths = [ ".config/underwrap/" ]; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "files-whitespace";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { files.paths = [ ".config/under wrap" ]; };
      message = wrapperStructureMessage "probe";
    })
    (mkRow {
      name = "files-control";
      family = "wrapper-structure";
      underwrap = withProbeWrapper { files.paths = [ ".config/under\twrap" ]; };
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

  accessRows = map (entry: mkRow {
    inherit (entry) name;
    family = "access-paths";
    underwrap = withProbeWrapper { accessPaths = entry.paths; };
    message = accessPathsMessage "probe";
  }) [
    { name = "access-relative"; paths = [ "relative/path" ]; }
    { name = "access-empty-tilde-relative"; paths = [ "~/" ]; }
    { name = "access-empty-component"; paths = [ "~/a//b" ]; }
    { name = "access-dot-component"; paths = [ "/a/./b" ]; }
    { name = "access-dotdot-component"; paths = [ "~/a/../b" ]; }
    { name = "access-whitespace"; paths = [ "/a bad" ]; }
    { name = "access-control"; paths = [ "~/a\tbad" ]; }
    { name = "access-absolute-duplicate"; paths = [ "/a" "/a" ]; }
    { name = "access-absolute-overlap"; paths = [ "/a" "/a/b" ]; }
    { name = "access-absolute-root-overlap"; paths = [ "/" "/a" ]; }
    { name = "access-tilde-duplicate"; paths = [ "~/a" "~/a" ]; }
    { name = "access-tilde-overlap"; paths = [ "~" "~/a" ]; }
  ];

  collisionInputs = [ "probe/w10544094" "probe/w10571720" ];
  collisionPrefix = value:
    builtins.substring 0 12 (builtins.hashString "sha256" value);

  instanceMaxName = "a" + builtins.concatStringsSep "" (lib.replicate 20 "z");
  instanceTooLongName = "a" + builtins.concatStringsSep "" (lib.replicate 21 "z");
  wrapperMaxName = "A" + builtins.concatStringsSep "" (lib.replicate 127 "z");
  wrapperTooLongName = "A" + builtins.concatStringsSep "" (lib.replicate 128 "z");

  validRows = [
    {
      name = "instance-min-boundary";
      underwrap = withInstanceName "a";
    }
    {
      name = "instance-max-boundary";
      underwrap = withInstanceName instanceMaxName;
    }
    {
      name = "wrapper-min-boundary";
      underwrap = withNamedWrappers { A = baselineWrapper; };
    }
    {
      name = "wrapper-max-boundary";
      underwrap = withNamedWrappers { "${wrapperMaxName}" = baselineWrapper; };
    }
    {
      name = "canonical-path-boundaries-and-domain-separation";
      underwrap = baselineUnderwrap // {
        targetRuntimeDirBase = "/";
        instances.probe = baselineInstance // {
          backend = baselineInstance.backend // { caFile = "/"; };
          wrappers.probe = baselineWrapper // {
            target = "/";
            files.paths = [ "single" ];
            accessPaths = [ "/same/path" "~/same/path" ];
            envSet = baselineWrapper.envSet // {
              HOME = "/";
              PATH = "relative path remains unrestricted";
            };
          };
        };
      };
    }
    {
      name = "ordinary-unicode-path-and-exact-tilde";
      underwrap = withProbeWrapper {
        target = "/opt/café/設定";
        files.paths = [ "設定/probe" ];
        accessPaths = [ "~" ];
        envSet = baselineWrapper.envSet // { HOME = "/home/利用者"; };
      };
    }
    {
      name = "backend-ca-write-text-derivation";
      underwrap = baselineUnderwrap // {
        instances.probe = baselineInstance // {
          backend = baselineInstance.backend // {
            caFile = pkgs.writeText "underwrap-test-ca.pem" "test CA fixture\n";
          };
        };
      };
    }
  ];

  rows = [
    (mkRow {
      name = "target-runtime-relative";
      family = "target-runtime-absolute";
      underwrap = baselineUnderwrap // { targetRuntimeDirBase = "run/underwrap"; };
      message = targetRuntimeAbsoluteMessage;
    })
    (mkRow {
      name = "target-runtime-empty-component";
      family = "target-runtime-absolute";
      underwrap = baselineUnderwrap // { targetRuntimeDirBase = "/run//underwrap"; };
      message = targetRuntimeAbsoluteMessage;
    })
    (mkRow {
      name = "target-runtime-dot-component";
      family = "target-runtime-absolute";
      underwrap = baselineUnderwrap // { targetRuntimeDirBase = "/run/./underwrap"; };
      message = targetRuntimeAbsoluteMessage;
    })
    (mkRow {
      name = "target-runtime-dotdot-component";
      family = "target-runtime-absolute";
      underwrap = baselineUnderwrap // { targetRuntimeDirBase = "/run/x/../underwrap"; };
      message = targetRuntimeAbsoluteMessage;
    })
    (mkRow {
      name = "target-runtime-whitespace";
      family = "target-runtime-absolute";
      underwrap = baselineUnderwrap // { targetRuntimeDirBase = "/run/under wrap"; };
      message = targetRuntimeAbsoluteMessage;
    })
    (mkRow {
      name = "target-runtime-control";
      family = "target-runtime-absolute";
      underwrap = baselineUnderwrap // { targetRuntimeDirBase = "/run/under\twrap"; };
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
      name = "instance-empty";
      family = "instance-name";
      underwrap = withInstanceName "";
      message = instanceNameMessage;
    })
    (mkRow {
      name = "instance-uppercase-first";
      family = "instance-name";
      underwrap = withInstanceName "Probe";
      message = instanceNameMessage;
    })
    (mkRow {
      name = "instance-too-long";
      family = "instance-name";
      underwrap = withInstanceName instanceTooLongName;
      message = instanceNameMessage;
    })
    (mkRow {
      name = "instance-invalid-punctuation";
      family = "instance-name";
      underwrap = withInstanceName "probe.bad";
      message = instanceNameMessage;
    })
    (mkRow {
      name = "instance-whitespace-control";
      family = "instance-name";
      underwrap = withInstanceName "probe\tbad";
      message = instanceNameMessage;
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
    (mkRow {
      name = "backend-ca-relative";
      family = "backend-ca-path";
      underwrap = baselineUnderwrap // {
        instances.probe = baselineInstance // {
          backend = baselineInstance.backend // { caFile = "etc/ca.pem"; };
        };
      };
      message = backendCaPathMessage;
    })
    (mkRow {
      name = "backend-ca-dotdot";
      family = "backend-ca-path";
      underwrap = baselineUnderwrap // {
        instances.probe = baselineInstance // {
          backend = baselineInstance.backend // { caFile = "/etc/x/../ca.pem"; };
        };
      };
      message = backendCaPathMessage;
    })
    (mkRow {
      name = "backend-ca-whitespace";
      family = "backend-ca-path";
      underwrap = baselineUnderwrap // {
        instances.probe = baselineInstance // {
          backend = baselineInstance.backend // { caFile = "/etc/bad ca.pem"; };
        };
      };
      message = backendCaPathMessage;
    })
  ]
  ++ structuralRows
  ++ accessRows
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
      name = "env-set-dotdot-home";
      family = "env-set-path";
      underwrap = withProbeWrapper { envSet.HOME = "/home/x/../caller"; };
      message = envSetPathMessage "probe";
    })
    (mkRow {
      name = "env-set-whitespace-xdg-config";
      family = "env-set-path";
      underwrap = withProbeWrapper { envSet.XDG_CONFIG_HOME = "/home/caller/bad config"; };
      message = envSetPathMessage "probe";
    })
    (mkRow {
      name = "wrapper-empty";
      family = "wrapper-name";
      underwrap = withNamedWrappers { "" = baselineWrapper; };
      message = wrapperNameMessage;
    })
    (mkRow {
      name = "wrapper-too-long";
      family = "wrapper-name";
      underwrap = withNamedWrappers { "${wrapperTooLongName}" = baselineWrapper; };
      message = wrapperNameMessage;
    })
    (mkRow {
      name = "wrapper-invalid-first";
      family = "wrapper-name";
      underwrap = withNamedWrappers { _probe = baselineWrapper; };
      message = wrapperNameMessage;
    })
    (mkRow {
      name = "wrapper-invalid-punctuation";
      family = "wrapper-name";
      underwrap = withNamedWrappers { "probe/name" = baselineWrapper; };
      message = wrapperNameMessage;
    })
    (mkRow {
      name = "wrapper-whitespace-control";
      family = "wrapper-name";
      underwrap = withNamedWrappers { "probe\tname" = baselineWrapper; };
      message = wrapperNameMessage;
    })
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
    instanceNameMessage
    backendUrlMessage
    backendCaPathMessage
    (wrapperStructureMessage "probe")
    (accessPathsMessage "probe")
    (passthroughLinkerMessage "probe")
    (passthroughForcedMessage "probe")
    (envSetLinkerMessage "probe")
    (envSetRuntimeMessage "probe")
    (envSetPathMessage "probe")
    wrapperNameMessage
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
    assert ensure "baseline expected assertion inventory does not contain exactly 18 messages"
      (lib.length baselineExpectedMessages == 18);
    assert ensure "baseline production assertion inventory differs"
      (baselineActualMessages == baselineExpectedMessages);
    assert ensure "baseline underwrap assertion is false"
      (lib.all (entry: entry.assertion) baselineUnderwrapAssertions);
    assert ensure "baseline global assertion is false"
      (lib.all (entry: entry.assertion) baseline.config.assertions);
    assert ensure "baseline system.build.toplevel evaluation failed"
      baselineToplevel.success;
    true;

  checkValidRow = row:
    let
      evaluated = evaluate (completeConfig row.underwrap { });
      toplevel = builtins.tryEval evaluated.config.system.build.toplevel;
    in
    assert checkedBaseline;
    assert ensure "valid row ${row.name}: underwrap assertion is false"
      (lib.all (entry: entry.assertion) (underwrapAssertionsOf evaluated));
    assert ensure "valid row ${row.name}: global assertion is false"
      (lib.all (entry: entry.assertion) evaluated.config.assertions);
    assert ensure "valid row ${row.name}: system.build.toplevel evaluation failed"
      toplevel.success;
    {
      inherit (row) name;
      toplevelSuccess = toplevel.success;
    };

  checkedValidRows = map checkValidRow validRows;
  validRecord = lib.concatMapStringsSep "\n" (row:
    "${row.name}\t${lib.boolToString row.toplevelSuccess}"
  ) checkedValidRows;

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
    assert ensure "positive boundary row count differs" (lib.length checkedValidRows == 7);
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
  inherit matrixRecord validRecord;
} ''
  mkdir -p "$out"
  printf 'row\tfamily\tmessage\ttoplevel\n' > "$out/matrix.tsv"
  printf '%s\n' "$matrixRecord" >> "$out/matrix.tsv"
  printf 'row\ttoplevel\n' > "$out/valid.tsv"
  printf '%s\n' "$validRecord" >> "$out/valid.tsv"
''
