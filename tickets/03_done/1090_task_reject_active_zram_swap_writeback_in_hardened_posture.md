# Reject active zram swap writeback in the hardened host posture

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-010
Source themes: C-002

## Outcome

`services.underwrap.hardenTargetHost = true` fails closed whenever a NixOS zram-generator swap device can write memory pages to a backing device whose encryption underwrap cannot positively establish. Pure in-memory zram remains valid, as do the existing accepted ordinary-swap postures; disabling `hardenTargetHost` remains the explicit weaker posture.

The pinned high-level `zramSwap.writebackDevice` option carries only a device path and passes that path to zram-generator for each generated zram device. The same final generator configuration can also be supplied through the pinned `services.zram-generator.settings` interface. A path spelling, `/dev/mapper` prefix, or operator assertion is not proof that every underlying writeback block is encrypted for the lifetime of the mapping. The production assertion therefore inspects the final enabled zram-generator configuration, not only the high-level `zramSwap` selector.

Under the pinned generator semantics, only device sections beginning with `zram` and followed by a Rust-`u64`-parseable suffix are materialized; that grammar includes edge spellings such as `zram+0` and leading zeroes. Reimplementing that numeric parser in Nix would add a brittle bypass surface. The hardened assertion instead treats every final section whose name begins with `zram` as a device candidate, a deliberate fail-closed superset of every parser-recognized name. A candidate is swap-formatted exactly when `mount-point` is absent and `fs-type` is absent or equals `swap`. The assertion rejects an enabled candidate that is swap-formatted and carries `writeback-device`, including malformed zram-prefixed candidates and candidates a runtime `host-memory-limit` could later suppress.

The pinned `formats.ini` emitter writes section names, key names, and string values without structural quoting sufficient to prevent parser normalization or newline injection. Under `hardenTargetHost = true` with zram-generator enabled, the approved local mitigation therefore validates the complete final typed settings before applying the zram swap/writeback predicate: every attrset section and key name must be a conservative canonical token, and every string value must contain neither CR nor LF. A noncanonical `fs-type` on a zram writeback candidate is rejected fail closed rather than accepted as a non-swap filesystem. Canonical non-zram sections and canonical mounted zram filesystem sections remain allowed. Disabling zram-generator bypasses this inert configuration check, and `hardenTargetHost = false` remains the explicit weaker admitted posture. This narrow behavior expansion was explicitly approved after a pinned-parser bypass was proven.

The pinned high-level module generates device sections from `lib.range 0 (zramSwap.swapDevices - 1)`, which is empty for zero and negative counts. Consequently, a non-null high-level writeback option is inert when zram is disabled or its device count is non-positive and does not appear in the final generator settings. This security correction intentionally changes NixOS configuration admission for previously accepted swap-writeback deployments and for ambiguous enabled typed settings; it does not claim to inspect live kernel swap state or to provide encrypted zram writeback.

## Scope

- In `nix/modules/underwrap.nix`, add one independent final-zram-generator swap-writeback condition to the hardened-host assertions. When `hardenTargetHost = true` and zram-generator is enabled, first require conservative canonical section/key tokens and reject CR/LF in every string value across the final typed settings, then require that no zram-prefixed, swap-formatted settings section contains `writeback-device`.
- Use section-name prefix `zram` as a deliberate fail-closed superset of the pinned parser's accepted device grammar, including `zram+0` and leading-zero forms, before applying its swap predicate: `mount-point` is absent and `fs-type` is absent or `swap`. Ignore non-section values defensively; malformed zram-prefixed swap/writeback candidates and noncanonical `fs-type` values are rejected rather than creating parser bypasses. Canonical non-zram sections and canonical mounted zram filesystem sections remain admitted.
- Keep the existing ordinary `config.swapDevices` encryption assertion independent and unchanged in effect. A configuration fails if either ordinary disk-backed swap is not admitted by that assertion or zram swap writeback is configured.
- Give the new assertion an operator-facing message that identifies zram swap disk writeback, explains that backing encryption cannot be verified from the configured path, and names the safe choices: disable swap writeback, use pure RAM-only zram/no swap, or select the explicitly weaker `hardenTargetHost = false` posture.
- Add `nix/tests/swap-posture-eval.nix` as a dedicated real-module evaluation matrix and wire it through `flake.nix` as `checks.x86_64-linux.frontend-swap-posture-eval`. Inspect the exact assertions, force `config.system.build.toplevel` with `builtins.tryEval`, and inspect final zram-generator settings so the test cannot pass vacuously.
- Keep `nix/tests/node.nix` and `nix/tests/test.py` in their valid hardened/no-swap posture unless evidence reveals a focused regression gap. Rerun the complete deployed VM as the required regression boundary for the NixOS module and privileged wrapper deployment.
- Synchronize the present-state claims in `README.md`, the module's `hardenTargetHost` option description, `docs/architecture.md`, `docs/configuration.md`, `docs/security.md`, `docs/agent-facing-wrappers.md`, and `docs/testing.md`. The strongest claim distinguishes existing-admitted ordinary swap, pure in-memory zram, prohibited zram swap writeback, and non-swap zram filesystems.
- Preserve deterministic pre-fix evaluations outside the repository showing that clean HEAD accepts both the high-level `zramSwap.writebackDevice` path and an enabled direct swap-formatted `services.zram-generator.settings` section: each has a generated `writeback-device`, leaves the existing underwrap ordinary-swap assertion true, and successfully evaluates the system toplevel under hardened mode.

