# Documentation overview

If you are new to underwrap, start here.

underwrap's docs are organized by question:

- **Should I use this?** → [Choosing underwrap](comparison.md)
- **How do I get one wrapper working?** → [Getting started](getting-started.md)
- **How should I design agent-facing wrappers?** → [Agent-facing wrappers](agents.md)
- **What should I check before I call a wrapper design done?** → [Wrapper author checklist](wrapper-author-checklist.md)
- **How do the advanced workflows work?**
  - [GitHub via libsecret](gh-libsecret.md)
  - [Google Workspace via OAuth derivation](gws-oauth.md)
  - [Remote backend](remote-backend.md)
- **How do I configure it in NixOS?** → [Configuration reference](configuration.md)
- **How does the system fit together?** → [Architecture](architecture.md)
- **What are the security goals, mechanisms, and limits?** → [Security](security.md)
- **How is it tested?** → [Testing](testing.md)

## Recommended reading path

### If you are evaluating underwrap

1. [README](../README.md)
2. [Choosing underwrap](comparison.md)
3. [Getting started](getting-started.md)
4. [Security](security.md)

### If you want the agent-facing pattern

1. [README](../README.md)
2. [Agent-facing wrappers](agents.md)
3. [Wrapper author checklist](wrapper-author-checklist.md)
4. [Getting started](getting-started.md)
5. [Security](security.md)

### If you want to deploy it seriously

1. [Getting started](getting-started.md)
2. [Configuration reference](configuration.md)
3. [Remote backend](remote-backend.md) if relevant
4. [Testing](testing.md)
5. [Security](security.md)

### If you are reviewing the security model

1. [Architecture](architecture.md)
2. [Security](security.md)
3. [Testing](testing.md)

## The three main usage levels

### 1. Simple token injection

This is the easiest way to adopt underwrap.

Example:
- wrap `jira-cli`
- inject `JIRA_API_TOKEN`
- keep the token out of the caller's shell session

Start here if you mainly want to stop exporting secrets globally.

### 2. Narrow agent-facing wrappers

This is the most opinionated use case.

Example:
- expose `agent-gh-comment`
- inject `GH_TOKEN` only for that one command
- point underwrap at your own narrow helper instead of the full `gh` CLI

This is where underwrap is most compelling.

### 3. Stateful or derived credential workflows

This is where underwrap moves beyond simple env-var injection.

Examples:
- `gh` storing credentials via a private Secret Service in libsecret mode
- `gws` using stored refresh-token material to derive short-lived access tokens
- remote backend mode where secrets remain on your workstation

## A note on the security docs

The [Security](security.md) document is intentionally detailed and technical.

That is by design. underwrap is trying to make strong guarantees about secret handling in a tricky part of the stack: local CLI execution on multi-process systems. The high-level docs keep things approachable; the security doc explains the reasoning in depth.
