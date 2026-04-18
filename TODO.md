# underwrap — TODO

The backend release plan in
[`docs/backend-release-plan.md`](docs/backend-release-plan.md) was
worked off. What's left is the usual polish and follow-up work.

## Soon

- [ ] Central audit forwarding: `underwrapd` should POST the signed
      per-call approval blob to the backend so operators see every
      approved invocation in one place, not just on the target host.
- [ ] Admin API operator-scoped tokens: today every operator has equal
      power. Add operator roles (read-only, policy-editor, secret-editor)
      scoped by store.
- [ ] mTLS for the admin listener as an alternative to bearer tokens.
- [ ] Admin UI: grow into real reactivity (Leptos + hydration) when the
      first page needs live updates.
- [ ] Central audit log surface on the admin API + a UI page for it.

## Later

- [ ] Secret metadata / versioning
- [ ] Encrypted backend: passphrase-derived key via Argon2
- [ ] sops / age import pipeline
- [ ] Home Manager helpers (completions)
- [ ] Multi-operator audit-scoped tokens in the UI