Expected F-010 fix paths are therefore:

- `nix/modules/underwrap.nix`
- `nix/tests/swap-posture-eval.nix` (new)
- `flake.nix`
- `README.md`
- `docs/architecture.md`
- `docs/configuration.md`
- `docs/security.md`
- `docs/agent-facing-wrappers.md`
- `docs/testing.md`

Any approved behavior-preserving cleanup has its own separately reviewed scope and commit before the finding fix.

## Non-goals

- Do not infer encryption from a pathname, `/dev/mapper` naming, `boot.initrd.luks.devices`, unrelated `swapDevices` rows, operator prose, or live device topology.
- Do not add an operator attestation/allowlist, a new public option, automatic encryption, a zram-generator override, runtime probing, or support for zram swap writeback under `hardenTargetHost = true`.
- Do not reject inert high-level writeback configuration solely because the option is non-null when zram is disabled or the pinned module generates no zram devices.
- Do not reject a canonical zram-generator filesystem section merely because it uses kernel zram writeback; when the section has a mount point it is not a memory-swap path covered by F-010. Canonical non-zram freeform sections remain ignored by the swap/writeback predicate, but their section/key tokens and string values are subject to the approved ambiguity checks while hardening and the generator are enabled. Do not redesign direct generator configuration or attempt to police arbitrary operator-authored `/etc` drop-ins outside the typed NixOS options evaluated by the module.
- Do not redesign or weaken ordinary swap encryption semantics, validate zram device-count policy, change the pinned nixpkgs/generator packages, configure swap, migrate backing devices, or broaden the claim to hibernation, zswap, tmpfs `noswap`, memory locking, core dumps, or kernel/Yama assertions.
- Do not change any Rust/web protocol, backend, resource, target-UID, capability, namespace, service, binary, dependency, lockfile, or `underwrap-exec -> underwrap-ns` chain.
- Do not absorb F-011, F-018, F-024, any medium/low finding, Opportunity, unresolved item, or the high-to-medium transition.

## Acceptance criteria

### Hardened assertions

- With `services.underwrap.enable = true`, `hardenTargetHost = true`, and `services.zram-generator.enable = true`, the final typed settings are rejected unless every attrset section and key name is a conservative canonical token and every string value contains neither CR nor LF. Only after that complete ambiguity check does the evaluator apply the zram swap/writeback predicate.
- Zram swap writeback is rejected whenever any final zram-prefixed settings section has `writeback-device`, lacks `mount-point`, and lacks `fs-type` or sets `fs-type = "swap"`. A present noncanonical `fs-type` is rejected fail closed rather than treated as proof of a non-swap filesystem.
- The zram-prefix rule deliberately contains every name accepted by the pinned generator parser, including `zram+0` and leading-zero spellings, and rejects malformed zram-prefixed swap/writeback candidates rather than risking a narrower parsing bypass. Canonical non-zram sections and canonical mounted zram filesystem sections do not create false failures. The check uses presence of `writeback-device` as fail-closed evidence, does not inspect or log the backing path, evaluates only section attrsets, and handles the freeform settings shape without crashing.
- The zram-writeback assertion is separate from the existing ordinary-swap assertion. Mixed invalid posture exposes both failed conditions; correcting one does not mask the other.
- The assertion message does not claim that underwrap can recognize encrypted backing from a device path. It identifies the memory-to-disk risk and the explicit weaker-posture escape.
- `hardenTargetHost = false` omits both underwrap swap-posture assertions and permits the underlying NixOS configuration without silently presenting the strongest claim.

