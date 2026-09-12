# Wrappers

Expose a narrow command, not a broad CLI with a hidden token.
Underwrap chooses a root-configured target after backend authorization and protects its projection from the untrusted local caller; the target defines what the caller can make that capability do.
The target can print, persist, or transmit its own secret, so a fixed executable path alone does not make a safe action boundary.
Read [Security](security.md) before exposing production credentials.

Prefer a capability such as:

```text
printf '%s\n' 'reviewed' | agent-gh-comment 123
```

rather than unrestricted `gh <anything>`.
The helper fixes the action; the downstream credential should independently limit its impact.

## Design a narrow helper

Fix the service, account or repository, subcommand, flags, and output shape wherever possible.
Give each caller-controlled input a grammar, size bound, encoding, and rejection behavior.
Reject unknown options and conflicting inputs, avoid shell evaluation and string-built commands, and separate positional arguments with `--` where the downstream CLI supports it.

Use a root-controlled Nix store path for the helper and absolute paths for every executable it can launch, including interpreters, Git, SSH, pagers, editors, browsers, plugins, and credential helpers.
If PATH is genuinely needed, construct a fixed non-secret `envSet.PATH` from reviewed packages.
The daemon fixes only the top-level target; caller-controlled configuration, hooks, extensions, or descendant selection can still widen behavior.

Prefer bounded stdin to a caller-selected file path.
A secret-bearing target may be able to read paths such as `/proc/self/environ`, so accepting an arbitrary readable path can turn a benign-looking upload or comment operation into disclosure.
Review symlink handling, allowed roots, regular-file requirements, body bounds before dispatch, option-shaped values, and error output.
Do not print, log, or put secret material in downstream argv, crash diagnostics, telemetry, or ordinary files.

### GitHub comment example

[`examples/agent-github-comment.nix`](../examples/agent-github-comment.nix) defines a fixed-repository PR comment helper.
The caller supplies one decimal PR number from 1 through 999999999 and at most 65,536 bytes of comment text on stdin.
The helper buffers at most one extra byte to reject oversize input, uses its target-owned runtime directory, registers cleanup for normal and handled-signal exits, and calls the Nix-pinned `gh pr comment` with fixed repository and flags.
The buffer is caller-supplied comment text, not a place for brokered resource values, and cleanup is not guaranteed after every possible termination.

