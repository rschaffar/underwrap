# Agent-facing wrappers

underwrap is especially interesting when the caller is an AI agent or other automation that should be able to **use** a capability without being handed the reusable secret behind it.

This document explains the pattern underwrap is designed for, and just as importantly, where its limits are.

## The core idea

Instead of giving an agent a raw token such as `GH_TOKEN`, you give it a command.

Prefer this:

```text
agent-gh-comment <pr-number> <body-file>
```

Over this:

```text
gh <anything>
```

underwrap then injects the token only for that wrapper invocation.

## The recommended shape

The strongest pattern is usually:

```text
agent
  -> /run/wrappers/bin/agent-gh-comment     # setgid underwrap entry wrapper
  -> /nix/store/.../bin/agent-gh-comment    # your narrow helper
  -> /nix/store/.../bin/gh                  # real CLI
```

This separates responsibilities:

- **underwrap** handles secret delivery and the wrapper chain
- **your helper** defines the allowed action surface
- **the real CLI** does the actual API call

## Why narrow wrappers matter

underwrap is not a behavior sandbox.

If you wrap the full `gh` CLI directly, then whoever can run that wrapper can still do whatever the token permits through `gh`.

That may still be acceptable for some human-operated workflows. It is usually the wrong trade-off for agents.

A narrow helper is where you decide:

- which repository is fixed
- which subcommand is fixed
- which flags are allowed
- which arguments are validated
- whether file paths are allowed and how they are interpreted

## A concrete example

The repository includes an example Nix expression at:

- [`examples/agent-github-comment.nix`](../examples/agent-github-comment.nix)

That example builds a wrapper that only allows:

```text
gh pr comment --repo myorg/myrepo <pr> --body-file <file>
```

The caller can choose the PR number and comment body file, but not:

- an arbitrary repository
- an arbitrary `gh` subcommand
- arbitrary extra flags

That is the right general pattern.

## What underwrap contributes in this setup

When used this way, underwrap is trying to reduce several common failure modes.

### 1. The token does not need to live in the agent's shell environment

That means less risk of accidental leakage through:

- shell startup files
- process supervisors
- prompts and debugging output
- framework-level environment capture

### 2. The main target path is fixed by trusted configuration

underwrap reads the configured target from a root-owned sidecar config in `/etc/underwrap/wrappers/`.

That means the caller does not get to swap in an attacker-chosen target binary for the main wrapper hop.

### 3. The target starts from a clean base environment

underwrap uses a cleared environment and then restores only a safe default whitelist plus trusted per-wrapper additions.

That helps reduce:

- `LD_PRELOAD` / `LD_LIBRARY_PATH` tricks
- malicious `UNDERWRAP_*` variables
- general ambient-environment weirdness

### 4. Wrapper identity is kernel-backed

The daemon does not trust a caller-supplied wrapper name. It derives identity from the wrapper's effective GID.

That is important when multiple wrappers exist with different secrets and different intended targets.

### 5. Secrets can stay off the target host

With the remote backend, you can keep the real secrets on your workstation and expose them to the target machine only through an SSH-tunneled backend socket.

## What underwrap does **not** solve for you

This is the most important section.

underwrap does **not** by itself answer these questions:

- Is the wrapped action too broad?
- Can the target CLI itself reveal the secret?
- Can the agent abuse a legitimate command in a harmful way?
- Is the downstream token scope too powerful?
- Does your helper validate arguments well enough?

Those are wrapper design and token-scope questions.

underwrap helps with **secret delivery and process handling**. It does not replace careful wrapper design.

## Design guidelines for agent-facing wrappers

### Prefer commands over shells

Good:

```text
agent-gh-comment <pr> <body-file>
```

Less good:

```text
agent-gh --comment <pr> --body-file <file>
```

The narrower and more explicit the interface, the easier it is to audit.

### Pin downstream tools by absolute path

If your helper runs `gh`, `jq`, `curl`, or anything similar, prefer absolute Nix store paths over `PATH` lookups.

### Validate every free-form argument

Typical examples:

- numeric PR numbers
- existing readable file paths
- fixed repo names
- bounded message sizes
- known-safe flag subsets

### Use the narrowest token possible

underwrap reduces the risk of casually exposing the raw token. It does not make an overpowered token safe.

### Assume the caller is creative

Even if you do not treat the agent as fully adversarial, it is wise to assume it will eventually try weird argument combinations, unexpected files, or inconvenient edge cases.

## Suggested rollout path

A practical adoption sequence is:

1. get a simple wrapper working with [Getting started](getting-started.md)
2. wrap one narrow helper for a single agent action
3. review the helper as if it were an API surface
4. only then consider broader or more stateful workflows

## Security notes

If you want the deep design rationale behind the wrapper chain and identity model, read:

- [Architecture](architecture.md)
- [Security](security.md)

The short version is that underwrap is trying to make direct token extraction less convenient and less ambient than plain environment-variable workflows. That is valuable, but it is only part of the overall agent-safety story.
