{
  description = "underwrap — wrapper-oriented secret brokerage for local command execution";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, crane, flake-utils, rust-overlay, ... }:
    let
      # Per-system outputs (packages, devShells, checks).
      perSystem = flake-utils.lib.eachDefaultSystem (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };

          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" "llvm-tools-preview" ];
          };

          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

          # Common source filtering — Rust/Cargo files plus per-crate src/assets/
          # so any bundled static assets survive the sandboxed build.
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (craneLib.filterCargoSources path type)
              || (pkgs.lib.hasInfix "/src/assets/" path)
              || (pkgs.lib.hasInfix "/crates/frontend-keyring-probe" path)
              # The committed OpenAPI spec at
              # `crates/backend-backendd/openapi.json` is read off disk
              # by the `openapi_export` drift test and rewritten +
              # panicked on any mismatch; without it in the sandbox
              # source the test sees "expected … actual <empty>".
              || (pkgs.lib.hasSuffix "/openapi.json" path);
            name = "source";
          };

          # RustSec advisory database for cargo-audit.
          advisoryDb = pkgs.fetchzip {
            url = "https://github.com/RustSec/advisory-db/archive/refs/heads/main.tar.gz";
            hash = "sha256-ehYI2+TnkdX86YRk+38K8ulZTDyifBbUTFub7IQ9vSk=";
          };

          # Common build inputs.
          commonArgs = {
            inherit src;
            pname = "underwrap";
            strictDeps = true;
            buildInputs = [ ];
            nativeBuildInputs = [ ];
            # Belt-and-suspenders cap on integration-test
            # parallelism. The real fix for the readiness flake was
            # making `ensure_package_built` a no-op when the binary
            # is already on disk (which it always is in a crane
            # build), so the daemon test no longer fires a recursive
            # `cargo build` that CPU-starves sibling TestDaemon
            # startups. The cap stays because it's a sensible
            # default for a contended sandbox and protects against
            # future regressions of the same shape.
            cargoTestExtraArgs = "-- --test-threads=4";
          };

          # Build workspace dependencies first (for caching).
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          # Build the admin SPA via npm. The resulting `dist/`
          # directory is what `rust_embed` reads via the
          # `$UNDERWRAP_WEB_DIST` interpolation in
          # `crates/backend-backendd/src/web.rs`. The Rust build
          # itself doesn't depend on `nodejs`; only this derivation
          # does, so the resulting binary closure stays node-free.
          underwrapWebDist = pkgs.buildNpmPackage {
            pname = "underwrap-web-dist";
            version = "0.1.0";
            src = ./web;
            # If this hash drifts the build fails with the
            # offending hash printed; copy it back into this file.
            npmDepsHash = "sha256-953WYvZOCLf2OEvkZftxMMHz73aTuBrq+qtMI8B2HtU=";
            # The npm `prebuild` hook runs `openapi-typescript` against
            # `../crates/backend-backendd/openapi.json`, which lives
            # outside the `./web` source root. Stage it at the
            # expected relative path before the build phase so the
            # generator can find it.
            preBuild = ''
              mkdir -p ../crates/backend-backendd
              cp ${./crates/backend-backendd/openapi.json} \
                ../crates/backend-backendd/openapi.json
            '';
            npmBuildScript = "build";
            installPhase = ''
              runHook preInstall
              mkdir -p $out
              cp -r dist/. $out/
              runHook postInstall
            '';
          };

          # Common args extended with the env-var pointer to the
          # built SPA — every Rust derivation in the workspace gets
          # the same `UNDERWRAP_WEB_DIST` value, which only
          # `backend-backendd`'s build.rs / rust-embed actually
          # consults.
          commonArgsWithWeb = commonArgs // {
            UNDERWRAP_WEB_DIST = "${underwrapWebDist}";
          };

          # Build the full workspace — all crates.
          workspace = craneLib.buildPackage (commonArgsWithWeb // {
            inherit cargoArtifacts;
          });

          # Individual crate packages (share the same dep artifacts).
          mkCrate = name: craneLib.buildPackage (commonArgsWithWeb // {
            inherit cargoArtifacts;
            pname = name;
            cargoExtraArgs = "--package ${name}";
          });

        in {
          packages = {
            default = workspace;
            underwrapd = mkCrate "frontend-underwrapd";
            underwrap-exec = mkCrate "frontend-exec";
            underwrap-ns = mkCrate "frontend-ns";
            underwrap-debug-tool = mkCrate "frontend-debug-tool";
            underwrap-keyring-probe = mkCrate "frontend-keyring-probe";
            underwrap-go-keyring-probe = pkgs.buildGoModule {
              pname = "underwrap-go-keyring-probe";
              version = "0.1.0";
              src = ./nix/tests/go-keyring-probe;
              vendorHash = "sha256-4MyeW7s/AEDkNMcFWie5mWS9F47OtFSX7QBpcTktrrc=";
            };
            underwrap-backendd = mkCrate "backend-backendd";
            underwrap-secretsd = mkCrate "frontend-secretsd";
            # Expose the SPA build separately so the VM test (and any
            # external consumer) can grab the static bundle without
            # rebuilding the workspace.
            underwrap-web-dist = underwrapWebDist;
          };

          checks = {
            # Workspace-wide cargo test.
            # Binary-dependent integration tests skip gracefully in sandbox.
            workspace-test = craneLib.cargoTest (commonArgs // {
              inherit cargoArtifacts;
            });

            # Faster, better test runner for local/CI use.
            workspace-nextest = craneLib.cargoNextest (commonArgs // {
              inherit cargoArtifacts;
              partitions = 1;
              partitionType = "count";
            });

            # Clippy.
            workspace-clippy = craneLib.cargoClippy (commonArgs // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });

            # Formatting.
            workspace-fmt = craneLib.cargoFmt {
              inherit src;
              pname = "underwrap";
              version = "0.1.0";
            };

            # RustSec advisory database checks.
            workspace-audit = craneLib.cargoAudit {
              inherit src;
              advisory-db = advisoryDb;
              # RUSTSEC-2023-0071 affects RSA decryption timing. underwrap uses
              # ssh-key for unlock-key signature verification with public keys,
              # not RSA private-key decryption. Keep the audit gate enabled but
              # suppress this specific advisory until the dependency graph moves.
              cargoAuditExtraArgs = "--ignore yanked --ignore RUSTSEC-2023-0071";
            };

            # Supply-chain policy checks.
            # Keep this lightweight and policy-free for now; audit handles
            # advisories separately and license policy can be added later via deny.toml.
            workspace-deny = craneLib.cargoDeny {
              inherit src;
              pname = "underwrap";
              version = "0.1.0";
              cargoDenyChecks = "bans sources";
            };

            # Coverage summary / report generation.
            workspace-llvm-cov = craneLib.cargoLlvmCov (commonArgs // {
              inherit cargoArtifacts;
            });
          };

          devShells.default = craneLib.devShell {
            # Extra packages available in the dev shell.
            packages = with pkgs; [
              rust-analyzer
              cargo-watch
              cargo-edit
              cargo-expand
              cargo-nextest
              cargo-llvm-cov
              cargo-audit
              cargo-deny
              cargo-udeps
              cargo-mutants
              nodejs
              # Playwright e2e tests in `web/` use the chromium binary
              # nixpkgs builds (patched for the Nix dynamic-loader). The
              # env vars below tell the @playwright/test npm package to
              # find that binary instead of downloading its own copy
              # (which won't run unpatched on NixOS anyway).
              playwright-driver.browsers
            ];
            PLAYWRIGHT_BROWSERS_PATH = "${pkgs.playwright-driver.browsers}";
            PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD = "1";
            # Playwright also probes the host browser executable when
            # running tests outside Nix's sandbox; pointing it at the
            # patched chromium keeps node_modules' `npx playwright …`
            # behaviour consistent with the nixpkgs binary.
            PLAYWRIGHT_NODEJS_PATH = "${pkgs.nodejs_22}/bin/node";
          };
        }
      );

      # System-independent outputs.
      systemIndependent = {
        # NixOS modules.
        nixosModules.default = ./nix/modules/underwrap.nix;
      };

      # NixOS VM test (x86_64-linux only since it needs KVM).
      vmTest =
        let
          system = "x86_64-linux";
          pkgs = import nixpkgs { inherit system; };

          spoofScript = pkgs.writeScript "spoof.py"
            (builtins.readFile ./nix/tests/spoof.py);

          ancestorAttackScript = pkgs.writeScript "ancestor-attack.py"
            (builtins.readFile ./nix/tests/ancestor-attack.py);

          testScript = builtins.readFile ./nix/tests/test.py;

        in {
          checks.${system} = (perSystem.checks.${system} or {}) // {
            vm-test = pkgs.testers.nixosTest {
              name = "underwrap-vm-test";

              nodes.machine = { pkgs, ... }: import ./nix/tests/node.nix {
                inherit pkgs;
                underwrapPackage = perSystem.packages.${system}.default;
                goKeyringProbePackage = perSystem.packages.${system}.underwrap-go-keyring-probe;
                nixosModule = self.nixosModules.default;
              };

              testScript = ''
                import os
                os.environ["SPOOF_SCRIPT"] = "${spoofScript}"
                os.environ["ANCESTOR_ATTACK_SCRIPT"] = "${ancestorAttackScript}"
                os.environ["PYTHON3"] = "${pkgs.python3}/bin/python3"
                ${testScript}
              '';
            };
          };
        };

    in
      # Merge all outputs.
      perSystem
      // systemIndependent
      // vmTest;
}
