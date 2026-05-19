# Task: Journal + stderr warning on dropped writes, with suppression env var

Parent: `410_story_wrapper_buffered_writeback_and_warnings.md`

## Scope

When `frontend-ns` decides to drop changes — either because the grant
is read-only (locally known from Checkout) or because the server
rejected a Checkin due to a mid-run grant flip — emit a clear,
two-channel warning.

### Journal

One structured `tracing::warn!` line with stable fields:

```
event = "writeback_rejected"
wrapper = "<wrapper_name>"
resource = "<resource_id>"
reason = "grant_readonly" | "server_rejected"
```

This is the same `tracing` setup as the rest of `frontend-ns`, so it
lands in the systemd journal when the wrapper is invoked under
journald (the normal case on NixOS).

### Stderr

One human-readable line, suppressed when
`UNDERWRAP_SUPPRESS_READONLY_WARNING=1` is set on the wrapper's
own environment (not the target's — the env whitelist in
`frontend-exec` does not propagate this var). Suggested text:

```
underwrap: changes to resource '<resource_id>' were not persisted
(grant is read-only). Set UNDERWRAP_SUPPRESS_READONLY_WARNING=1
to silence this message.
```

For `reason = server_rejected`, mention the race explicitly:

```
underwrap: changes to resource '<resource_id>' were rejected by
the backend (grant changed during the run). Set
UNDERWRAP_SUPPRESS_READONLY_WARNING=1 to silence this message.
```

### Env propagation

Add `UNDERWRAP_SUPPRESS_READONLY_WARNING` to the wrapper-only env
whitelist in `frontend-exec` — it must be visible to
`frontend-exec` and `frontend-ns` but stripped before exec'ing the
target (`env_clear()` whitelist).

### Exit status

The warning does not change the wrapper's exit status. The target
already exited; the warning is informational.

## Acceptance criteria

- [ ] `frontend-ns` emits both the structured warn event and the
      stderr line on a dropped write.
- [ ] `UNDERWRAP_SUPPRESS_READONLY_WARNING=1` suppresses the stderr
      line but not the journal event.
- [ ] The env var is never visible to the target process.
- [ ] Unit tests cover: suppression on, suppression off, both
      reason values, multiple resources in the same run produce
      one line per resource.
- [ ] `cargo test --workspace` green; clippy clean.

## Out of scope

- The decision logic itself — task 413.
- VM-level assertions on journal output — task 432.
