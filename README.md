# underwrap

Run local CLI tools with secrets injected just in time — without handing those reusable secrets directly to the caller.

> [!WARNING]
> **Early project / security review status:** underwrap is still young. Large parts of the current implementation were built quickly with heavy agent assistance (yes, "vibe coded"), and a deeper security review is still pending.
>
> The project already aims to make accidental secret extraction and casual same-user snooping much harder, especially in agent-facing workflows. It also **strives** to hold up under a more adversarial agent threat model, and many design choices are driven by that goal. But that stronger claim should still be treated as an aspiration under active scrutiny, not as a finished security certification.
>
> If you spot a weakness, questionable assumption, or misleading claim, feedback is very welcome.

**NixOS only.** underwrap relies on NixOS features such as `security.wrappers`, declarative users and groups, file capabilities, and a NixOS module that generates the wrapper chain and daemon configuration. `underwrap-backendd` is the backend service for both local and remote deployments; in remote setups it can also run on other Linux machines.

## Why this exists

underwrap is for a specific problem:

- you want to run an existing CLI such as `gh`, `jira`, or `gws`
- that CLI needs a token or credential
- you do **not** want that secret sitting in the caller's shell environment, config files, or long-lived dotfiles
- ideally, you also want to keep secrets off the target machine entirely

The most opinionated use case is:

> **agent-facing wrappers without handing out reusable tokens**

For example, instead of giving an agent `GH_TOKEN`, you expose a narrow command such as:

```text
agent-gh-comment <pr-number> <body-file>
```

underwrap can inject the token only for that wrapper invocation, while the target runs through a hardened wrapper chain instead of from the agent's ambient shell environment.

## What underwrap does

At a high level:

1. a setgid wrapper connects to its dedicated wrapper socket
2. the daemon selects the expected wrapper from that socket
3. the daemon confirms the peer's effective GID matches that wrapper
4. the daemon fetches or derives the secret
5. the wrapper drops privilege
6. the target runs with the secret injected just for that invocation

Depending on how you configure it, underwrap can also:

- keep secrets in a central `underwrap-backendd` service on the same
  machine or on a trusted remote workstation/server
- let operators manage stores, users, and per-`(wrapper, host, user)`
  agent grants through a single-binary admin SPA that
  `underwrap-backendd` serves directly over HTTPS
- require a fresh SSH-agent signature for individual invocations of
  high-privilege wrappers (per-call approval)
- provide a private Secret Service (`libsecret`) environment for tools
  such as `gh`

## What makes it interesting

underwrap is trying to combine a few things that are rarely available together:

- **wrapper-first UX** — users run a command like `work-jira issue list`
- **kernel-backed wrapper identity** — wrapper socket selection sets the expected wrapper, and the peer's setgid-derived effective GID must confirm it
- **per-agent `(wrapper, host, user)` grants** — central policy
  controls which agents on which hosts can use each wrapper, with
  four modes per grant (locked, unlocked, time-limited, ssh-agent
  per-call approval) and a single global write-lock toggle per store
- **clean target environments** — the target does not inherit the caller's full shell environment
- **process-isolated delivery** — under the default hardened posture
  (`services.underwrap.hardenTargetHost = true`, which pins
  `kernel.yama.ptrace_scope=2`), no non-root same-UID process — including
  the caller — has a known kernel-mediated path to read the wrapper or
  target's environ, memory, or file descriptors. What the target program
  itself does with the secret (its output streams, its own file writes,
  its own network behavior) remains the responsibility of the wrapper
  author; libsecret and directory-credential modes exist for targets
  where that responsibility cannot be met
- **mutable credential workflows** — useful for libsecret-backed tools and app-managed state
- **central-backend deployment** — secrets can stay on your workstation
  or a central server, reachable only via authenticated mTLS

For the design details and limitations, see [Security](docs/security.md).

## What it does not do

