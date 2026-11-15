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
      perSystem = flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };

          rustToolchain = pkgs.rust-bin.stable."1.97.1".default.override {
            extensions = [ "rust-src" "rust-analyzer" "llvm-tools-preview" ];
          };

          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

          # Common source filtering — Rust/Cargo files, per-crate src/assets/,
          # the committed API schema, and shipped examples exercised by tests.
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (craneLib.filterCargoSources path type)
              || (pkgs.lib.hasInfix "/src/assets/" path)
              # Integration tests compile these crate-local PKI fixtures with
              # `include_str!`; Cargo source filtering otherwise drops them.
              || (pkgs.lib.hasInfix "/crates/integration-tests/testdata/" path)
              # The committed OpenAPI spec at
              # `crates/underwrap-backend/openapi.json` is read off disk
              # by the `openapi_export` drift test and rewritten +
              # panicked on any mismatch; without it in the sandbox
              # source the test sees "expected … actual <empty>".
              || (pkgs.lib.hasSuffix "/openapi.json" path)
              || (pkgs.lib.hasSuffix "/examples" path)
              || (pkgs.lib.hasInfix "/examples/" path);
            name = "source";
          };

          # RustSec advisory database for cargo-audit.
          advisoryDb = pkgs.fetchzip {
            url = "https://github.com/RustSec/advisory-db/archive/02e2517f457a359fc6d3440ea4560c8ea188fee8.tar.gz";
            hash = "sha256-YTTu5uxIuGNogLaOUMatuHRyYXOocyJ0YSi6kcIONIo=";
          };

          # Common build inputs.
          commonArgs = {
            inherit src;
            pname = "underwrap";
            strictDeps = true;
            buildInputs = [ ];
            nativeBuildInputs = [ ];
            # Bound integration-test parallelism so concurrent daemon/backend
            # fixtures remain within their readiness budget on contended build
            # hosts.
            cargoTestExtraArgs = "-- --test-threads=4";
          };

          # Build workspace dependencies first (for caching).
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          # Fetch the lockfile's npm dependency closure once and reuse it for
          # both the production SPA artifact and the complete Vitest suite.
          webNpmDeps = pkgs.fetchNpmDeps {
            src = ./web;
            # If this hash drifts the fetch fails with the offending hash
            # printed; copy it back into this file.
            hash = "sha256-G3U/JwsFgeCOyrQ1xE2lCT4CKg5gUQxfGGHlPe2nBMI=";
          };

          # The build/test npm hooks generate API bindings before their main step.
          # The committed schema lives outside the `./web` source root, so stage
          # it at the relative path expected by `openapi-typescript`.
          webPreBuild = ''
            mkdir -p ../crates/underwrap-backend
            cp ${./crates/underwrap-backend/openapi.json} \
              ../crates/underwrap-backend/openapi.json
          '';

          # Build the admin SPA via npm. The resulting `dist/` directory is what
          # `rust_embed` reads via the `$UNDERWRAP_WEB_DIST` interpolation in
          # `crates/underwrap-backend/src/web.rs`. The Rust build itself doesn't
          # depend on `nodejs`; only this derivation does, so the resulting
          # binary closure stays node-free.
          underwrapWebDist = pkgs.buildNpmPackage {
            pname = "underwrap-web-dist";
            version = "0.1.0";
            src = ./web;
            npmDeps = webNpmDeps;
            preBuild = webPreBuild;
            npmBuildScript = "build";
            installPhase = ''
              runHook preInstall
              mkdir -p $out
              cp -r dist/. $out/
              runHook postInstall
            '';
          };

          # Run the complete Vitest suite and its selected API/library coverage
          # gate using the same pinned dependency closure as the production build.
          underwrapWebTest = pkgs.buildNpmPackage {
            pname = "underwrap-web-test";
            version = "0.1.0";
            src = ./web;
            npmDeps = webNpmDeps;
            preBuild = webPreBuild;
            npmBuildScript = "test:coverage";
            installPhase = ''
              runHook preInstall
              touch $out
              runHook postInstall
            '';
          };

          # Common args extended with the env-var pointer to the
          # built SPA — every Rust derivation in the workspace gets
          # the same `UNDERWRAP_WEB_DIST` value, which only
          # `underwrap-backend`'s build.rs / rust-embed actually
          # consults.
          commonArgsWithWeb = commonArgs // {
            UNDERWRAP_WEB_DIST = "${underwrapWebDist}";
          };

          # Individual crate packages (share the same dep artifacts).
          mkCrate = name: craneLib.buildPackage (commonArgsWithWeb // {
            inherit cargoArtifacts;
            pname = name;
            cargoExtraArgs = "--package ${name}";
          });

          underwrapdPackage = mkCrate "underwrapd";
          underwrapBackendPackage = mkCrate "underwrap-backend";

          # The integration-test crate launches these real executables. `cargo
          # test` otherwise exposes only test-harness binaries in its target
          # directory, so point the harness at the package-specific binaries.
          workspaceTestBinaries = {
            "CARGO_BIN_EXE_underwrapd" = "${underwrapdPackage}/bin/underwrapd";
            "CARGO_BIN_EXE_underwrap-backend" = "${underwrapBackendPackage}/bin/underwrap-backend";
          };

          workspaceTestArgs = commonArgsWithWeb // workspaceTestBinaries // {
            inherit cargoArtifacts;
          };

          # Build the full workspace — all crates.
          workspace = craneLib.buildPackage workspaceTestArgs;

        in {
          packages = {
            default = workspace;
            underwrapd = underwrapdPackage;
            underwrap-exec = mkCrate "underwrap-exec";
            underwrap-ns = mkCrate "underwrap-ns";
            underwrap-debug-tool = mkCrate "underwrap-debug-tool";
            underwrap-backend = underwrapBackendPackage;
            underwrap-secretsd = mkCrate "underwrap-secretsd";
            underwrap-info = mkCrate "underwrap-info";
            # The package root is itself a portable skill directory. Consumers
            # may instead use lib.agentSkills.underwrap without a system build.
            underwrap-agent-skill = pkgs.runCommand "underwrap-agent-skill" { } ''
              mkdir -p "$out"
              cp -R ${self.lib.agentSkills.underwrap}/. "$out/"
              cp ${./LICENSE-MIT} "$out/LICENSE-MIT"
              cp ${./LICENSE-APACHE} "$out/LICENSE-APACHE"
            '';
            # Expose the SPA build separately so the VM test (and any
            # external consumer) can grab the static bundle without
            # rebuilding the workspace.
            underwrap-web-dist = underwrapWebDist;
          };

          checks = {
            # Workspace-wide cargo test. The packaged daemon and backend
            # binaries are supplied through the exact environment paths above.
            workspace-test = craneLib.cargoTest workspaceTestArgs;

            # Faster, better test runner for local/CI use.
            workspace-nextest = craneLib.cargoNextest (workspaceTestArgs // {
              partitions = 1;
              partitionType = "count";
            });

            # Clippy.
            workspace-clippy = craneLib.cargoClippy (commonArgsWithWeb // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });

            # Rustdoc.
            workspace-doc = craneLib.cargoDoc (commonArgsWithWeb // {
              inherit cargoArtifacts;
              env.RUSTDOCFLAGS = "--deny warnings";
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
            };

            # Check dependency bans and sources.
            # The separate audit check covers the pinned advisory database.
            workspace-deny = craneLib.cargoDeny {
              inherit src;
              pname = "underwrap";
              version = "0.1.0";
              cargoDenyChecks = "bans sources";
            };

            # Coverage summary / report generation.
            workspace-llvm-cov = craneLib.cargoLlvmCov workspaceTestArgs;

            # The production SPA artifact consumed by every Rust package and the
            # complete deterministic Vitest suite built from the same npm cache.
            web-build = underwrapWebDist;
            web-test = underwrapWebTest;

            # Evaluate both shipped examples as complete NixOS systems through
            # the real flake module and selected system package. Referencing the
            # complete toplevels from the check also forces both builds.
            examples-nixos-eval =
              let
                evaluate = example: user: nixpkgs.lib.nixosSystem {
                  inherit system;
                  specialArgs = { underwrap = self; };
                  modules = [
                    example
                    ({ ... }: {
                      boot.isContainer = true;
                      system.stateVersion = "25.11";
                      users.users.${user}.isNormalUser = true;
                    })
                  ];
                };
                nixosToplevel =
                  (evaluate ./examples/nixos.nix "alice").config.system.build.toplevel;
                agentToplevel =
                  (evaluate ./examples/agent-github-comment.nix "automation").config.system.build.toplevel;
              in
              pkgs.runCommand "underwrap-examples-nixos-eval" {
                inherit nixosToplevel agentToplevel;
              } ''
                mkdir -p "$out"
                ln -s "$nixosToplevel" "$out/nixos"
                ln -s "$agentToplevel" "$out/agent-github-comment"
              '';

            # Prove the frontend module rejects plaintext runtime endpoints and
            # preserves an admitted HTTPS proxy/base-path URL exactly in the
            # generated daemon configuration.
            frontend-backend-url-eval =
              let
                evaluate = url: nixpkgs.lib.nixosSystem {
                  inherit system;
                  modules = [
                    self.nixosModules.default
                    ({ ... }: {
                      boot.isContainer = true;
                      system.stateVersion = "25.11";
                      services.underwrap = {
                        enable = true;
                        package = pkgs.hello;
                        hardenTargetHost = false;
                        instances.probe.backend.url = url;
                      };
                    })
                  ];
                };
                invalid = evaluate "http://backend.example.invalid";
                validUrl = "https://proxy.example.invalid/runtime/base/";
                valid = evaluate validUrl;
                expectedMessage = "services.underwrap.instances.probe.backend.url: must use lowercase https://; plaintext runtime backend transport is not supported";
                invalidAssertion = pkgs.lib.findFirst
                  (entry: entry.message == expectedMessage)
                  (throw "missing instance-specific backend URL assertion")
                  invalid.config.assertions;
                validAssertion = pkgs.lib.findFirst
                  (entry: entry.message == expectedMessage)
                  (throw "missing instance-specific backend URL assertion")
                  valid.config.assertions;
                # Force derivation construction; the toplevel attribute set is lazy.
                invalidToplevel = builtins.tryEval invalid.config.system.build.toplevel.drvPath;
                validToplevel = builtins.tryEval valid.config.system.build.toplevel.drvPath;
                daemonConfig = valid.config.environment.etc."underwrap/daemon/probe.yaml".source;
              in
              assert !invalidAssertion.assertion;
              assert !invalidToplevel.success;
              assert validAssertion.assertion;
              assert validToplevel.success;
              pkgs.runCommand "underwrap-frontend-backend-url-eval" {
                inherit daemonConfig validUrl expectedMessage;
              } ''
                grep -F -- "\"url\":\"$validUrl\"" "$daemonConfig"
                mkdir -p "$out"
                cp "$daemonConfig" "$out/probe-daemon.json"
                printf '%s\n' "$expectedMessage" > "$out/plaintext-rejection.txt"
              '';

            # Public discovery metadata, optional descriptions, unchanged
            # privileged configuration, and portable skill packaging.
            wrapper-discovery-eval = import ./nix/tests/wrapper-discovery-eval.nix {
              inherit nixpkgs system;
              underwrapModule = self.nixosModules.default;
              infoPackage = self.packages.${system}.underwrap-info;
              skillSource = self.lib.agentSkills.underwrap;
              skillPackage = self.packages.${system}.underwrap-agent-skill;
            };

            # Prove every production frontend security assertion and each
            # independently meaningful combined-predicate class fails closed.
            frontend-security-assertions-eval = import ./nix/tests/frontend-security-assertions-eval.nix {
              inherit nixpkgs system;
              underwrapModule = self.nixosModules.default;
            };

            # Round-trip admitted literal runtime paths through the actual
            # module, daemon configuration and inert tmpfiles parser.
            runtime-tmpfiles-eval = import ./nix/tests/runtime-tmpfiles-eval.nix {
              inherit nixpkgs system;
              underwrapModule = self.nixosModules.default;
            };

            # Evaluate the production frontend module across the complete
            # ordinary-swap and zram-generator hardened-posture matrix.
            frontend-swap-posture-eval = import ./nix/tests/swap-posture-eval.nix {
              inherit nixpkgs system;
              underwrapModule = self.nixosModules.default;
            };
          };

          devShells.default = craneLib.devShell {
            # Extra packages available in the dev shell.
            packages = with pkgs; [
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
        # Harness-independent source directory, suitable for Home Manager's
        # home.file.<harness skill directory>.source without compiling Rust.
        lib.agentSkills.underwrap = ./.agents/skills/underwrap;
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

          peerProbeScript = pkgs.writeScript "peer-probe.py"
            (builtins.readFile ./nix/tests/peer-probe.py);

          directNsAttackScript = pkgs.writeScript "direct-ns-attack.py"
            (builtins.readFile ./nix/tests/direct-ns-attack.py);

          # The main test script is large (>128 KB). Embedding it inline in the
          # `testScript` attribute pushes that attribute past the kernel's
          # single-argument/env-string limit (MAX_ARG_STRLEN = 128 KB), so the
          # test-driver build fails to exec bash with "Argument list too long".
          # Stage it as its own store file (builtins.toFile is a pure eval-time
          # write — no builder, no argv) and `exec` it at runtime so the
          # `testScript` attribute itself stays tiny.
          mainTestScript = builtins.toFile "underwrap-vm-test.py"
            (builtins.readFile ./nix/tests/test.py);

          runtimeCases = import ./nix/tests/runtime-tmpfiles-cases.nix;
          mkVmTest = name: targetRuntimeDirBase: pkgs.testers.nixosTest {
              inherit name;

              nodes.machine = { pkgs, ... }: import ./nix/tests/node.nix {
                inherit pkgs targetRuntimeDirBase;
                underwrapPackage = perSystem.packages.${system}.default;
                nixosModule = self.nixosModules.default;
              };

              testScript = ''
                import os
                os.environ["UNDERWRAP_TEST_RUNTIME_BASE"] = ${builtins.toJSON targetRuntimeDirBase}
                os.environ["SPOOF_SCRIPT"] = "${spoofScript}"
                os.environ["ANCESTOR_ATTACK_SCRIPT"] = "${ancestorAttackScript}"
                os.environ["PEER_PROBE_SCRIPT"] = "${peerProbeScript}"
                os.environ["DIRECT_NS_ATTACK_SCRIPT"] = "${directNsAttackScript}"
                os.environ["PYTHON3"] = "${pkgs.python3}/bin/python3"
                exec(compile(open("${mainTestScript}").read(), "${mainTestScript}", "exec"), globals())
              '';
            };
        in {
          checks.${system} = (perSystem.checks.${system} or {}) // {
            vm-test = mkVmTest "underwrap-vm-test" runtimeCases.custom;
            runtime-tmpfiles-vm-test = mkVmTest "underwrap-runtime-tmpfiles-vm-test" runtimeCases.combined;
          };
        };

    in
      # Merge all outputs while adding the x86-only VM check without replacing
      # the complete per-system check set on aarch64-linux.
      perSystem
      // systemIndependent
      // {
        checks = perSystem.checks // vmTest.checks;
      };
}
