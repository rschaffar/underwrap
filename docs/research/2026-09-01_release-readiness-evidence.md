# Release-readiness evidence

Date: 2026-09-01.

Status: Candidate validated and integrated; bounded review corrections independently verified.

## Candidate

At validation time, the local-integration product candidate was commit `5aae8810d1e941e214e4494057d8c750bcc0f0e4` on branch `release/readiness`.

It was the required rebase of the originally validated candidate `f61d8ddeb76de76dad0e4c8bffc45dcee7c0bd9e` onto local `main` commit `7b3145615f5cd775aa2280c4d07315e47d66e066`.

An exact tree comparison found only the eight-line `AGENTS.md` worktree-integration policy inherited from `main`; product source, tests, locks, Nix expressions, and product documentation were unchanged.

The tracked tree was clean before and after both exact-candidate matrices and the disposable walkthrough.

The candidate is review preparation, not a release, tag, publication, formal proof, or completed independent audit.

The integration review range is `7b3145615f5cd775aa2280c4d07315e47d66e066..5aae8810d1e941e214e4494057d8c750bcc0f0e4`.

It contains these rebased commits:

- `95d089f` — register the active release-readiness plan;
- `69d9e7a` — make VM blob observation bounded and exact;
- `489a4e2` — curate the VM equality investigation;
- `cf00c71` — record the SPA coverage decision evidence;
- `a7da304` — add meaningful API endpoint tests and enforce SPA coverage through Nix;
- `8386010` — isolate namespace fd-ownership witnesses;
- `ba0bcc5` — refresh the compatible `lucide-react` lock and Nix npm hash;
- `c32f75b` — serialize integration-backend port reservation through readiness;
- `715a1de` — preserve restrictive-umask coverage artifacts and isolate target-status EOF witnesses;
- `7ccb7d3` — isolate the daemon pidfd close witness;
- `5aae881` — isolate the stage-one relocated-fd close witness.

## Integration and review addendum

Documentation-only commits extended the feature to `402060e73804f5ea4f64755258123bccdd5ea542`.

Local `main` integrated that exact feature tip through merge `8ce345c333a91ce937cf39dd85124c2dd772b8ae`.

The feature branch and linked worktree were removed after the verified merge, and the product and documentation candidates remain reachable through its second parent.

The [bounded independent review](2026-09-01_release-readiness-independent-review.md) found no Critical or Major product defect and identified two Minor corrections.

Commit `efecdb1373fdd04e5f5600f5ebc31ba49670fc13` extends backend-fixture startup coordination across cooperating Cargo and nextest processes.

Correction documentation and verification produced tip `abc94fc0ec0842272d0890702bd1be097e2f8aaf`.

Local `main` integrated that tip through final tree-identical merge `dfe0ca7429e9c5e0ac7568d35eab3675257527f1`, and the correction feature branch and linked worktree were removed.

On 2026-09-06 the user accepted completion and requested archival of the release-readiness plan.

The current documentation reconciles the review, both merges, branch cleanup, residual unrelated-process bind window, and continuing external-audit nonclaim.

## Evidence classes

**Direct** means the named command ran against the exact integration candidate on this host.

**Tree-equivalent direct** means the behavior ran against original candidate `f61d8dd`, and the only later candidate-tree delta was non-product `AGENTS.md` guidance.

**Simulated operator interaction** means the disposable VM used the real authenticated admin API in place of browser clicks while exercising the same backend transition.

**Historical** means evidence predates this exact candidate and is not counted as its fresh pass.

**Unavailable** means the property was not exercised in this environment.

Log hashes below identify local captures under `/tmp`.

Those logs are not committed artifacts and may be removed during hygiene cleanup.

## Environment

| Item | Observed value |
|---|---|
| Host system | `x86_64-linux` |
| Kernel | Linux `6.18.48` |
| Rust | `rustc 1.97.1 (8bab26f4f 2026-07-14)` |
| Cargo | `cargo 1.97.1 (c980f4866 2026-06-30)` |
| Node | `24.19.0` |
| npm | `11.17.0` |
| Playwright package | `1.61.1` |
| Playwright driver Node | `22.23.2` |
| Chromium | Chrome for Testing `149.0.7827.55`, Playwright revision `1228` |
| Browser store | `/nix/store/lwir8qh9b4j4a2bsmwrsx6c4z4d1fh8b-playwright-browsers` |

The post-install exact-candidate environment capture has SHA-256 `18c285bce75fe8599b674a0e53e11a13b8f18e6013a5335d27bad8e7dba7438d`.

