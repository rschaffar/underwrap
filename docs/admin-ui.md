# Admin UI

`underwrap-admin-ui` is the operator-facing web service that wraps the
backend's admin API in a form-based UI.

Operators log in with a username + password, pick a backend target, and
manage wrapper modes, rules, and grants. The UI holds the backend's
admin bearer token internally and signs session cookies with its own
session key.

## Deployment shape

```text
browser ──TLS──► reverse proxy ──HTTP──► underwrap-admin-ui ──HTTP bearer──► underwrap-backendd admin
```

The UI itself serves plain HTTP. Production deployments should put a
TLS-terminating reverse proxy (nginx, Caddy, systemd's
`ProtectHostname=` front, …) in front of it. The session cookie is
`httponly; SameSite=Lax`, and CSRF tokens protect every state-changing
form.

## Configuration

```yaml
listen: 127.0.0.1:8080

session_key_file: /run/keys/underwrap-admin-ui.session

operators:
  alice:
    password_hash_file: /etc/underwrap-admin-ui/alice.phc
  bob:
    password_hash_file: /etc/underwrap-admin-ui/bob.phc

backends:
  corp:
    admin_url: http://127.0.0.1:19443
    token_file: /run/keys/underwrap-admin-corp
  personal:
    admin_url: http://127.0.0.1:19444
    token_file: /run/keys/underwrap-admin-personal
```

- `session_key_file` holds exactly 32 random bytes. Generate once and
  keep stable — all existing sessions invalidate when this changes.
- Each operator's `password_hash_file` contains an Argon2 PHC-encoded
  hash. Create one with:

  ```bash
  cargo run -p underwrap-admin-ui --bin gen-password  # (TODO helper)
  ```

  Until the helper ships, any `argon2` CLI works:

  ```bash
  echo -n "$PASSWORD" | argon2 "$(openssl rand -base64 16)" -id -e > alice.phc
  ```

- Each `backends.*.token_file` is a file containing the bearer token
  declared in the matching `underwrap-backendd` `admin.tokens` entry.

## Pages

- **`/login`** — username + password form. Submits via `POST /login`.
- **`/`** — logged-in landing page listing the configured backend
  targets.
- **`/t/<target>`** — store switcher and the three editors below.

Inside one store view:

- **Wrappers** table — shows each wrapper's current mode with a color-
  coded tag. Below each row, an inline form changes the mode to
  `locked`, `unlocked` (permanent), `unlocked_until` (with an epoch
  timestamp), or `per_call_approval` (with the OpenSSH public key the
  operator's ssh-agent must sign with).
- **Rules** table + "Add rule" form — id, wrapper, resource, operation,
  allow/deny.
- **Grants** table + "Add grant" form — same tuple plus optional
  start/expire epochs and a reason.

Every state-changing form carries the session's CSRF token in a hidden
input and is rejected with `403` if the token doesn't match.

## Audit

Every successful admin request ends up in `underwrap-backendd`'s audit
log with the operator name from the authenticated bearer token. The UI
does not forward free-form comments today; the `reason` fields on the
wrapper state and grant forms are the operator-visible notes.

## Testing

- Unit tests cover session cookie sign/verify, CSRF match, password
  verification, and HTML escaping on every template.
- An end-to-end test (`tests/end_to_end.rs`) boots a real
  `underwrap-backendd` + the UI in-process, drives the full
  login → wrapper-mode-edit → backend-policy-changed loop over HTTP,
  and asserts that CSRF rejection returns `403` and wrong-password
  returns `401`.