### Evaluation matrix

The dedicated `checks.x86_64-linux.frontend-swap-posture-eval` check covers real `nixpkgs.lib.nixosSystem` evaluations importing the production underwrap module for every row below:

| Hardened | Ordinary swap | zram configuration | Expected |
| --- | --- | --- | --- |
| true | none | high-level zram disabled, null writeback | allow |
| true | existing-admitted random-encrypted | zram disabled | allow |
| true | unencrypted device | zram disabled | reject ordinary-swap assertion |
| true | none | high-level enabled, count 1, null writeback (pure zram) | allow |
| true | otherwise valid | high-level enabled, count 1, non-null writeback | reject zram-writeback assertion |
| true | none | high-level disabled, count 1, non-null inert writeback option | allow; generator disabled and no writeback emitted |
| true | none | high-level enabled, count 0, non-null inert writeback option | allow; no zram section or writeback emitted |
| true | none | high-level enabled, count -1, non-null inert writeback option | allow; no zram section or writeback emitted |
| true | none | direct generator enabled, swap-default section with writeback | reject zram-writeback assertion |
| true | none | direct generator enabled, explicit `fs-type = "swap"` section with writeback | reject zram-writeback assertion |
| true | none | direct generator enabled, parser edge section `zram+0` with swap-shaped writeback | reject zram-writeback assertion |
| true | none | direct generator enabled, parser edge section `zram00` with swap-shaped writeback | reject zram-writeback assertion |
| true | none | direct generator enabled, malformed zram-prefixed section with swap-shaped writeback | reject fail closed |
| true | none | `direct-section-name-normalization`: enabled direct section name normalized by the parser to a zram candidate | reject ambiguous section name fail closed |
| true | none | `direct-writeback-key-normalization`: enabled direct writeback key normalized by the parser | reject ambiguous key name fail closed |
| true | none | `direct-non-zram-value-injection`: enabled direct non-zram string value injects a zram section and writeback directive with LF | reject multiline value fail closed |
| true | none | `direct-non-zram-key-injection`: enabled direct non-zram key injects a zram section and writeback directive with LF | reject multiline key fail closed |
| true | none | `direct-noncanonical-swap-fs-type`: enabled direct zram writeback section with noncanonical swap `fs-type` | reject ambiguous `fs-type` fail closed |
| true | none | direct generator enabled, canonical filesystem section with mount point and writeback | allow; section is not swap-formatted |
| true | none | direct generator enabled, canonical non-zram section with swap-shaped writeback keys | allow; section is outside the zram device namespace |
| true | none | direct generator disabled, recognized swap-shaped section with writeback | allow; generator is inert |
| true | unencrypted device | high-level enabled, count 1, non-null writeback | reject both independent assertions |
| false | unencrypted device | direct generator enabled, swap-default section with writeback | allow under the explicit weaker posture |

- Every hardened allowed row has both underwrap swap assertions present and true and has a successful forced `system.build.toplevel` evaluation.
- Every hardened rejected row has the exact expected false underwrap assertion set and a failed forced `system.build.toplevel` evaluation; unrelated false assertions are not accepted as evidence.
- The weaker-posture row has both underwrap swap assertions absent and a successful forced toplevel evaluation.
- High-level enabled count-1 writeback is proven active by the final zram0 `writeback-device`; pure zram has a final zram section without that setting; zero/negative counts create no section; disabled high-level zram does not enable the generator or emit a section even if its inert option is non-null.
- Direct settings witnesses distinguish canonical/default and explicit swap sections, parser edges `zram+0` and `zram00`, a malformed zram-prefixed fail-closed candidate, a canonical mounted filesystem section, and a canonical non-zram section, and distinguish enabled from disabled generator configuration.
- The five evaluator rows for section-name normalization, writeback-key normalization, non-zram value injection, non-zram key injection, and noncanonical swap `fs-type` each reject with exactly the zram-writeback assertion false and prove that the final generator settings retain the typed witness.
- Canonical non-zram and canonical mounted-zram rows remain allowed. Generator-disabled settings and the `hardenTargetHost = false` row remain admitted, with the weaker-posture row omitting both underwrap swap assertions.
- The check uses synthetic paths only as evaluation fixtures and does not assert that path spelling proves encryption.
- The matrix does not use multiple high-level zram devices with one writeback device because the pinned zram module independently rejects that configuration; this keeps attribution to the underwrap assertion exact.

