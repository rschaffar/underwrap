# Documentation

underwrap's documentation is organized by task and audience.

## Evaluate underwrap

- [Project overview](../README.md) — purpose, boundaries, maturity, and a short
  architecture summary.
- [Choosing underwrap](comparison.md) — where the wrapper-oriented model fits
  and where simpler tools are preferable.
- [Security](security.md) — threat model, hardened-posture claim, assumptions,
  mechanisms, and limitations.

## Complete the first deployment

1. [Getting started](getting-started.md) — a deterministic local tutorial with
   one backend, one frontend instance, and one stateless wrapper.
2. [Admin UI](admin-ui.md) — bootstrap, stores, client enrollment, resources,
   permissions, users, and encrypted-store operations.
3. [Configuration](configuration.md) — NixOS and backend configuration
   reference.
4. [Deploying the backend](remote-backend.md) — separate backend and target
   hosts, TLS, runtime client enrollment, and operational hardening.

## Design wrappers

- [Agent-facing wrappers](agent-facing-wrappers.md) — expose a narrow command
  rather than a reusable token or broad CLI.
- [Wrapper author checklist](wrapper-author-checklist.md) — target, arguments,
  environment, helper lookup, token scope, projections, and local-user model.
- [GitHub via libsecret](gh-libsecret.md) — share a private Secret Service
  resource between an authentication wrapper and narrower action wrappers.

## Reference

- [Architecture](architecture.md) — components, identities, request flow,
  projections, and crate layout.
- [Backend](backend.md) — configuration, runtime API, admin API, policy model,
  entities, and resource blob storage.
- [Admin UI](admin-ui.md) — operator tasks and browser authentication model.
- [Configuration](configuration.md) — NixOS options and backend YAML.
- [Testing](testing.md) — validation commands and the properties exercised by
  each test layer.

The generated OpenAPI document at
`crates/underwrap-backend/openapi.json` is the normative admin HTTP contract.
The runtime API is internal and documented in [Backend](backend.md).

## Security review

Read these in order when reviewing a trust boundary:

1. [Architecture](architecture.md)
2. [Security](security.md)
3. [secretsd security](secretsd-security.md) for the target-facing D-Bus
   boundary
4. [Testing](testing.md)

The security documentation describes a pre-release design, not a formal proof
or independent audit.

## Contributors and coding agents

- [`AGENTS.md`](../AGENTS.md) — canonical engineering and security instructions
- [Testing](testing.md) — command and coverage reference
- `.pi/skills/underwrap-ticket-workflow/SKILL.md` — ticket execution workflow
- `.pi/skills/underwrap-component-refine/SKILL.md` — review-and-ticket-plan
  workflow

`CLAUDE.md` is a symlink to `AGENTS.md`, so agent-specific entry points cannot
drift from the canonical project contract.
