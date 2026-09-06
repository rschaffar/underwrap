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
5. [Current candidate security-review handoff](research/2026-09-01_security-review-handoff.md)
6. [Bounded independent review outcome](research/2026-09-01_release-readiness-independent-review.md)

The security documentation describes a pre-release design, not a formal proof or external independent end-to-end audit.

## Internal project records

- [`AGENTS.md`](../AGENTS.md) — canonical engineering, security, and workflow guidance
- [Plans](plans/README.md) — active executable plans and their evidence lifecycle
- [Archived plans](plans/archived/README.md) — completed plan handoffs
- [ADRs](adr/README.md) — accepted decisions and trade-offs
- [Implementation notes](implementation-notes/README.md) — maintained current mechanisms and empirical findings
- [Research](research/README.md) — dated investigations, rejected options, and retractions
- [Roadmaps](roadmaps/README.md) — future direction without implementation authority
- [Testing](testing.md) — command and coverage reference
- [Writing plans](../.agents/skills/writing-plans/SKILL.md) — active plan workflow
- [Reviewing code](../.agents/skills/reviewing-code/SKILL.md) — ordinary bounded read-only review
- [Hardening code paths](../.agents/skills/hardening-code-paths/SKILL.md) — bounded edge-case implementation
- [Comprehensive code review](../.agents/skills/underwrap-codebase-review/SKILL.md) — whole-repository or explicitly comprehensive review
- [Historical ticket records](../.agents/skills/underwrap-ticket-workflow/SKILL.md) — interpretation guidance for the superseded ticket procedure

The accepted review report, handover, and completed tickets remain immutable historical evidence.

Future comprehensive reviews may append new reports under `tickets/_review/` without overwriting existing files.

`tickets/` is no longer the active work board for new work.

`CLAUDE.md` is a symlink to `AGENTS.md`, and `.claude/skills` and `.pi/skills` point to the canonical `.agents/skills/` directory.
