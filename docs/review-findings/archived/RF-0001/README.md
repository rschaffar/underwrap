# RF-0001 — Replaceable staging root can substitute collected files

**Priority:** High · **State:** archived · **Resolution:** fixed

**Review aliases:** `N-01` · **Confidence:** High source confidence; synthetic discovery witness and packaged correction verification.

## Why it matters

A caller who preclaims the original staging directory can replace its pathname so a legitimate writable invocation collects caller-supplied files.
Earlier replacement can also move materialization outside the private tmpfs.

## Correction and disposition

Fixed in `365916646f67bda32fcd1ab356baaeead1361925`.
One private `noswap` tmpfs is attached through checked target-runtime descriptors; retained projection handles select materialization, binding and collection.
Distinct-UID shipped-chain VM regressions verify early and post-bind legacy-name replacement, exact persistence and read-only controls.

## Caveat

The original cross-UID exploit and direct secret recovery were not executed.
Correction verification is scoped to the tested VM kernel; libsecret bus-path hardening remains outside this finding.
Backend authorization and whole-blob last-writer-wins are unchanged.
The alias belongs to review baseline `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
