# Agent-facing wrappers

This guide describes the main underwrap use case: letting an AI agent or other
automation invoke a CLI capability **without** handing it the reusable API
token directly.

## The problem

A common agent setup already allows the agent to:

- read the workspace
- write temporary files
- invoke a small set of approved commands

In that setup, the main remaining problem is often **credential exposure**:

- you do not want `GH_TOKEN`, `SLACK_TOKEN`, or similar secrets in the agent's
  shell environment
- you do not want tokens written to local files on the target machine
- you do not want the agent to be able to trivially print or reuse the raw
  token outside the intended tool flow

underwrap is designed to be the hardened secret-delivery layer for that case.

## Recommended pattern

The strongest pattern is:

```text
agent
  -> /run/wrappers/bin/agent-gh-comment     (setgid underwrap entry wrapper)
  -> /nix/store/.../bin/agent-gh-comment    (your narrow wrapper)
  -> /nix/store/.../bin/gh                  (or another fixed tool)
```

That is:

1. The agent invokes a wrapper command that NixOS deployed declaratively.
2. underwrap authenticates the wrapper by its kernel-visible effective GID.
3. underwrap injects the secret just-in-time.
4. underwrap executes your **fixed target** from a root-owned sidecar config.
5. Your target should itself be a **narrow wrapper** that limits what the agent
   can do.
6. That wrapper may call `gh`, `slack-cli`, or another tool via **store-pinned
   absolute paths**.

Example narrow commands:

- `agent-gh-comment <pr> <body-file>`
- `agent-gh-list-open-prs`
- `agent-slack-post <channel> <body-file>`
- `agent-gws-read-doc <doc-id>`

This is much safer than exposing a broad tool like `gh <anything>` directly.

## What underwrap gives you here

When used this way, underwrap provides several useful hardening properties.

### The agent does not need the raw token

The wrapper can use the secret-backed capability without the raw token being:

- exported in the agent shell
- written to a config file
- stored on target disk in remote-backend mode
- exposed as a normal argument

This is the main value for agent-facing use.

### The configured target is fixed

The target binary path comes from a trusted root-owned sidecar config in
`/etc/underwrap/wrappers/`.

- The target path must be **absolute**.
- underwrap does **not** search `PATH` for the configured main target.
- A caller cannot redirect the wrapper to some attacker-chosen binary.

For agent-facing wrappers, this is what lets you point underwrap at a Nix-store
helper you wrote yourself.

### The target starts from a clean environment

underwrap starts the target with `env_clear()` and then restores only:

- a safe default whitelist (`HOME`, `USER`, `PATH`, locale, XDG vars, etc.)
- trusted per-wrapper additions from the sidecar config (`envPassthrough`,
  `envSet` in the NixOS module)

This blocks:

- `LD_PRELOAD` / `LD_LIBRARY_PATH` injection
- user injection of internal `UNDERWRAP_*` control variables
- arbitrary inherited environment affecting the target

If your wrapper should not rely on `PATH`, call downstream tools by absolute
Nix store path.

### Wrapper identity is kernel-verified

The daemon does not trust a client-supplied wrapper name. It derives wrapper
identity from the wrapper's effective GID.

That means one wrapper cannot simply claim to be another wrapper and read its
secret.

### Wrapper parent processes are hardened

The wrapper processes that briefly hold secret material:

- set `PR_SET_DUMPABLE=0`
- zeroize secret buffers after handoff
- avoid argv/temp-file secret passing
- disable core dumps for the target process

### Secrets can stay off the target machine

With the remote backend, the target NixOS machine receives secrets over an
SSH-tunneled Unix socket from your workstation. This is especially useful for
agent VMs or ephemeral hosts.

## What underwrap does NOT do

underwrap is not a full behavior sandbox.

It does **not** by itself guarantee that the target tool is narrowly scoped.
If you wrap a broad tool directly, you are still giving the caller broad tool
capabilities.

Examples:

- Wrapping full `gh` still gives the caller full `gh` powers.
- Wrapping full `slack-cli` still gives the caller whatever that CLI can do.
- If the target tool itself can reveal the token, underwrap cannot stop that.

So the recommended design is:

- use **narrow wrappers** for agent-facing commands
- use **scoped tokens** where possible
- pin downstream tools to **absolute Nix store paths**
- keep the wrapper interface as small and auditable as possible

## When this is a good fit

Use this pattern when:

- the agent already has workspace read access
- the problem is protecting the **reusable credential**, not hiding the local
  files from the agent
- you want to keep existing CLIs in the loop instead of building a custom API
  broker for everything
- you want the wrapper commands deployed declaratively by NixOS

If you control the whole integration and can avoid exposing a general CLI at
all, a narrow broker service may still be simpler. underwrap is most useful
when you want hardened secret delivery around real local CLI execution.

## NixOS example

See [`examples/agent-github-comment.nix`](../examples/agent-github-comment.nix)
for a complete example.

That example shows this pattern:

- create a small Nix-store wrapper `agent-gh-comment`
- hardcode the allowed GitHub action (`gh pr comment`)
- point underwrap at that wrapper as the target
- inject `GH_TOKEN` only for that wrapper invocation

## Practical guidance

For agent-facing wrappers, prefer:

- `agent-gh-comment`
- `agent-gh-list-open-prs`
- `agent-slack-post`

Over:

- `agent-gh <anything>`
- `agent-slack <anything>`

The narrower the wrapper, the more meaningful underwrap's secret-delivery
hardening becomes.