## Clean validation matrix

Every command row below was executed against the exact rebased candidate.

Rows that reused an identical Nix derivation say so rather than claiming a fresh execution.

Commands ran sequentially.

No failed product check was silently replaced by a later pass.

| Row | Result | Local log SHA-256 |
|---|---|---|
| Candidate identity and clean status | Passed at exact detached commit `5aae8810`; no tracked changes | `18c285bce75fe8599b674a0e53e11a13b8f18e6013a5335d27bad8e7dba7438d` |
| Candidate Nix prerequisites | Passed with unchanged output paths | `3b7191c56f8c41b723e4d23959be8a5898fd43caec0b494448ba8bdd324598c6` |
| `cargo test --workspace` | Passed | `c43e8ac6100e0ca80ff4cc04bcac57d69af1134b49f1e918ac92de4dd2fca1e5` |
| `cargo fmt -- --check` | Passed with empty output | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `cargo clippy --all-targets --workspace -- -D warnings` | Passed | `8003b65b9b529f927e4af86ffd2db177ee23ca52d350fd3de53b9c60a43d12b0` |
| `./crates/integration-tests/run` | Passed, 107 tests | `eea0e8514d17974d7d5924207c73002813e0ef4e0fc673624be7910eaa91fbc2` |
| Clean `npm ci` | Passed, 247 packages audited, zero vulnerabilities | `dec124538f1e1a07cee0e76280d5ffe7d50e9d2f9df894691fd5396da62c3410` |
| `npm test` | Passed, 30 files and 192 tests | `b61b8882a22e3f7728c3d65ee1405a7f996d22e79613bc519bc077790ecd32d6` |
| `npm run test:coverage` | Passed every retained 80-percent threshold | `c09354997f99dc2e30fb40a094bb6b3e0ded7da2cf0ceaaf4a7d89462a4bad9f` |
| `npm run build` | Passed | `dd5717ea4ab12b05a30b1018aab03556d2d6e711e66840c04a766d3a20f032c7` |
| `npm run test:e2e` | Passed, 38 browser tests | `987e56d829bc5b56ef8611032b01c186d859f6c4c4af16b18a4487769b0ac3e6` |
| `npm ls --all` | Passed; only platform and feature optional dependencies were unmet | `5d0a75056dcfadec5251c46ec1690c71e4398591c6e40f4786f74ace600f80d5` |
| `npm audit --package-lock-only` | Passed, zero vulnerabilities | `a8dadbbee85401562d675d333ffdd2061f80a521a20404a61aae7c2d8feb8d3e` |
| `npm outdated --json` policy review | Expected status 1 with exactly the three documented deferred rows | `011dcbad2192e6d7bb8fdedc4a371980473265379052f015c5bb8b14b4957942` |
| `nix flake check --all-systems --no-build` | Passed evaluation for x86-64 and AArch64 Linux | `668b71a3650b242f24a9452c305940c6aed95ecb41da27ba354ca3c83a2d834b` |
| `nix flake check` | Passed exact-candidate evaluation with every output already valid | `68a9b0c110cff2810b749ca36a5791e8c2739b72618989d4263c51124bfdb971` |
| Candidate VM derivation | Reused the identical successful derivation whose full log reports `ALL TESTS PASSED`, including F-007 and F-009 | `3becfb6a3e8cc3637dcd6530b7395396061f3fbbf93dd1f478e482e60c026f62` |

The original `f61d8dd` full Nix run built all 65 current-system checks and has log SHA-256 `eab5b447c1c15b0e90c626f92c32773e10eb94f08856288325cd96346fdc0276`.

The exact rebased candidate changed only repository workflow guidance, evaluated to the same check derivations, and therefore had zero missing checks to build.

The candidate VM row is identical-derivation evidence rather than a second VM execution after rebase.

An initial environment probe ran `npx` before `npm ci` and resolved an unpinned registry version.

It was rejected as environment evidence, and the accepted capture records the lock-installed Playwright 1.61.1 after clean installation.

The selected SPA totals are 185 of 192 statements, 58 of 70 branches, 66 of 67 functions, and 171 of 174 lines.

The production build retained Vite's informational warning that one minified chunk exceeds 500 kB.

The three reviewed `npm outdated` rows were `@playwright/test` `1.61.1 → 1.62.1`, `@types/node` `24.13.3 → 26.4.1`, and TypeScript `5.9.3 → 7.0.2`.

The first human-table policy parser included Nix's preceding warning line and failed its own row-set assertion.

