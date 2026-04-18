# underwrap — TODO

Work is tracked against milestones in
[`docs/backend-release-plan.md`](docs/backend-release-plan.md).

## Now

- [ ] M1 — approval model rework (backend-owned modes, per-call ssh-agent
      forwarded to backend, expiry timer)
- [ ] M2 — remove unix-socket backend transport, rename approval modules
- [ ] M3 — admin API bearer-token auth
- [ ] M4 — `underwrap-admin-ui` crate (Leptos + axum), login flow, wrapper
      mode / rule / grant editors, browser tests
- [ ] M5 — collapse `backend-v2-*.md` drafts, rewrite user-facing docs

## Later

- [ ] Secret metadata / versioning
- [ ] Encrypted backend: passphrase-derived key via Argon2
- [ ] sops / age import pipeline
- [ ] Home Manager helpers (completions)
- [ ] mTLS for the admin API (in addition to bearer tokens)
- [ ] Multi-operator roles and audit-scoped tokens in the UI
