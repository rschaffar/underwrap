# ADR-0002: Use a strict privileged configuration grammar

Status: Accepted

Date: 2026-08-25

## Decision

Validate the root-managed NixOS and daemon configuration before privileged artifacts or services consume it.

Instance names match `[a-z][a-z0-9_-]{0,20}`.

Wrapper names match `[A-Za-z0-9][A-Za-z0-9._+-]{0,127}` and exclude the reserved `debug` selector.

Privileged paths are canonical absolute paths, except that wrapper access paths also accept exact `~` and canonical `~/...` paths in their separate tilde domain.

Reject controls, whitespace-bearing names, traversal, empty or dot components, duplicates, and overlapping access paths where the current configuration contract forbids them.

Keep resource names outside this grammar because they belong to the backend resource contract rather than privileged local selector generation.

[`docs/configuration.md`](../configuration.md) is the user-facing contract, and [`docs/testing.md`](../testing.md) owns both the daemon configuration tests and the Nix module's fail-closed evaluation matrix.

## Alternatives considered

Deferring validation until service startup would allow root-owned users, groups, wrappers, paths, or configuration files to be generated from values that only fail later.

Silently normalizing names or paths would make configured selectors differ from the values operators supplied and could hide collisions.

Keeping a broad grammar for compatibility is unnecessary before the first release and would preserve ambiguity without a deployed compatibility benefit.

## Consequences

Invalid privileged configuration fails before deployment or daemon admission instead of selecting a weaker runtime fallback.

The Nix module and daemon parser enforce the same selector and path intent at their respective boundaries.

Future grammar changes are interface decisions and require synchronized configuration documentation and positive and negative evaluation evidence.
