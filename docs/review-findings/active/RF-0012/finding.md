# RF-0012 — Runtime-base paths are not escaped for tmpfiles syntax

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`targetRuntimeDirBase` accepts lexical path components containing percent specifiers such as `%t`, a literal backslash sequence such as `back\x20slash`, or a quote.
The path grammar rejects whitespace/control and dot components but not these characters (`nix/modules/underwrap.nix:25-47,756-758`).
The literal base is passed to the daemon at `nix/modules/underwrap.nix:165` and interpolated unescaped into tmpfiles fields at `nix/modules/underwrap.nix:675-681`.

Tmpfiles has a different grammar: percent expansion, backslash unescaping and quoting can change the path or invalidate a rule.
A Nix string accepted as a literal filesystem path is therefore not necessarily preserved by the generated provisioning language.
The root-owned base and target-owned 0700 children can be absent at the exact configured names.

## Consequence and boundary

The affected asset is runtime-directory selection, ownership provisioning and availability.
The boundary is trusted Nix configuration to generated tmpfiles syntax, not an unprivileged caller to root-code boundary.
A runtime path disagreement can prevent target setup; an unmatched quote also causes parser failure.
`docs/configuration.md:64-101` describes admitted lexical paths and distinct runtime identities, but does not document this parser restriction.

The coordinator's actual-module evaluator confirmed admission and literal rule emission for all three examples.
It used x86_64-linux, a container-style configuration, `hardenTargetHost=false`, a synthetic package and one harmless wrapper.
This isolates generation; it does not establish a running deployment.
The later evaluation supersedes the original specialist's evaluation-pending note, not the continuing activation gap.

## Effect of RF-0001

At `7d4b1d30794171956031cef45b805b0c46500619`, [RF-0001](../../archived/RF-0001/README.md) replaces shared-path files staging with a descriptor-selected runtime attachment.
`crates/underwrap-ns/src/directory_credential.rs:134-152` opens the configured target runtime directory and requires exact visible target UID/GID and mode 0700 before creating the private tmpfs attachment.
`crates/underwrap-ns/src/main.rs:224-245` performs this setup before the namespace fork, including for files-only wrappers.
A missing or differently interpreted provisioned path therefore rejects files setup before target execution; later handle retention does not correct provisioning syntax.
The lexical admission and unescaped tmpfiles generation remain unchanged, so this finding remains open rather than becoming a caller-to-root security claim.

## Correction and alternatives

Encode paths correctly for tmpfiles, covering specifiers as well as quoting/escapes, or reject unsupported characters with a clear module assertion and documented grammar.
Keep custom bases and the existing `/run/user` exclusion; do not replace this targeted correction with a general ban on configuration flexibility.
Shell quoting alone must not be assumed to implement tmpfiles quoting.

## Acceptance criteria

- For ordinary and percent/backslash/quote cases, compare admitted literal configuration, daemon path and parser-selected base/child paths.
  They must agree exactly, or unsupported input must fail an intentional evaluation assertion.
- Verify the specific assertion rather than accepting an unrelated evaluation failure.
- In an authorized disposable VM, check exact paths and root/target ownership and permissions after activation; no differently interpreted path should be created.
- Preserve ordinary custom-base and `/run/user` rejection controls.
- Include a files-only wrapper in custom-base activation controls, verifying exact runtime-directory ownership/mode and successful descriptor-based staging at the admitted literal path.
  Early setup rejection for a misprovisioned path is not a substitute for correcting module admission or encoding.
  These are future acceptance checks, not recorded fix results.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
