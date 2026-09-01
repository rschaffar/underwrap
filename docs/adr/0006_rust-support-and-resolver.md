# ADR-0006: Pin the Rust support matrix and retain resolver 2

Status: Accepted

Date: 2026-08-31

## Decision

Declare Rust 1.97.1 as the minimum compiler for every workspace package and pin Rust and Cargo 1.97.1 in the Nix development, check, and release environment.

Retain Cargo resolver 2 for the edition-2024 workspace.

Use the exact pinned compiler build as the enforcement evidence when dependency re-resolution selects a package that requires newer Rust.

Keep `Cargo.lock` as the selected dependency graph and do not promise independent support for newer compilers merely because `rust-version` is a minimum.

[`docs/testing.md`](../testing.md) is the maintained support and verification contract, while the root and member Cargo manifests carry the machine-readable package metadata.

## Alternatives considered

Omitting `rust-version` would leave package metadata and downstream Cargo tooling unable to distinguish the support floor from the current Nix pin.

Resolver 3 could fall back to older compatible dependency versions during re-resolution, which would hide a newer dependency requirement rather than expose it at the exact supported compiler build.

Promising every newer compiler would exceed the pinned validation matrix.

## Consequences

Older compilers are unsupported, and the exact 1.97.1 matrix must remain green.

A re-resolved dependency that exceeds this floor fails visibly instead of silently selecting an older release through resolver fallback.

Raising the floor or changing resolver policy requires synchronized workspace metadata, lock and dependency evidence, Nix tooling, and testing documentation.
