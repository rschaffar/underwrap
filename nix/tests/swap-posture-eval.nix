{
  nixpkgs,
  system,
  underwrapModule,
}:

let
  lib = nixpkgs.lib;
  pkgs = import nixpkgs { inherit system; };

  ordinarySwapMessage = "services.underwrap.hardenTargetHost is enabled but at least one swapDevice is not encrypted. Tokens paged out of a wrapped target's memory would surface as plaintext on disk. Enable randomEncryption or LUKS on every swapDevice, remove swap, or set services.underwrap.hardenTargetHost = false (and accept the weaker claim).";
  zramWritebackMessage = "services.underwrap.hardenTargetHost is enabled but the typed zram-generator settings either configure zram swap writeback or cannot be interpreted unambiguously. Use pure RAM-only zram or no swap, configure writeback only for a zram filesystem with an exact mount-point or canonical non-swap fs-type in canonical single-line typed settings, or set services.underwrap.hardenTargetHost = false (and accept the weaker claim).";
  swapMessages = [
    ordinarySwapMessage
    zramWritebackMessage
  ];

  ordinarySwap = {
    device = "/dev/underwrap-test-ordinary-swap";
  };
  randomEncryptedSwap = ordinarySwap // {
    randomEncryption.enable = true;
  };
  writebackDevice = "/dev/underwrap-test-zram-writeback";

  highLevelSettings = withWriteback: {
    zram0 = {
      zram-size = "50 / 100 * ram";
      compression-algorithm = "zstd";
      swap-priority = 5;
    }
    // lib.optionalAttrs withWriteback {
      writeback-device = writebackDevice;
    };
  };

  directSettings = name: section: {
    ${name} = section;
  };
  swapWritebackSection = {
    writeback-device = writebackDevice;
  };

  rows = [
    {
      name = "no-swap";
      harden = true;
      module = { };
      expectedFalse = [ ];
      generatorEnabled = false;
      expectedSettings = { };
    }
    {
      name = "ordinary-random-encrypted";
      harden = true;
      module.swapDevices = [ randomEncryptedSwap ];
      expectedFalse = [ ];
      generatorEnabled = false;
      expectedSettings = { };
    }
    {
      name = "ordinary-plain";
      harden = true;
      module.swapDevices = [ ordinarySwap ];
      expectedFalse = [ ordinarySwapMessage ];
      generatorEnabled = false;
      expectedSettings = { };
    }
    {
      name = "high-level-pure-zram";
      harden = true;
      module.zramSwap = {
        enable = true;
        swapDevices = 1;
      };
      expectedFalse = [ ];
      generatorEnabled = true;
      expectedSettings = highLevelSettings false;
    }
    {
      name = "high-level-active-writeback";
      harden = true;
      module.zramSwap = {
        enable = true;
        swapDevices = 1;
        writebackDevice = writebackDevice;
      };
      expectedFalse = [ zramWritebackMessage ];
      generatorEnabled = true;
      expectedSettings = highLevelSettings true;
    }
    {
      name = "high-level-disabled-inert-writeback";
      harden = true;
      module.zramSwap = {
        enable = false;
        swapDevices = 1;
        writebackDevice = writebackDevice;
      };
      expectedFalse = [ ];
      generatorEnabled = false;
      expectedSettings = { };
    }
    {
      name = "high-level-zero-inert-writeback";
      harden = true;
      module.zramSwap = {
        enable = true;
        swapDevices = 0;
        writebackDevice = writebackDevice;
      };
      expectedFalse = [ ];
      generatorEnabled = true;
      expectedSettings = { };
    }
    {
      name = "high-level-negative-inert-writeback";
      harden = true;
      module.zramSwap = {
        enable = true;
        swapDevices = -1;
        writebackDevice = writebackDevice;
      };
      expectedFalse = [ ];
      generatorEnabled = true;
      expectedSettings = { };
    }
    {
      name = "direct-default-swap-writeback";
      harden = true;
      module.services.zram-generator = {
        enable = true;
        settings = directSettings "zram7" swapWritebackSection;
      };
      expectedFalse = [ zramWritebackMessage ];
      generatorEnabled = true;
      expectedSettings = directSettings "zram7" swapWritebackSection;
    }
    {
      name = "direct-explicit-swap-writeback";
      harden = true;
      module.services.zram-generator = {
        enable = true;
        settings = directSettings "zram8" (
          swapWritebackSection
          // {
            fs-type = "swap";
          }
        );
      };
      expectedFalse = [ zramWritebackMessage ];
      generatorEnabled = true;
      expectedSettings = directSettings "zram8" (
        swapWritebackSection
        // {
          fs-type = "swap";
        }
      );
    }
    {
      name = "direct-parser-plus-zero";
      harden = true;
      module.services.zram-generator = {
        enable = true;
        settings = directSettings "zram+0" swapWritebackSection;
      };
      expectedFalse = [ zramWritebackMessage ];
      generatorEnabled = true;
      expectedSettings = directSettings "zram+0" swapWritebackSection;
    }
    {
      name = "direct-parser-leading-zero";
      harden = true;
      module.services.zram-generator = {
        enable = true;
        settings = directSettings "zram00" swapWritebackSection;
      };
      expectedFalse = [ zramWritebackMessage ];
      generatorEnabled = true;
      expectedSettings = directSettings "zram00" swapWritebackSection;
    }
    {
      name = "direct-malformed-zram-prefix";
      harden = true;
      module.services.zram-generator = {
        enable = true;
        settings = directSettings "zram-malformed" swapWritebackSection;
      };
      expectedFalse = [ zramWritebackMessage ];
      generatorEnabled = true;
      expectedSettings = directSettings "zram-malformed" swapWritebackSection;
    }
    {
      name = "direct-section-name-normalization";
      harden = true;
      module.services.zram-generator = {
        enable = true;
        settings = directSettings " zram12 " swapWritebackSection;
      };
      expectedFalse = [ zramWritebackMessage ];
      generatorEnabled = true;
      expectedSettings = directSettings " zram12 " swapWritebackSection;
    }
    {
      name = "direct-writeback-key-normalization";
      harden = true;
      module.services.zram-generator = {
        enable = true;
        settings = directSettings "zram13" {
          " writeback-device " = writebackDevice;
        };
      };
      expectedFalse = [ zramWritebackMessage ];
      generatorEnabled = true;
      expectedSettings = directSettings "zram13" {
        " writeback-device " = writebackDevice;
      };
    }
    {
      name = "direct-non-zram-value-injection";
      harden = true;
      module.services.zram-generator = {
        enable = true;
        settings = directSettings "cache-value" {
          benign = "safe\n[zram0]\nwriteback-device=${writebackDevice}";
        };
      };
      expectedFalse = [ zramWritebackMessage ];
      generatorEnabled = true;
      expectedSettings = directSettings "cache-value" {
        benign = "safe\n[zram0]\nwriteback-device=${writebackDevice}";
      };
    }
    {
      name = "direct-non-zram-key-injection";
      harden = true;
      module.services.zram-generator = {
        enable = true;
        settings = directSettings "cache-key" {
          "benign\n[zram0]\nwriteback-device" = writebackDevice;
        };
      };
      expectedFalse = [ zramWritebackMessage ];
      generatorEnabled = true;
      expectedSettings = directSettings "cache-key" {
        "benign\n[zram0]\nwriteback-device" = writebackDevice;
      };
    }
    {
      name = "direct-noncanonical-swap-fs-type";
      harden = true;
      module.services.zram-generator = {
        enable = true;
        settings = directSettings "zram14" (
          swapWritebackSection
          // {
            fs-type = " swap ";
          }
        );
      };
      expectedFalse = [ zramWritebackMessage ];
      generatorEnabled = true;
      expectedSettings = directSettings "zram14" (
        swapWritebackSection
        // {
          fs-type = " swap ";
        }
      );
    }
    {
      name = "direct-mounted-filesystem";
      harden = true;
      module.services.zram-generator = {
        enable = true;
        settings = directSettings "zram9" (
          swapWritebackSection
          // {
            fs-type = "ext4";
            mount-point = "/var/lib/underwrap-test-zram";
          }
        );
      };
      expectedFalse = [ ];
      generatorEnabled = true;
      expectedSettings = directSettings "zram9" (
        swapWritebackSection
        // {
          fs-type = "ext4";
          mount-point = "/var/lib/underwrap-test-zram";
        }
      );
    }
    {
      name = "direct-non-zram-section";
      harden = true;
      module.services.zram-generator = {
        enable = true;
        settings = directSettings "cache0" swapWritebackSection;
      };
      expectedFalse = [ ];
      generatorEnabled = true;
      expectedSettings = directSettings "cache0" swapWritebackSection;
    }
    {
      name = "direct-disabled-generator";
      harden = true;
      module.services.zram-generator = {
        enable = false;
        settings = directSettings "zram10" swapWritebackSection;
      };
      expectedFalse = [ ];
      generatorEnabled = false;
      expectedSettings = directSettings "zram10" swapWritebackSection;
    }
    {
      name = "mixed-ordinary-and-zram-failure";
      harden = true;
      module = {
        swapDevices = [ ordinarySwap ];
        zramSwap = {
          enable = true;
          swapDevices = 1;
          writebackDevice = writebackDevice;
        };
      };
      expectedFalse = swapMessages;
      generatorEnabled = true;
      expectedSettings = highLevelSettings true;
    }
    {
      name = "weaker-posture";
      harden = false;
      module = {
        swapDevices = [ ordinarySwap ];
        services.zram-generator = {
          enable = true;
          settings = directSettings "zram11" swapWritebackSection;
        };
      };
      expectedFalse = [ ];
      generatorEnabled = true;
      expectedSettings = directSettings "zram11" swapWritebackSection;
    }
  ];

  evaluate =
    row:
    nixpkgs.lib.nixosSystem {
      inherit system;
      modules = [
        underwrapModule
        (
          { ... }:
          {
            boot.isContainer = true;
            system.stateVersion = "25.11";
            services.underwrap = {
              enable = true;
              package = pkgs.hello;
              hardenTargetHost = row.harden;
            };
          }
        )
        row.module
      ];
    };

  sorted = lib.sort builtins.lessThan;
  checkRow =
    row:
    let
      evaluated = evaluate row;
      assertionDefinitions = evaluated.options.assertions.definitionsWithLocations;
      underwrapDefinitions = lib.filter (
        definition: lib.hasSuffix "/nix/modules/underwrap.nix" definition.file
      ) assertionDefinitions;
      underwrapAssertions = (builtins.head underwrapDefinitions).value;
      ordinaryAssertions = lib.filter (entry: entry.message == ordinarySwapMessage) underwrapAssertions;
      zramAssertions = lib.filter (entry: entry.message == zramWritebackMessage) underwrapAssertions;
      expectedFalse = sorted row.expectedFalse;
      falseUnderwrapMessages = sorted (
        map (entry: entry.message) (lib.filter (entry: !entry.assertion) underwrapAssertions)
      );
      falseAssertionCount = lib.length (lib.filter (entry: !entry.assertion) evaluated.config.assertions);
      toplevel = builtins.tryEval evaluated.config.system.build.toplevel.drvPath;
      finalGenerator = evaluated.config.services.zram-generator;
      ensure =
        label: condition: if condition then true else throw "swap posture matrix row ${row.name}: ${label}";
      definitionPresence = ensure "underwrap assertion definition count differs" (
        lib.length underwrapDefinitions == 1
      );
      assertionPresence = ensure "swap assertion presence differs" (
        lib.length ordinaryAssertions == (if row.harden then 1 else 0)
        && lib.length zramAssertions == (if row.harden then 1 else 0)
      );
      falseUnderwrapSet = ensure "complete false underwrap assertion message set differs" (
        falseUnderwrapMessages == expectedFalse
      );
      falseAssertionTotal = ensure "complete false assertion count differs" (
        falseAssertionCount == builtins.length row.expectedFalse
      );
      toplevelResult = ensure "forced system.build.toplevel result differs" (
        toplevel.success == (row.expectedFalse == [ ])
      );
      generatorEnable = ensure "final generator enable witness differs" (
        finalGenerator.enable == row.generatorEnabled
      );
      generatorSettings = ensure "final generator settings witness differs" (
        finalGenerator.settings == row.expectedSettings
      );
    in
    assert definitionPresence;
    assert assertionPresence;
    assert falseUnderwrapSet;
    assert falseAssertionTotal;
    assert toplevelResult;
    assert generatorEnable;
    assert generatorSettings;
    {
      inherit (row) name harden;
      generatorEnabled = finalGenerator.enable;
      falseAssertions = map (
        message: if message == ordinarySwapMessage then "ordinary-swap" else "zram-writeback"
      ) expectedFalse;
      toplevelSuccess = toplevel.success;
    };

  checkedRows = map checkRow rows;
  matrixRecord = lib.concatMapStringsSep "\n" (
    row:
    "${row.name}\tharden=${lib.boolToString row.harden}\tgenerator=${lib.boolToString row.generatorEnabled}\tfalse=${lib.concatStringsSep "," row.falseAssertions}\ttoplevel=${lib.boolToString row.toplevelSuccess}"
  ) checkedRows;
in
pkgs.runCommand "underwrap-frontend-swap-posture-eval" { } ''
  mkdir -p "$out"
  cat > "$out/matrix.tsv" <<'EOF'
  ${matrixRecord}
  EOF
''
