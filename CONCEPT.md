# underwrap concept

## Summary

`underwrap` is a generic wrapper-oriented secret brokerage system for local command execution.

Its core purpose is:

- let a user invoke a normal-looking command wrapper such as `gh-authorized`, `gws-castlabs`, or `agent-slack`
- allow that wrapper to obtain a secret or derived credential just in time
- inject the secret into the child process environment or stdin
- drop any elevated privilege before executing the target tool
- avoid exposing secrets to the normal interactive user account, shell startup files, or application config files

Unlike classic password managers or desktop keyrings, `underwrap` is designed first for **tool wrapping**, not for human retrieval. It should work on Linux in headless and desktop environments and should not depend on NixOS.

## Why this exists

Current patterns are unsatisfying:

- plain environment variables leak into shells, history, process environments, agent configs, or service files
- root-owned plaintext files plus `setgid` wrappers are simple, but awkward for mutable credentials such as OAuth refresh tokens
- desktop keyrings are user-centric and GUI-centric, not wrapper-centric
- secret managers are often too remote, too heavyweight, or too human-driven for local CLI execution
- mutable credentials often force ugly workflows like re-encrypting files, rebuilding systems, or manually copying secrets around

`underwrap` exists to make wrapped-command secret use and secret refresh a first-class local workflow.

## Design goals

1. **Wrapper-first UX**
   - A wrapped command should feel like a normal binary.
   - Example: `gws-castlabs calendar calendarList list`

2. **Keep secrets away from the normal user context**
   - no long-lived secret env vars in shells
   - no secrets in shell RC files
   - no secrets in normal app configs
   - no requirement that the interactive user be able to read stored secrets directly

3. **Just-in-time injection**
   - resolve secret as late as possible
   - inject only for the final process tree that needs it
   - support env injection, stdin injection, temporary file injection, and possibly header injection later

4. **Privilege drop before exec**
   - wrapper may use a narrow elevated capability to contact broker or open a restricted socket
   - wrapper must drop that privilege before `exec()` of the actual target

5. **Mutable secret support**
   - support overwriting OAuth refresh credentials and other rotating secrets without rebuilds or redeploys
   - support local reauth flows as first-class operations

6. **Policy and authorization**
   - not every wrapped command may read every secret
   - wrappers should be granted only specific secret access
   - write/update flows should be separate from read flows

7. **Portable architecture**
   - should work outside NixOS
   - should be packageable for Linux distributions in general
   - should not assume a particular init system, though systemd support should be excellent
   - should have a normal standalone runtime configuration format for non-Nix users

8. **First-class NixOS support**
   - should ship as a flake
   - should expose packages and NixOS modules
   - should make users/groups, sockets, services, wrappers, and policy generation easy to declare
   - should integrate cleanly with Home Manager where useful, without making Home Manager required

9. **Auditable and debuggable**
   - clear logs
   - explicit policy files
   - clear status and unlock inspection

## Non-goals

At least initially, `underwrap` is **not** trying to be:

- a full remote secret management platform
- a cloud KMS replacement
- a password manager UI
- a desktop autofill system
- a general PKCS#11 / SSH agent replacement
- a container secret orchestrator

## High-level idea

`underwrap` consists of four main pieces:

1. **Daemon**: `underwrapd`
   - runs under a dedicated system user
   - owns secret storage
   - enforces policy
   - performs secret reads, writes, derivations, refreshes, and unlock checks

2. **CLI control tool**: `underwrapctl`
   - admin and user-visible operations
   - unlock/lock/status
   - store/update/delete secret material
   - reauth and maintenance workflows

3. **Execution wrapper**: `underwrap-exec`
   - installed under different names or configured symlink names
   - requests a specific secret or derived credential from the daemon
   - injects it into env/stdin/tmpfile
   - drops privilege
   - executes target tool