The corrected JSON-based assertion preserved npm's expected status 1 and proved that exactly the three approved rows remained.

That was an auxiliary evidence-parser error, not a package, test, or candidate failure.

Key successful Nix outputs are:

- SPA distribution: `/nix/store/7vwfkbhnjd5f81b9jaggj1qs9nnhspml-underwrap-web-dist-0.1.0`;
- `underwrapd`: `/nix/store/5263l3mwkq96xs4qan90acc0ls6jjmfz-underwrapd-0.1.0`;
- backend: `/nix/store/5nagk7h288z8q6mzglw8yw5lhhwr7isv-underwrap-backend-0.1.0`;
- Nix SPA coverage test: `/nix/store/7gjysw7caly76fsrs1b6hhhr5ril906y-underwrap-web-test-0.1.0`;
- Rust LLVM coverage report: `/nix/store/bshm2i2vsdp32c1smbqv79ln41ksc25d-underwrap-llvm-cov-0.1.0`;
- NixOS VM result: `/nix/store/ix170c00qsp3xc66yfbml42ah24h4ak4-vm-test-run-underwrap-vm-test`.

The output-path capture has SHA-256 `7d288a1a6c0b2c0a4b1cfe802d27019cfe08dc5a3ade08409f84c2f80ea2ea98`.

## Red-row disposition before the final candidate

The exact-candidate matrix above has no unexplained red row.

Earlier candidate validation stopped and restarted after each accepted correction.

The failures and dispositions are owned by [`2026-09-01_workspace-validation-stability.md`](2026-09-01_workspace-validation-stability.md):

- an unreadable restrictive-umask coverage profile made LLVM 22.1.6 terminate during merge;
- a target-status EOF witness observed a writer temporarily inherited across a parallel spawn;
- a daemon pidfd close witness observed a reused process-global descriptor number;
- the equivalent stage-one relocation witness was isolated before the final restart.

The original process-local integration backend port collision, compatible frontend lock refresh, and earlier namespace descriptor witnesses were likewise corrected without retrying, delaying, lowering concurrency, or weakening assertions.

The later bounded review found that separate cooperating nextest processes remained outside that first mutex.

Correction `efecdb1` adds cross-process advisory coordination through HTTPS readiness while documenting that an unrelated process can still claim a released loopback port.

## Disposable deployment walkthrough

A purpose-built NixOS VM exercised one backend, one frontend instance, and one stateless env wrapper against original candidate `f61d8dd`.

The rebased integration candidate differs from that walkthrough tree only in repository workflow guidance and received no separate walkthrough execution.

The successful VM result is `/nix/store/gs9pipra4sxsz6cwrqjvbn1mxjv52wi4-vm-test-run-underwrap-release-readiness-walkthrough`.

The complete successful log has SHA-256 `534d06c67c24f3a3c26b8b8805a952f8ad43fe2556fb9e47a3c8d16fe94392c6`.

The final temporary Nix expression and test script had SHA-256 values `53433b8c54e61135d07df958315e099c876e49b4456b2bee3e2397440828e507` and `0c2c1f6ad5842c7a74b5132d5b1708b776c1b12299eb598a6aaaa850799bafb6`.

The temporary scripts remained outside the repository.

| Tutorial step | Evidence classification | Result |
|---|---|---|
| Configure one NixOS frontend and wrapper | Tree-equivalent direct | Module evaluation, VM boot, daemon startup, shim, sidecars, setgid entry, dedicated target row, and hardened host posture passed |
| Create backend state and bootstrap the first operator before listener startup | Tree-equivalent direct | Dedicated service identity, state ownership, offline stdin-only bootstrap, and hardened service startup passed |
| Sign in and create a plain `demo` store | Simulated operator interaction on the tree-equivalent direct VM | Real HTTPS session-cookie and CSRF admin calls created the store before client enrollment |
| Generate and enroll the runtime client | Tree-equivalent direct key generation plus simulated operator interaction | Keygen installed the combined identity as the daemon account with mode `0400`; the real admin API enrolled its public certificate as an active client |
| Restart the daemon and register assets | Tree-equivalent direct | Auto-create produced `demo/token`, `secret-check → demo/token`, and the initial locked, non-writable permission |
| Add `env/DEMO_TOKEN` and unlock | Simulated operator interaction on the tree-equivalent direct VM | The real secret and permission endpoints stored a non-production UTF-8 value and selected `Unlocked`, `writable=false` |
| Run the wrapper | Tree-equivalent direct | Both the bare `secret-check` shim and explicit `underwrap-demo secret-check` succeeded and printed only the fixed availability message |
| Verify deployment state | Tree-equivalent direct | Service, setgid group/mode, identity owner/mode, client, resource, pair, permission, `last_used_at`, target UID mapping, TLS verification, backend hardening, state mode, and selected journal non-disclosure checks passed |
| Restart the plain backend | Tree-equivalent direct | The runtime client reconnected and the wrapper succeeded without restoring a key |

