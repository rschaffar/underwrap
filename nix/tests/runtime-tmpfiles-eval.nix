{ nixpkgs, system, underwrapModule }:
let
  lib = nixpkgs.lib;
  pkgs = import nixpkgs { inherit system; };
  cases = import ./runtime-tmpfiles-cases.nix;
  evaluate = base: nixpkgs.lib.nixosSystem {
    inherit system;
    modules = [
      underwrapModule
      {
        boot.isContainer = true;
        system.stateVersion = "25.11";
        services.underwrap = {
          enable = true;
          package = pkgs.hello;
          hardenTargetHost = false;
          targetRuntimeDirBase = base;
          instances.probe = {
            backend.url = "https://backend.example.invalid";
            wrappers.files = {
              target = "${pkgs.coreutils}/bin/true";
              resource = "probe/files";
              files.paths = [ ".config/probe" ];
            };
          };
        };
      }
    ];
  };
  checkedCases = lib.mapAttrs (name: base:
    let
      evaluated = evaluate base;
      config = evaluated.config;
      toplevel = builtins.tryEval config.system.build.toplevel.drvPath;
    in
    assert lib.assertMsg (lib.all (entry: entry.assertion) config.assertions)
      "runtime tmpfiles ${name}: admitted literal failed an assertion";
    assert lib.assertMsg toplevel.success
      "runtime tmpfiles ${name}: admitted literal failed toplevel evaluation";
    {
      inherit base;
      rules = config.systemd.tmpfiles.rules;
      daemonConfig = config.environment.etc."underwrap/daemon/probe.yaml".source;
      targetTable = config.environment.etc."underwrap/target-uids".text;
    }
  ) cases;
  manifest = pkgs.writeText "runtime-tmpfiles-cases.json" (builtins.toJSON checkedCases);
in
pkgs.runCommand "underwrap-runtime-tmpfiles-eval" {
  nativeBuildInputs = [ pkgs.python3 pkgs.systemd ];
} ''
  mkdir -p "$out"
  python3 ${./runtime-tmpfiles-parser.py} ${manifest} "$out"
''