4. **Policy + wrapper definitions**
   - declarative configuration describing wrappers, allowed secrets, injection mode, target binary, unlock requirements, and update permissions

## Core architecture

### 1. Daemon user

Create a dedicated local account, for example:

- user: `underwrap`
- group: `underwrap`

The daemon owns persistent state such as:

- encrypted secret store
- metadata store
- audit logs or event logs
- ephemeral unlock state
- temporary materialized files if needed

The normal interactive user should not own these files.

### 2. IPC

Use Unix domain sockets.

Suggested sockets:

- `/run/underwrap/read.sock` — wrapper read path
- `/run/underwrap/control.sock` — control and maintenance path

The daemon must authenticate callers using at least:

- `SO_PEERCRED` on Linux
- socket file permissions
- optional wrapper identity validation

The socket is the main security boundary instead of plaintext group-readable secret files.

### 3. Wrapper access model

The execution wrapper is the central UX element.

A wrapper definition might say:

- wrapper name: `gws-castlabs`
- target binary: `/usr/bin/gws`
- secret reference: `google/castlabs`
- injection mode: `env`
- env variable: `GOOGLE_WORKSPACE_CLI_TOKEN`
- derivation mode: `oauth_refresh_to_access_token`
- unlock policy: `ssh-agent-or-scope`

The user invokes `gws-castlabs ...`. The wrapper asks the daemon for the material needed for this wrapper. The daemon verifies policy and possibly unlock state, returns an access token or secret, and the wrapper injects it before exec.

### 4. Secret storage

This needs to support both static and mutable secrets.

Two categories:

#### Static secrets
Examples:

- API tokens
- webhook secrets
- long-lived service passwords
- client secrets

These may be provisioned by:

- import from file
- import from stdin via `underwrapctl store`

Integration with sops/age or cloud secret systems is a future possibility via the exec-based provider model, not an immediate goal.

#### Mutable secrets
Examples:

- OAuth authorized-user credentials JSON
- refresh tokens
- rolling session material
- locally reauthorized app credentials

These must support:

- atomic overwrite
- versioning or rollback metadata
- optional update restrictions per secret

## Storage encryption

`underwrap` should support encrypted-at-rest storage, but this needs careful framing.

### Important principle

Encrypted-at-rest is useful, but it is not magic. If the machine is already unlocked, the daemon eventually needs plaintext in memory to serve wrappers. The main value is:

- protection when the machine or disk is offline
- optional second unlock boundary after login
- better separation from generic file access

### Implemented storage model

The current implementation provides two built-in backends behind a `StorageBackend` trait:

1. **plain local backend**
   - plaintext files owned by daemon user
   - simplest, good default for most deployments

2. **encrypted local backend**
   - secrets encrypted with ChaCha20-Poly1305 under a local master key file
   - daemon decrypts on demand

The backend is abstracted behind a trait so the wrapper and daemon protocol do not care which backend is used.

### Future storage directions (not yet implemented)

Later, additional backends could be added via an exec-based provider model — the daemon spawns a configured command and reads secret material from its stdout. This would enable integration with external systems (Secret Service / libsecret, Vault, 1Password, cloud secret managers) without linking their libraries into the daemon. Built-in backends would stay minimal; external providers handle everything else.

Hardware-assisted unseal (TPM, YubiKey) for the encrypted backend's master key is another future possibility.

## Unlock model

Unlocking is an important part of the system and should be more expressive than simple file permissions.

### Required capabilities

- lock one secret, one wrapper, one namespace, or everything
- unlock for `N` uses
- unlock for `T` seconds
- unlock only in a scope/session
- support one-shot auth when no explicit unlock exists
- expose status

### Examples

```bash
underwrapctl unlock gws-castlabs --uses 5 --seconds 1800
underwrapctl status gws-castlabs
underwrapctl lock gws-castlabs
```

### Scopes

A scope is a logical execution context, for example a shell session.

Possible mechanism:

- env variable such as `UNDERWRAP_SCOPE`
- wrappers include scope in daemon requests
- daemon maintains unlock state per scope

This preserves the nice property of today’s scope-local unlock windows.

## Authentication methods for unlock and update

The daemon should support several unlock/update authorization methods:

1. **SSH agent challenge**
   - challenge signed by a configured key in `ssh-agent`
   - matches current workflow well

2. **YubiKey / hardware touch**
   - either through agent-based signing or direct plugin integration later

3. **local OS identity + policy**
   - useful for low-risk secrets or machine-local workflows

4. **admin policy**
   - for bootstrapping and recovery

These methods should be pluggable per wrapper or per secret.

## Read path workflow

### Static token example

Example wrapper: `gh-authorized`

1. user runs `gh-authorized pr list`
2. wrapper identifies itself as `gh-authorized`
3. wrapper connects to `underwrapd`
4. daemon verifies caller is allowed to request wrapper `gh-authorized`
5. daemon checks unlock policy
6. daemon loads secret `github/token`
7. daemon returns token material
8. wrapper sets `GH_TOKEN`
9. wrapper drops elevated capability/group
10. wrapper execs `gh`

### OAuth example

Example wrapper: `gws-castlabs`

1. user runs `gws-castlabs gmail users.labels.list ...`
2. wrapper requests credential for wrapper `gws-castlabs`
3. daemon loads stored authorized-user credentials JSON
4. daemon exchanges refresh token for fresh access token
5. daemon may persist updated credential state if Google returns changed token data
6. daemon returns short-lived access token only
7. wrapper injects `GOOGLE_WORKSPACE_CLI_TOKEN`
8. wrapper drops privilege
9. wrapper execs `gws`

This is cleaner than teaching every wrapper how to do OAuth refresh.

## Write/update workflows

This is one of the main reasons for the project.

### Reauth workflow for mutable OAuth credentials

Example: Google Workspace.

1. user runs something like:
   ```bash
   underwrapctl reauth google castlabs
   ```
2. control tool launches the plain vendor CLI or browser auth flow as the user
3. tool captures exported authorized-user credentials
4. tool sends update request to daemon over control socket
5. daemon validates policy:
   - caller may update only `google/castlabs`
   - update method allowed
   - optional user confirmation or auth challenge satisfied
6. daemon atomically replaces stored secret material
7. future wrapped invocations use new credentials immediately

### Why this matters

This makes refresh-token workflows possible without:

- rebuilding systems
- writing secrets to user config permanently
- storing mutable session-like credentials in Git
- requiring root ownership flips to the normal user

## Wrapper privilege model

One of the most important design choices.

### Current-style model

A wrapper can still use `setgid` to access a restricted socket or directory, then drop group before exec.

Benefits:

- narrow privilege
- no `setuid root`
- mirrors the existing proven model

This is a good default on Linux.

### Future alternatives

Possible future options:

- file capabilities if useful
- no wrapper privilege at all if daemon can authenticate caller sufficiently by wrapper path and user identity
- `bwrap` / namespace-based isolation for even tighter process containment

### Recommendation

First implementation should keep the current mental model:

- wrapper has just enough privilege to talk to the daemon
- wrapper receives secret
- wrapper drops privilege
- wrapper execs target

## Authorization model

This is where the daemon must be stronger than plain file permissions.

### Policy requirements

Policy should be able to express:

- wrapper `gh-authorized` may read only `github/token`
- wrapper `agent-slack` may read only `slack/xoxp`
- wrapper `gws-castlabs` may read/update only `google/castlabs`
- wrapper `gws-projektbuero` may read/update only `google/projektbuero`
- `underwrapctl reauth google castlabs` may update `google/castlabs`
- normal read wrappers may not perform writes

### Caller identity checks

The daemon should combine multiple checks:

- peer uid/gid from socket
- socket path permissions
- wrapper identity from configuration
- optionally, wrapper executable path hash or inode metadata in later versions