### Documentation and regression

- `hardenTargetHost` documentation says the strongest posture permits no swap, ordinary swap already admitted by the module's encryption assertion, and pure zram without swap writeback; a zram-generator swap section with writeback requires the explicitly weaker posture.
- Published docs make no claim that all zram writeback is rejected, that zram backing is encrypted, that an inert option creates a disk path, that a mounted zram filesystem is memory swap, or that runtime device state/arbitrary external generator configuration is inspected.
- The existing NixOS VM still boots and passes with hardened mode and no swap, including its Yama, identity, namespace, capability, lifecycle, resource, and log-hygiene evidence.
- No source, generated, or validation artifact remains outside the exact task allowlist.

## Security boundary

- Protected asset: secret-bearing frontend memory pages, including target and helper resource material, after the kernel selects them for swap.
- Attacker: a later reader of an unencrypted zram swap writeback backing device, such as after disk reuse, loss, or offline access. Root/kernel compromise of the running host remains out of scope.
- Boundary direction: secret-bearing memory -> zram compressed swap -> configured writeback backing device. The strongest NixOS posture rejects an unverified disk leg before deployment evaluation succeeds.
- Invariant: under `hardenTargetHost = true`, each swap path represented by ordinary `config.swapDevices` remains governed by the existing encryption assertion. When zram-generator is enabled, the complete final typed settings must first have canonical section/key tokens and single-line string values; each zram-prefixed, swap-formatted section is then rejected if it carries writeback, and a noncanonical `fs-type` cannot establish a non-swap filesystem. This fail-closed superset covers every pinned parser-recognized device name while preserving pure RAM-only zram, canonical mounted zram filesystems, canonical non-zram sections, disabled-generator settings, and the explicit weaker posture as specified.

## Validation

Run resource-intensive checks sequentially.

- Preserve both clean-HEAD pre-fix evaluator outputs and SHA-256 hashes outside the repository: the high-level `zramSwap.writebackDevice` counterexample and the direct enabled swap-formatted zram-generator counterexample.
- Record the clean-HEAD baseline that `nixfmt --check nix/modules/underwrap.nix flake.nix` already reports both files as unformatted; do not introduce whole-file formatting churn to mask that baseline.
- `nixfmt --check nix/tests/swap-posture-eval.nix`
- `nix-instantiate --parse nix/modules/underwrap.nix`
- `nix-instantiate --parse nix/tests/swap-posture-eval.nix`
- `nix-instantiate --parse flake.nix`
- `nix build .#checks.x86_64-linux.frontend-swap-posture-eval`
- `nix build .#checks.x86_64-linux.vm-test`
- `nix flake check`; if the known clean-baseline LLVM 21 `llvm-profdata merge` SIGILL recurs after coverage tests pass, preserve the exact output and do not misattribute it to F-010.
- Static checks proving the production assertion, exact matrix, final generator witnesses, and synchronized present-state documentation.
- `git diff --check`
- Exact independently authored staged-path allowlist, empty unstaged/untracked set, and staged credential-pattern scan before each commit.

No standalone Rust, SPA, or browser check is required unless an approved cleanup changes those surfaces. The full VM and flake checks still compile and exercise the packaged system.

## Dependencies

- Completed and completion-accepted prior high findings F-005 through F-009 provide the current deployment, transport, lifecycle, and resource boundaries; F-009 final fix is `6fa33dc39d7a2d773c243db1b5c9473765a0008e`.
- No unresolved implementation dependency. F-011 remains the next separately gated high finding; F-018/F-024 and all other findings/Opportunities remain separate.
- Mandatory repository-wide and ticket-local behavior-preserving cleanup discovery begins only after this exact ticket is approved. No cleanup unit is pre-approved.
