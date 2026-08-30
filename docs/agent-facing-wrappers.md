# Agent-facing wrappers

underwrap is designed for automation that should be able to use a capability
without receiving the reusable secret behind it.

The central design rule is:

> Expose a narrow command, not a broad CLI with a hidden token.

Prefer:

```text
printf '%s\n' 'reviewed' | agent-gh-comment <pr-number>
```

over:

```text
gh <anything>
```

## Recommended shape

```text
agent
  → agent-gh-comment <args>                       per-user shim
  → underwrap-<instance> agent-gh-comment <args> setgid entry
  → underwrapd                                    peer + backend policy
  → /nix/store/.../bin/agent-gh-comment          daemon-chosen target
  → /nix/store/.../bin/gh                        fixed downstream CLI
```

Responsibilities stay separate:

- underwrap controls secret delivery, target selection, identity, namespaces,
  and writeback;
- the narrow helper defines the action surface;
- the downstream CLI performs the service request;
- backend policy decides whether the runtime client/wrapper/resource tuple is
  active and writable.

## Why the helper matters

underwrap is not a behavior sandbox. Wrapping the complete `gh` CLI lets the
caller exercise whatever behavior the configured target and token permit.
Hiding the token does not make the action surface narrow.

A helper can fix:

- repository or service account;
- subcommand and verb;
- allowed flags;
- argument grammar and size;
- accepted input files;
- downstream tool paths;
- output shape.

Review that helper like an API exposed to a creative caller.

## Example

[`examples/agent-github-comment.nix`](../examples/agent-github-comment.nix)
defines a helper that permits only:

```text
gh pr comment --repo myorg/myrepo <pr> --body-file <private-buffer>
```

The caller controls the validated PR number and supplies at most 64 KiB of
comment text on stdin. The helper copies those bytes into its dedicated target
UID's private runtime directory, registers cleanup for normal and handled-signal
exits, and never accepts a caller-selected path. That last property matters: allowing an arbitrary readable
path could direct the secret-bearing helper to `/proc/self/environ` and disclose
the projected token. The repository, subcommand, and flags remain fixed.

The example also pins `gh` to its Nix store path and supplies complete frontend
and backend connection configuration.

## What underwrap contributes

### Invocation-scoped delivery

The secret does not need to live in the caller's shell environment, startup
files, automation framework configuration, or ordinary CLI config.

For env projection, the final target still receives the raw value for its
lifetime. The helper must not print, log, persist, or expose it through
caller-controlled behavior.

### Daemon-chosen target

The daemon reads the absolute target path from root-owned config after
validating the wrapper selector and receiving backend authorization. The caller
cannot substitute another top-level binary through argv, `argv[0]`, or `PATH`.

### Cleared target environment

The target starts with `env_clear()`. underwrap adds its fixed built-in
passthrough, configured additions, trusted fixed values, and selected resource
env entries.

This blocks ambient `LD_PRELOAD`, `LD_LIBRARY_PATH`, caller `PATH`, and
wrapper-control variable injection. The built-in passthrough still includes
caller-controlled temporary-directory, display, SSH-agent, and proxy values and
cannot be reduced per wrapper in this release. Include those surfaces in the
helper review.

### Dedicated target UID

Every `(instance, wrapper)` runs under a dedicated UID/GID, separate from the
caller and other wrappers. This is the primary boundary against caller-owned
processes reading target `/proc` state.

Under the complete hardened posture, the NixOS module also requires Linux 6.5+,
defaults Yama `ptrace_scope=2` with `mkDefault`, and admits no swap, ordinary
swap admitted by the random-key/LUKS assertion, or pure RAM-only zram. Enabled
typed zram-generator swap sections with writeback are rejected because path
encryption cannot be established. The predicate permits writeback for canonical
mounted zram filesystem sections and ignores canonical non-zram sections because
neither is classified as memory swap. When zram-generator is enabled, all final
typed section and directive names must nevertheless be canonical tokens and all
string values, including those in mounted and non-zram sections, must be
single-line. Lowering Yama, adding external `/etc` drop-ins or runtime device
changes outside the typed configuration evaluated by the module, or disabling
the posture weakens the claim. See
[Security](security.md#security-claim) for the qualified claim.

### Central resource and policy

Active underwrap-managed durable brokered resource material stays in the backend's selected `StorageBackend`.
The target host sees the projection in process memory.
Files bytes use namespace-private `noswap` tmpfs for one invocation.
Libsecret resource state remains in process memory, while its namespace-private bus tmpfs carries only runtime socket and helper artifacts without a `noswap` claim.
Independently managed credentials and trusted configuration are separate domains described in [Security](security.md#design-invariants).
The configured target can still print, persist, or transmit its projection, including when backend writeback is read-only.
The hardened posture permits only the swap cases described above.

The backend can revoke a permission without reconfiguring the frontend host.

## What underwrap does not contribute

underwrap does not answer:

- whether the action is too broad;
- whether the target can reveal its projection;
- whether plugins, hooks, config files, or descendants add behavior;
- whether arguments permit path traversal, flag injection, or confused-deputy
  behavior;
- whether the downstream credential is over-scoped;
- whether a legitimate action can be abused;
- which local Unix user is invoking the wrapper.

The last point is easy to miss: `allowedUsers` installs PATH shims, but the
system-wide instance entry point is executable independently and backend policy
identifies the frontend runtime client instance, not the Unix caller. Add sudo,
polkit, or another OS-level gate when local-user authorization matters.

## Helper design guidelines

### Pin every executable

Use absolute Nix store paths for the helper and all downstream tools. Avoid bare
lookups for `gh`, `git`, `ssh`, `jq`, `curl`, shells, pagers, editors, and
browsers.

If a target requires `PATH`, set a fixed non-secret trusted `envSet.PATH`.
Never put a brokered value or other secret in `envSet`, because Nix materializes it in generated daemon configuration.

### Validate every caller-controlled value

Examples:

- numeric and bounded issue/PR identifiers;
- fixed or strictly parsed repository names;
- explicit enum-like flag choices;
- maximum body and file sizes;
- clean paths under an allowed root;
- regular-file checks where symlinks are inappropriate.

Use `--` before positional arguments when the downstream CLI supports it.

### Minimize inherited behavior

Inspect:

- config files reachable through `accessPaths`;
- plugin and hook discovery;
- helper-selection environment variables;
- SSH agent or display use;
- subprocesses and shell expansion;
- error and verbose output.

A fixed top-level target is not enough when that target deliberately delegates
to caller-controlled helpers.

### Scope the credential

Use the narrowest service-side token that supports the action. underwrap
reduces direct token exposure; it cannot make an administrator token equivalent
to a repository-scoped token.

### Choose the simplest projection

- use env for a fixed token and an audited helper;
- use files for credential state that belongs in a directory tree;
- use libsecret for tools that genuinely expect Secret Service.

Do not choose a more complex projection merely because it sounds more secure.
The target's behavior determines which projection reduces exposure.

## Review sequence

1. Complete [Getting started](getting-started.md) with a stateless helper.
2. Read the [wrapper author checklist](wrapper-author-checklist.md).
3. Build one helper for one action.
4. Test malformed arguments and hostile environment/input cases.
5. Give the wrapper the narrowest downstream credential and backend permission.
6. Review logs and output for secret values.
7. Add broader or mutable workflows only when the use case requires them.

## Related documentation

- [Wrapper author checklist](wrapper-author-checklist.md)
- [Configuration](configuration.md)
- [Architecture](architecture.md)
- [Security](security.md)