A key design challenge is avoiding a situation where any process with the socket group can pretend to be any wrapper. The daemon should not blindly trust a string provided by the client.

## Injection modes

Initial modes:

1. **env**
   - set one env var
   - good for most CLIs

2. **stdin**
   - pipe secret to process stdin

3. **tempfile**
   - materialize a temp file for the child process
   - unlink early when possible

4. **derived env**
   - daemon returns derived short-lived value rather than stored value directly
   - ideal for OAuth access tokens

Later possibilities:

- HTTP proxy/header mode
- file descriptor passing
- multi-secret templates

## Configuration model

`underwrap` should have a **portable runtime configuration format** that works without NixOS.

### Recommended runtime format

The default external configuration format should be **YAML**.

Reasons:

- good fit for policy-style configuration
- readable for operators
- handles nested wrapper/secret definitions well
- easy to generate from other systems

TOML could be considered later, but YAML is the best first-class operator-facing format.

### Configuration layering

The conceptual model should be independent from any one file format, but the initial implementation should likely load one or more YAML files that define:

- daemon settings
- storage backend settings
- wrappers
- secrets
- auth methods
- unlock policy
- update policy
- logging policy

A wrapper definition should capture at least:

- wrapper name
- target binary
- argv passthrough behavior
- secret reference
- derivation mode
- injection mode
- destination env var or file path
- unlock/auth requirements
- allowed update flows
- logging/audit policy

A secret definition should capture at least:

- secret id
- backend
- mutability
- allowed readers
- allowed updaters
- derivation type if any
- metadata like labels, owner, creation time, rotation time

### NixOS configuration story

NixOS should not replace the native YAML model; it should generate it.

That means:

- non-Nix users write YAML directly
- NixOS users declare wrappers/secrets/options as Nix attrs
- the NixOS module renders the canonical runtime YAML config for `underwrapd`

This preserves portability while still giving first-class NixOS ergonomics.

### Home Manager role

Home Manager support may be useful for:

- shell/session scope setup
- client-side helper commands
- completions
- optional per-user convenience config

But the daemon, sockets, storage, and policy model should remain system-level by default.

## Logging and audit

The daemon should log structured events such as:

- read request granted/denied
- update request granted/denied
- unlock/lock changes
- refresh-token exchange success/failure
- wrapper exec request metadata

But logs must never include the secret value.

Useful fields:

- timestamp
- wrapper id
- caller uid/gid
- secret id
- action
- result
- reason on denial

## Failure modes to plan for

- daemon unavailable
- socket permission problems
- unlock state expired
- auth challenge failed
- secret missing
- derivation failed (e.g. OAuth refresh failed)
- policy mismatch
- storage backend locked
- machine resumed and daemon lost key material

Wrappers should emit concise operator-friendly errors.

## Security properties sought

### Better than user env secrets

- secrets do not live in normal shell env by default
- secrets are injected only for the final child process
- wrappers can drop privilege before exec

### Better than static root-owned files for mutable creds

- mutable credentials can be updated without rebuilds
- update and read paths are separate
- daemon policy mediates access
- OAuth access token can be derived without exposing refresh material to wrapper code

### Remaining realities

- once the child process has the secret in env/stdin/tmpfile, that process can misuse it
- local root can always win
- a compromised daemon is high impact
- unlocked key material may reside in daemon memory

The goal is not perfection; it is a much better local security/UX tradeoff for wrapped command execution.

## Portability notes

Target platform first:

- Linux

Should support:

- headless servers
- desktop systems
- systemd and non-systemd setups where practical
- direct YAML-based configuration on non-Nix systems

Linux-specific features likely in first implementation:

- Unix sockets
- `SO_PEERCRED`
- `setgid`
- `/run` paths

Possible future portability:

- BSD/macOS may need different peer credential mechanisms and wrapper privilege model