underwrap is **not** a full sandbox and it is **not** a policy engine for broad CLIs.

Important boundaries:

- If you wrap a broad tool directly, you are still giving the caller broad access to that tool.
- If the target program itself can reveal the secret, underwrap cannot magically prevent that.
- If the target resolves broad helper behavior that you did not review, underwrap does not sandbox that descendant behavior for you.
- If the machine is compromised as `root`, the game is over.
- The current design leans on Linux and NixOS behavior, especially Yama and the wrapper/capability setup.

A good short way to think about underwrap is:

> it tries to make "use the secret through this intended command" easier than "extract the raw secret value"

That is valuable, but it is not the same thing as making arbitrary local execution safe.

That is why the recommended pattern for agent-facing use is usually:

```text
underwrap wrapper -> your narrow helper -> real CLI
```

not:

```text
underwrap wrapper -> unrestricted general-purpose CLI
```

See [Agent-facing wrappers](docs/agents.md).

## A quick example

Imagine a NixOS machine where you want a Jira wrapper that injects `JIRA_API_TOKEN` only when `jira` runs.

After configuration, the user experience looks like this:

```bash
$ work-jira issue list
```

Instead of this:

```bash
$ export JIRA_API_TOKEN=...
$ jira issue list
```

That means the token does not need to live in the shell session. underwrap fetches it when the wrapper runs, injects it for the target process, and then the wrapper exits.

The full first-setup walkthrough is in [Getting started](docs/getting-started.md).

## Typical workflows

underwrap is most useful when you want one of these patterns:

- **A simple token-backed wrapper**
  - Example: `jira-cli` using `JIRA_API_TOKEN`
  - Good first setup and easiest mental model

- **A narrow agent-facing command**
  - Example: `agent-gh-comment`
  - The caller can perform one approved action without receiving the reusable token

- **A libsecret-backed CLI inside a private session**
  - Example: `gh auth login` storing credentials through underwrap's private Secret Service

- **A central-backend deployment**
  - Secrets live on your workstation or a central server; target
    machines reach them through authenticated HTTPS with mTLS

## Should you use underwrap?

underwrap is a good fit when:

- you are on NixOS
- you want to keep using existing CLIs instead of writing a broker service
- you care about not spraying secrets into shell environments and dotfiles
- you want a strong story for agent-facing wrappers
- you need mutable credentials, libsecret support, or remote secret storage

It is probably **not** the right tool when:

- plain environment variables are already good enough
- a static secret file is acceptable
- you mostly need behavior restriction, not hardened secret delivery
- you are not on NixOS

For a short decision guide, see [Choosing underwrap](docs/comparison.md).

## Quick start

```bash
nix develop
cargo build
nix flake check
```

If you want the manual layered equivalent instead of the flake gate, run:

```bash
cargo test --workspace
cargo fmt -- --check
cargo clippy --all-targets --workspace -- -D warnings
nix build .#checks.x86_64-linux.vm-test
```

Then start with [Getting started](docs/getting-started.md).

## Documentation

- [Documentation overview](docs/index.md)
- [Getting started](docs/getting-started.md)
- [The underwrap backend](docs/backend.md)
- [Agent-facing wrappers](docs/agents.md)
- [GitHub via libsecret](docs/gh-libsecret.md)
- [Configuration reference](docs/configuration.md)
- [Architecture](docs/architecture.md)
- [Deploying the central backend](docs/remote-backend.md)
- [Security](docs/security.md)
- [Testing](docs/testing.md)
- [Choosing underwrap](docs/comparison.md)
- [TODO](TODO.md)

## Status

The implementation covers the core wrapper chain, multi-instance NixOS
deployment, the central HTTPS backend with mTLS, the operator admin UI,
libsecret mode, and directory credential mode.

What it still needs most is:

- wider real-world usage
- simplification and polish
- deeper security review
- continued pressure-testing of the agent-facing threat model

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
