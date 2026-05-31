# Wrapper author checklist

This checklist is for designing a wrapper that should be good enough for underwrap's first release.

It is intentionally opinionated.

The goal is not merely to “make it work”. The goal is to produce a wrapper that has a clean trust boundary, looks good in a first release, and does not accidentally widen the secret exposure surface.

## 1. Is the wrapped action narrow enough?

Prefer a narrow helper over a broad CLI.

Better:

```text
agent-gh-comment <pr> <body-file>
```

Worse:

```text
gh <anything>
```

Ask:

- is the action surface intentionally small?
- are the allowed subcommands fixed?
- are the allowed flags fixed or tightly validated?
- does the token scope match that action surface?

If the answer is “this wrapper can do almost anything the token can do”, the wrapper is probably too broad for an agent-facing use case.

## 2. Is the top-level target path absolute and trusted?

The target should be a fixed absolute path from trusted configuration.

Ask:

- is the wrapper target an absolute path?
- is it a store path or another root-controlled path?
- can the caller swap it out indirectly?

The whole point is to distinguish:

- “run this approved helper”
- from
- “run an arbitrary caller-chosen binary with my secret”

## 3. Does the target or helper rely on PATH lookups?

By default, underwrap does **not** inherit `PATH`.

That is deliberate.

Ask:

- does the target shell out to `git`, `ssh`, `less`, `xdg-open`, or similar by bare name?
- can I replace those lookups with absolute paths?
- if not, can I set a trusted fixed `PATH` via `envSet.PATH`?

Prefer:

- absolute Nix store paths
- or an explicit trusted `envSet.PATH`

Avoid:

- caller-controlled PATH
- hidden helper lookup assumptions

## 4. Are helper-selection environment variables controlled?

Treat these as risky helper-selection surfaces:

- `EDITOR`
- `VISUAL`
- `PAGER`
- `BROWSER`

Ask:

- does the target honor any of these?
- would a malicious helper inherit the secret-bearing environment?
- should the wrapper set a fixed trusted value instead of passing through the caller's value?

Prefer `envSet` over `envPassthrough` for these if they are needed at all.

## 5. Is `envPassthrough` really necessary?

Every passthrough variable is a trust decision.

Ask for each one:

- why is this needed?
- what behavior of the target does it influence?
- could it select helper binaries, config files, sockets, or plugins?
- would a fixed trusted `envSet` value be cleaner?

Also note:

- `HOME`, `XDG_RUNTIME_DIR`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`,
  `XDG_CACHE_HOME`, and `XDG_STATE_HOME` are not passthrough knobs in
  underwrap's hardened model
- use `envSet` only for intentional `HOME` / base `XDG_*` overrides, and keep those override values absolute
- use the instance-level `xdgRuntimeDirBase` knob for `XDG_RUNTIME_DIR`

If you cannot explain why a passthrough is safe enough, do not add it.

## 6. Are all free-form arguments validated?

If your helper accepts caller-controlled arguments, validate them as if the helper were an API.

Examples:

- PR numbers should be numeric
- repo names should be fixed or validated
- file paths should be bounded and explicitly allowed
- message sizes should be capped
- flag subsets should be explicit

The helper should not quietly devolve into “basically a shell around the real CLI”.

## 7. Are secret values kept out of logs and diagnostics?

Ask:

- could the helper print the secret accidentally?
- could it dump request bodies containing the secret?
- could verbose/debug output leak headers or tokens?

Log identifiers, paths, and wrapper names.
Do **not** log secret material.

## 8. Does the wrapper need libsecret mode or directory mode?

Use libsecret mode when the target genuinely expects Secret Service behavior, such as:

- `gh auth login`
- tools that store/retrieve credentials via libsecret

Use directory mode when the target persists backend-managed credential state in files under the user's home view, such as:

- config directories containing tokens or refresh state
- tools that expect credential files under `.config/` or `.local/share/`

Do **not** use libsecret mode just because it sounds more advanced.
Do **not** use directory mode as a catch-all for random writable app state.

Ask:

- does the target need Secret Service semantics?
- does it actually persist credential material in a directory tree?
- or is a simple env-injected secret enough?

The simpler mode is usually better unless the workflow really needs libsecret or directory-backed state.

## 9. How tightly should access be time-bounded?

This is a backend-side question, not a local one. underwrap does not
ship a per-invocation prompt; if you need human-in-the-loop confirmation
on every call, layer `sudo`/polkit in front of the wrapper.

Ask:

- is standing `Unlocked` access acceptable, or should this wrapper be
  `Locked` by default and flipped to `UnlockedUntil { until_epoch }` for
  time-bounded windows?
- if an operator needs to dig into the projected secret to debug an
  incident, `UnsafeDebugUntil` enables the debug tool — a full
  interactive shell/exec session with **read-write** access to the
  secret (not a read-only viewer: a `debug` run can write the secret
  back, gated only by `writable`). Use the shortest TTL that fits.

Set wrapper permission modes in the [admin UI](admin-ui.md).

## 10. Is the token itself scoped tightly enough?

underwrap improves secret delivery. It does **not** make an overpowered token safe.

Ask:

- is the downstream token the narrowest one available?
- can it be limited to one repo / one service / one action class?
- is a mutable app-managed credential state more appropriate than a long-lived static token?

A good wrapper with a bad token scope is still a bad overall design.

## 11. Are you relying on broad descendant behavior you haven't reviewed?

Even with a fixed top-level target, descendant helper execution matters.

Ask:

- what child processes can this target spawn?
- what hooks/plugins/config files can affect it?
- what files or sockets does it trust?

underwrap is not a general sandbox. If the target is too behaviorally broad, a narrow helper is often the better answer.

## 12. Does the wrapper still look good as part of a first release?

This project is greenfield. Backwards compatibility is not the priority. A polished first release is.

Ask:

- is the wrapper design clean?
- is the trust boundary obvious?
- would a new reader understand why this wrapper exists?
- is there a simpler, tighter design now that should replace the current one?

If a better design is clear and does not weaken security, prefer the better design.

## A good default mindset

A wrapper should ideally read like a security boundary description:

- fixed target
- fixed helper set
- explicit argument surface
- explicit environment policy
- token scope matched to intent

If it instead feels like a convenience shim around a broad CLI, keep tightening it.

## Related docs

- [Getting started](getting-started.md)
- [Configuration reference](configuration.md)
- [Agent-facing wrappers](agents.md)
- [Security](security.md)
