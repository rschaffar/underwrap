# Choosing underwrap

underwrap is not a general secret-management replacement. It is a focused tool for a particular workflow:

> running local CLI commands with secrets injected just in time, especially when the caller should not simply be handed the reusable token

If that is your problem, underwrap may be a good fit. If it is not, simpler tools are often better.

## Good fit

underwrap is a strong candidate when most of these are true:

- you are on **NixOS**
- you want to keep using existing CLIs such as `jira`, `gh`, or `gws`
- you care about not exporting secrets broadly into shell sessions
- you want a cleaner story for **agent-facing wrappers**
- you may need mutable credentials, libsecret support, or a remote backend

## Probably not a fit

underwrap is probably the wrong tool when:

- a normal environment variable is already good enough
- a plain secret file is acceptable
- you mostly need to restrict behavior, not improve secret delivery
- you are not on NixOS
- you would be better served by a small purpose-built broker service

## Compared with common alternatives

### Plain environment variables

Simple and often sufficient, but the secret becomes ambient to the shell session and its child processes.

underwrap is trying to reduce that ambient spread by delivering the secret at invocation time instead.

### `direnv`, dotenv, and similar shell-scoped tools

Useful for developer ergonomics, but still fundamentally shell-environment solutions.

underwrap is more opinionated and more operationally heavy, but it is solving a different problem.

### `sops-nix`, agenix, and encrypted files on disk

These are great when you want declarative secret files for services.

underwrap is more interesting when the consumer is an interactive or scripted CLI process and the credential may be mutable or short-lived.

### Password-manager CLIs such as `op` or `bw`

These are good secret retrieval tools.

underwrap is different because it is centered on **wrappers and target processes**, not on giving the caller a secret string to do whatever it wants with.

### Vault and cloud secret managers

Those are broader systems with stronger central-management stories.

underwrap is much narrower: local CLI workflows, NixOS deployment, and process-oriented secret delivery. Sometimes that narrowness is a feature.

## The most opinionated reason to use underwrap

The clearest argument for underwrap is this:

- you have an agent or automation runner
- you want it to call a specific local CLI-backed capability
- you do not want to hand it the reusable token directly

That is the use case where underwrap feels most distinct.

See [Agent-facing wrappers](agents.md).

## The main trade-off

underwrap gives you a more deliberate secret-delivery path, but it also asks more of you:

- NixOS-specific deployment
- wrapper design discipline
- a willingness to understand at least some of the security model

If you do not need those properties, simpler tools will usually be easier to live with.

## Related docs

- [Getting started](getting-started.md)
- [Agent-facing wrappers](agents.md)
- [Security](security.md)