Important portability principle:

- the core daemon and wrapper design must remain generic
- NixOS support should be a first-class integration layer, not the core architecture

## Packaging and repository shape

This project should live independently from any one machine configuration.

Suggested repository layout:

```text
underwrap/
  README.md
  CONCEPT.md
  flake.nix
  daemon/
  ctl/
  exec/
  protocol/
  docs/
  packaging/
  nix/
    modules/
    home-manager/
  examples/
```

Potential deliverables:

- `underwrapd`
- `underwrapctl`
- `underwrap-exec`
- sample policies
- sample systemd units
- packaging for generic Linux distributions
- a flake for Nix users
- NixOS modules
- optional Home Manager helpers/modules

### First-class NixOS support

The repository should ship a flake with first-class Nix outputs.

At a minimum, the flake should expose:

- `packages.<system>.underwrap`
- `packages.<system>.underwrapd`
- `packages.<system>.underwrapctl`
- `packages.<system>.underwrap-exec`
- `nixosModules.default`
- possibly more granular modules if that improves composition

The NixOS module should make it easy to declaratively manage:

- the daemon package
- the daemon service and sockets
- service user/group creation
- runtime directories and tmpfiles
- wrapper definitions
- policy generation
- optional secret source integrations
- shell/session scope helpers where appropriate

### NixOS module philosophy

The NixOS module should not invent a parallel conceptual model. It should map Nix options onto the same concepts used by the YAML runtime configuration and generate the runtime config from them.

This keeps documentation, behavior, and debugging aligned across:

- generic Linux installs
- NixOS module users
- future packaging systems

## Suggested implementation phases

### Phase 0: concept and protocol

- define terminology
- define policy model
- define socket protocol
- define wrapper identity model
- define threat model and assumptions

### Phase 1: minimal static secret broker

- daemon with Unix socket
- plain local storage backend
- policy file
- wrapper requests env secret
- wrapper drops privilege before exec
- no mutability yet

### Phase 2: unlock support

- scope handling
- time/uses-based unlock state
- `underwrapctl unlock/status/lock`
- SSH-agent challenge auth

### Phase 3: mutable secret updates

- control socket
- atomic update API
- audit logging
- secret metadata/versioning

### Phase 4: derived credentials

- OAuth refresh-to-access-token derivation in daemon
- optional token caching
- update-on-refresh if upstream rotates material

### Phase 5: encrypted backend ✅

- local encrypted store (ChaCha20-Poly1305)
- file master key backend
- TPM/YubiKey backed unseal deferred to future work

### Phase 6: polished integrations (in progress)

- examples for GitHub, Slack, Google Workspace
- NixOS module with VM tests
- generic Linux install docs — TODO
- package manager integration — TODO

### Future: pluggable storage providers (not yet planned)

- exec-based provider interface for external secret sources
- libsecret, Vault, 1Password, cloud secret managers — all via exec providers
- built-in backends stay minimal (plain + encrypted only)

## Open questions

1. How should wrapper identity be authenticated strongly enough to avoid spoofing by arbitrary group members?
2. Should secret reads always return plaintext to wrappers, or should some integrations happen fully inside daemon helpers?
3. Should update permissions be tied to user identity, wrapper identity, hardware auth, or a combination?
4. How much policy should live in files versus daemon state?
5. Should encrypted storage be mandatory or optional in the MVP?
6. Should the daemon itself perform OAuth/browser flows, or should `underwrapctl` orchestrate external tools and only send resulting credentials to daemon?
7. Should there be first-class support for importing from sops/age while staying distro-agnostic?

## Proposed first real implementation target

The first compelling implementation target should probably be:

- GitHub token wrapper
- Slack token wrapper
- Google Workspace OAuth wrapper with local reauth

This covers:

- static secret read
- mutable secret update
- derived access token generation
- wrapper env injection
- unlock workflow

If those work well, the architecture is likely sound.