The operator browser clicks were not replayed inside this VM.

The equivalent authenticated API transitions are therefore labelled simulated operator interaction.

The independent candidate browser suite passed 38 real Playwright flows, but that evidence is not represented as one combined browser-through-disposable-VM test.

Three earlier temporary walkthrough harness attempts stopped on incorrect harness assumptions: an omitted optional `key_hex` field for a plain store, non-UTF-8 bytes chosen for an env value, and an incorrect setgid mode expectation.

The production behavior matched the current API and module contracts, and no product documentation correction was required.

## Separate-host guide reconciliation

The walkthrough used one disposable VM and loopback HTTPS.

It did not claim a separate-host deployment.

The following guide controls have direct same-host evidence:

- backend-owned rustls HTTPS and server-name/IP verification;
- a separate backend service identity;
- mode `0700` service-owned state and mode `0400` runtime identity;
- offline bootstrap before listener startup;
- runtime credential generation, enrollment, and daemon restart;
- auto-created assets, locked initial permission, explicit unlock, and wrapper success;
- backend and daemon selected log-hygiene checks;
- hardened service properties including no new privileges, private temporary storage, protected system/home, empty capabilities, core limit zero, and restrictive umask.

The candidate's main NixOS VM separately passed encrypted-store restart, locked-state, correct-key unlock, wrong-key rejection, rotation, and retained-generation recovery evidence.

The same VM passed cached-runtime-client reconnect and credential-cutover restart evidence.

DNS, non-loopback SANs, firewall traversal, routing, latency, partitions, distinct-host compromise, and multi-machine orchestration are unavailable in this environment.

The walkthrough used repository test PKI and did not validate production CA-key custody, production TLS-key placement, external vault practice, or operator browser trust installation.

## Contract and documentation checks

The workspace OpenAPI export tests passed against the committed generated contract.

Both SPA test and build commands regenerated TypeScript bindings successfully from that contract.

The full flake check passed the two shipped example evaluations, frontend URL admission, security assertions, swap posture, Rustdoc, audit, and supply-chain policy checks.

All 247 relative documentation paths resolved, and the targeted relative-anchor check passed.

The path and anchor check logs have SHA-256 values `8c8b3774b03714e4f873498d300a3d9d2fdf9104f1902f619f34294be57ad847` and `b75ae065c1db2a3524ee300fc0958db3137b32b07a59f966bb56ddee5b166ef7`.

The new research prose passed the one-sentence-per-line heuristic, and the bounded credential-pattern scan returned no match.

No current product guide claims a completed release or independent end-to-end audit.

## Residual limits and nonclaims

AArch64 Linux packages and checks were evaluated but not built or executed.

The separate-host properties listed above remain unavailable.

A bounded pass cannot prove the absence of every race.

LCOV report byte reproducibility under `nix build --rebuild` was not established, although ordinary coverage generation passed.

The VM observation correction does not identify the lower-level reason one earlier monolithic test-driver response was short; it prevents that observation path from carrying a multi-megabyte response.

Coverage thresholds and totals are regression evidence, not correctness or security proof.

No production credential, host, resource, release tag, publication path, or version bump was used.

An initial delegated bounded review attempt failed before startup because the installed Pi bundle lacked `dist/bundle/cli.js`.

That infrastructure failure produced no review evidence and does not count as independent acceptance.

A later fresh-context bounded internal review completed, found no Critical or Major product defect, and produced the two Minor findings and limitations recorded in the linked review record.

A second fresh-context bounded internal verification accepted exact correction range `8ce345c333a91ce937cf39dd85124c2dd772b8ae..2d24142462fb8589ccb23246b39c16e0684e703f`, classified both original Minor findings as fixed, and reported no unresolved Critical, Major, or Minor correction defect.

At that exact tip, all-system flake evaluation without builds and all 125 compatible current-system flake checks passed; the latter included `workspace-nextest`, LLVM coverage, and the NixOS VM.

This review and follow-up verification do not constitute an external independent end-to-end security audit.
