# Wrapper author checklist

Use this checklist before exposing a wrapper to humans, automation, or agents.
The goal is an explicit, reviewable capability boundary.

## 1. Is the action narrow enough?

Prefer one named action:

```text
printf '%s\n' 'reviewed' | agent-gh-comment <pr>
```

over a broad passthrough:

```text
gh <anything>
```

Check:

- subcommands and verbs are fixed;
- flags are fixed or explicitly enumerated;
- the service/repository/account scope is fixed where possible;
- the token scope matches the action;
- callers cannot turn the helper into a general shell or transport.

## 2. Is the target absolute and trusted?

`target` must be an absolute root-controlled path. Prefer a Nix store path.

Check:

- the daemon config names the intended helper, not a mutable user path;
- the target is not selected through caller input, symlinks, `PATH`, or a
  helper environment variable;
- the helper package and source are part of the review.

The daemon protects only the top-level target choice. The helper remains
responsible for descendants.

## 3. Are downstream executables pinned?

Inspect every subprocess:

- `git`, `ssh`, `gh`, `curl`, `jq`, shells;
- pagers, editors, browsers;
- credential helpers and plugins.

Use absolute paths. If a target genuinely needs `PATH`, set a fixed trusted
`envSet.PATH` built from the exact packages it requires.

Do not pass through caller `PATH`.

## 4. Are helper-selection variables controlled?

Treat these as executable-selection or behavior surfaces:

- `EDITOR`, `VISUAL`, `PAGER`, `BROWSER`;
- language/plugin paths;
- Git/SSH helper configuration;
- service-specific hook variables.

Prefer a fixed `envSet` value or absence. Do not add a variable to
`envPassthrough` without explaining what it can select.

## 5. Is the built-in environment acceptable?

The built-in passthrough set includes locale/terminal values and
caller-controlled values such as:

- `TMPDIR`;
- `SSH_AUTH_SOCK`;
- `DISPLAY` and `WAYLAND_DISPLAY`;
- HTTP/HTTPS proxy variables.

`envPassthrough` adds names; it cannot remove built-ins per wrapper in this
release. Passing a path does not bypass DAC, but a reachable path/socket can
steer behavior or provide an exfiltration channel.

If a helper's security claim requires the absence of one of these variables,
the current wrapper environment model does not provide that guarantee. Do not
document a stronger boundary than the implementation supplies.

## 6. Are arguments parsed like an API?

For every caller-controlled value, define:

- grammar;
- maximum size;
- accepted range or enum;
- path root and symlink policy;
- encoding;
- duplicate/conflicting flag behavior.

Examples:

- PR numbers are decimal and bounded;
- repository names are fixed or strictly parsed;
- stdin/request bodies are size-limited before dispatch;
- accepted files are regular, readable, size-limited, symlink-safe, and under an
  allowed root;
- a secret-bearing helper never accepts paths such as `/proc/self/environ`;
- unknown options are rejected;
- `--` separates options from positional values.

Avoid shell evaluation and string-built commands.

## 7. Can the target expose the secret?

Check output and persistence paths:

- normal stdout/stderr;
- verbose/debug modes;
- HTTP request/response dumps;
- crash reports and core dumps;
- history, cache, config, and log files;
- telemetry and tracing;
- child-process environments.

underwrap sets `RLIMIT_CORE=0` and isolates process state, but it cannot stop the
target from deliberately printing or writing its own projection.

Log identifiers and decisions, never secret values.

## 8. Is the projection mode appropriate?

### Env

Use for a fixed value consumed directly by an audited helper. The raw value
lives in the target environment for its lifetime.

### Files

Use when the application manages credential material as files. Configure only
the required clean relative mount paths. Do not use the files slot as generic
application storage.

### Libsecret

Use when the application expects Secret Service or a compatible keyring API.
Separate writable authentication wrappers from read-only action wrappers.

Choose the simplest mode that matches the target. Complexity adds state and
input surface.

## 9. Is host access explicit?

The target runs under a dedicated UID. `HOME` names the caller's passwd home,
but the target does not automatically gain access to it.

For every `accessPaths` entry, check:

- why the target needs it;
- whether a narrower subdirectory works;
- which config, hooks, plugins, keys, or sockets become reachable;
- whether target writes back to caller-owned state are acceptable;
- whether the filesystem supports idmapped mounts.

`accessPaths` exposes host state; it is not a backend-managed secret projection.

## 10. Is backend policy minimal?

Each permission row is keyed by runtime client instance, wrapper, and resource.
Check:

- standing `Unlocked` access is actually required;
- `UnlockedUntil` can bound the window;
- the writable flag is off unless files/libsecret changes must persist;
- the wrapper maps to the intended resource;
- stale permission rows and pairs are removed.

`UnsafeDebugUntil` permits an env summary, interactive shell, or arbitrary exec
inside the wrapper namespace. It can persist changes when writable is enabled.
Use the shortest practical TTL and disable it immediately afterward.

Per-invocation approval is not built in. Use sudo or polkit for a human prompt.

## 11. Is the downstream credential narrow?

Check service-side scope independently from wrapper shape:

- repository/project/account scope;
- read versus write permissions;
- endpoint/action restrictions;
- lifetime and rotation;
- revocation procedure.

A narrow helper with an administrator token still carries administrator-level
impact if the helper or downstream CLI can be abused.

## 12. Is the local-user model understood?

`allowedUsers` installs PATH shims only. It does not prevent another local user
from executing `/run/wrappers/bin/underwrap-<instance>` directly. Backend policy
identifies the frontend runtime client instance, not the Unix caller.

Check whether the host needs:

- sudo rules;
- polkit;
- a separate frontend instance/host;
- another OS-level local-user gate.

Do not describe the shim list as authorization.

## 13. Have descendant behaviors been reviewed?

List what the target can load or execute:

- plugins and extensions;
- Git hooks and credential helpers;
- shell startup files;
- interpreters and config-driven commands;
- network proxies;
- files exposed by `accessPaths`;
- other underwrap entry points.

A fixed target path does not make configurable descendant behavior fixed.

## 14. Is the boundary easy to explain?

A review-ready wrapper should have a short description containing:

- fixed target;
- allowed action and arguments;
- projection mode;
- explicit host access;
- fixed helper set/environment;
- backend resource and permission mode;
- downstream credential scope;
- known limitations.

If that description is difficult to write, simplify the wrapper before relying
on it.

## Validation

At minimum, test:

- successful intended actions;
- missing, malformed, oversized, and option-shaped arguments;
- hostile path and symlink cases;
- caller-controlled environment values;
- locked, expired, and missing permissions;
- read-only write attempts;
- absence of secret values in stdout, stderr, and service logs;
- target exit and signal propagation.

NixOS wrapper-boundary changes require the VM test described in
[Testing](testing.md).

## Related documentation

- [Agent-facing wrappers](agent-facing-wrappers.md)
- [Configuration](configuration.md)
- [Security](security.md)
- [Admin UI](admin-ui.md)