Before using the example, supply the flake module argument as in [Getting started](getting-started.md#1-wire-the-flake-and-frontend-module), use an existing automation user, replace the fixed repository, configure valid backend TLS trust, and satisfy [host posture](configuration.md#host-posture).
Rebuild, generate and enroll the frontend identity, create or auto-create its resource and pair, and restart the daemon after enrollment.
Add the appropriately scoped credential through the resource env slot, not `envSet`, and save a non-writable permission for the exact pair.
Review the pinned CLI's configuration and behavior before treating the example as a production integration; it is a helper pattern, not proof of downstream service compatibility or target trustworthiness.

## Choose a projection

Use the simplest projection the target actually needs.
Every wrapper names one resource and must enable at least one projection; the resource can contain env, files, and libsecret together.

### Environment

Env suits a fixed token consumed by an audited stateless helper.
The target receives the raw value for its lifetime; the caller does not receive it as an exported shell variable.
Projected values must be UTF-8 without NUL, even though backend storage supports arbitrary bytes.
Env is operator-owned and runtime finalize rejects changes relative to its loaded snapshot, but whole-resource writes remain last-writer-wins rather than compare-and-swap.

The cleared environment is rebuilt from a fixed built-in passthrough, configured additions, trusted fixed values, and resource env in that precedence order for admissible names.
Built-ins include temporary-directory, SSH-agent, display, and proxy settings and cannot be removed per wrapper with `envPassthrough`.
They do not bypass DAC, but reachable paths or sockets can steer the target or provide disclosure routes.
A helper may explicitly neutralize unwanted behavior, but must not claim the Nix option removes built-ins.
HOME and five XDG names are forced; `LD_*` settings are forbidden by module policy, and caller PATH is not automatically inherited.
See the exact [target environment reference](configuration.md#target-environment).

### Files

Files suit applications that need a credential directory tree.
Configure only the required canonical caller-home-relative mount paths, and ensure every resource key routes strictly below exactly one prefix without path collisions.
Projection planning rejects invalid routes before effects, stages regular files on invocation-private capped `noswap` tmpfs, and rejects symlinks during collection.
Do not use files resources as generic application storage.

The target sees writable files regardless of backend writable permission.
A read-only permission discards changes rather than making the filesystem read-only, while the backend rechecks any submitted writeback.
Files mounts stay anchored to the caller's passwd home even if trusted configuration changes HOME.

### Libsecret

Libsecret suits a target that actually selects the Secret Service backend of its keyring API.
Underwrap supplies an invocation-private filesystem-backed D-Bus service, initializes it from the resource, and retains validated mutation generations in memory.
The bus tmpfs carries runtime socket/helper artifacts and has no `noswap` claim; resource state itself remains in process memory.

The target can read every item in the projected libsecret resource and can mutate its local service even with writable permission off.
Split occasional writable authentication from routine non-writable action wrappers, and lock the authentication wrapper when not needed.
An effective non-writable flag prevents underwrap persistence, not disclosure or local mutation.
Account for the backend [client-wide override](backend.md#client-wide-override) before relying on an individual row's writable flag or Locked mode.

A publication ACK means the host retained a validated invocation generation, not that the D-Bus caller received success or the backend persisted it.
A complete resource is limited to 2,097,152 bytes, while a private canonical libsecret publication body has its separate 1,048,576-byte cap.
A resource under the first cap can still fail service readiness under the second.
See [Local protocol](local-protocol.md) for initialization, publication, EOF requirements, and input validation; typed implementation evidence does not establish universal Secret Service client conformance.
Mount-namespace hiding does not hide abstract host D-Bus sockets from deliberately nonconforming clients.

### Host access is separate

Every `(instance, wrapper)` runs under a dedicated UID/GID, including two wrappers that use the same binary or resource.
HOME naming the caller's home does not grant traversal or access, and the caller's runtime directory is not automatically exposed.

Use `accessPaths` only for specific caller-owned directories the target needs.
One-pair idmapping exposes caller-owned contents and makes target writes caller-owned on the host; it is not backend-managed secret projection or a read-only grant.
The named filesystem must support idmapped mounts; unsupported nested submounts are skipped with a warning.
Review every exposed config, hook, plugin, key, socket, and write destination, and prefer a narrower directory to the whole home.
Do not add ordinary host credential caches or sidecar secret stores as a workaround for application discovery.
See [Configuration](configuration.md#files-and-explicit-host-access) for lexical overlap domains and mount behavior.

## Qualified GitHub libsecret pattern

The split-wrapper pattern below is an integration starting point, not a verified working recipe for the pinned GitHub CLI.
Its configuration discovery, secure-storage selection, and access prerequisites have not been established end to end against that pinned application.
Do not assume that setting `libsecret = true` makes `gh auth login` persist usable credentials for another wrapper.

```nix
# Fragment inside an already configured instance.
# ghPrHelper is a separately reviewed fixed-action helper, not supplied here.
wrappers = {
  gh-auth = {
    target = "${pkgs.gh}/bin/gh";
    resource = "github/gh";
    libsecret = true;
  };
  gh-pr = {
    target = "${ghPrHelper}/bin/gh-pr-helper";
    resource = "github/gh";
    libsecret = true;
  };
};
```

Before an authentication trial with disposable credentials, establish all of the following:

- The exact pinned CLI build selects Secret Service rather than another keyring backend or a plaintext-file fallback.
- Required non-secret host/account discovery files, their locations, and configuration precedence are understood for this version and login method.
- Both distinct target UIDs can discover the necessary state through deliberately configured projections or narrowly reviewed host access; sharing a backend resource does not share a Unix UID or automatically make one wrapper's host-written files readable by the other.
- No token-bearing fallback writes to ordinary host config, cache, logs, or temporary files; if credential files are genuinely needed, design an explicit backend files projection rather than an unreviewed host cache.
- The action helper fixes repository, subcommands, arguments, descendants, and acceptable output, and the credential has the least downstream privilege required.

Only after those prerequisites are resolved, enroll the runtime client, register both pairs, restart its daemon, and grant `gh-auth → github/gh` a short active writable permission.
A conditional test command is `gh-auth auth login`; exact arguments and interaction depend on the pinned CLI and chosen authentication method.
Never put a token on its command line, and do not assume a controlling terminal or browser integration is available merely because stdio is preserved.
Confirm the expected collection/item structure in **Secrets** without revealing its payload, then test the narrow `gh-pr` helper with a non-writable permission.
Return `gh-auth` to `Locked` after the task.
If the application succeeds but state does not persist, follow [Writeback diagnosis](operations.md#writeback-diagnosis) rather than using unsafe debug or weakening filesystem isolation.

The broad authentication target is itself a powerful capability while enabled, accessible through the system-wide entry to local callers authorized by the runtime tuple.
Use a reviewed OS-level gate when only an operator should invoke it; `allowedUsers` and backend permission rows do not implement per-user backend authorization.

## Author checklist

Before enabling a wrapper, be able to state its fixed target, action, argument grammar, projection, host access, descendants, resource, permission, credential scope, and known limitations in a short reviewable description.
If that description is hard to write, simplify the wrapper.

- Fix executable and action selection, including hooks, plugins, configuration, proxies, helpers, and other reachable underwrap entry points.
- Validate input sizes, encodings, paths, symlinks, ranges, unknown options, and conflicting flags before dispatch.
- Review normal, verbose, error, crash, telemetry, child-process, cache, and persistence paths for secret disclosure.
- Keep brokered material out of `envSet`, argv, caller environments, ordinary temporary files, and host caches.
- Accept the non-removable environment built-ins explicitly, and document any helper-level neutralization without attributing it to module policy.
- Justify every host directory and every writable permission separately from the target's ability to disclose its projection.
- Scope downstream credentials independently and define their rotation and revocation route.
- Check shim-name collisions across instances and debug names; do not confuse shim installation with local-user authorization.
- Prefer time-bounded real access where practical, and use [unsafe debug](operations.md#unsafe-debug) only for deliberate short incident access with full disclosure understood.
- Serialize shared-resource writers and identify how persistence is checked without exposing values or confusing target status with finalize success.

Test the intended action plus missing, malformed, oversized, option-shaped, hostile-path, symlink, environment, locked, expired, and missing-permission cases.
Exercise read-only mutation and target exit/signal handling, and verify output and selected service logs without printing actual secret material.
NixOS module, identity, namespace, capability, mount, peer-authentication, or wrapper-chain changes require the VM check in [Testing](testing.md).
These checks witness selected behavior, not target trustworthiness, universal client compatibility, or complete absence of disclosure.
