# Task: Sanitise env for dbus-daemon and secretsd helper forks

Part of: `epic_frontend_security_hardening_2026-05`.

## Problem

`crates/frontend-ns/src/libsecret.rs:setup_libsecret` forks both
helpers via `std::process::Command::new(...).exec()` without
`env_clear()`. Rust's `Command` inherits the parent process's full
environment by default, which here is the caller's environment as
inherited by `underwrap-ns` (the target's env is only assembled in
`build_target_env` and applied to the *target's* `cmd`, not to the
helpers).

This matters specifically for `dbus-daemon --session`: the session
config on NixOS pulls service-activation directories from
`$XDG_DATA_DIRS/dbus-1/services` and `$XDG_RUNTIME_DIR/dbus-1/services`
(see dbus-daemon(1)). `XDG_DATA_DIRS` is not in
`FORCED_TARGET_ENV_NAMES` and is not in the default passthrough list,
so it never reaches the target — but it does reach the bus daemon,
because the bus daemon is forked before the cleaned target env exists.

The well-known-name race for `org.freedesktop.secrets` is closed by
existing code (dbus-daemon doesn't auto-activate on startup;
secretsd's `request_name` completes before its readiness byte), but
caller-controlled service directories combined with the fd leak from
ticket 1400 form a real exploit chain. This ticket closes the
helper-env leg of that chain independently.

It is also straightforward hygiene: the rest of the project applies
"another adversarial-adjacent process, scrub its env" consistently —
the helpers are the one place where this discipline is missing.

## What to do

In `crates/frontend-ns/src/libsecret.rs`:

1. Before each helper fork, build the helper's env explicitly. Two
   reasonable shapes — pick whichever is cleaner in code review:

   a. `cmd.env_clear()` and reintroduce a small whitelist. dbus-daemon
      and secretsd realistically need: `PATH` (set from trusted
      config — the wrapper config's `env_set` is the natural source,
      or hard-code a minimal `/run/current-system/sw/bin` for the
      NixOS path), `HOME` (from the trusted passwd entry, same as the
      target gets), and possibly `TZ`/`LANG` for log messages.
      Explicitly **drop** `XDG_DATA_DIRS`, `XDG_CONFIG_DIRS`,
      `XDG_RUNTIME_DIR`, `DBUS_*`, `LD_*` (already stripped by
      `AT_SECURE` but doesn't hurt), and any `UNDERWRAP_*` not
      explicitly needed.

   b. Or `cmd.env_clear()` and pass nothing. Verify dbus-daemon and
      secretsd actually function with empty env; if not, fall back
      to (a).

2. The dbus-daemon `--address=unix:path=...` argument already pins
   the listen path independent of env; `--session` config files are
   root-controlled. So the only env input that matters for
   `dbus-daemon`'s security posture is the service-discovery
   variables and `LD_*`. Both are caller-controllable today.

3. For secretsd specifically: it takes all its config via CLI flags
   (`--bus-address`, `--init-fd`, `--state-path`, `--ready-fd`). Empty
   env is fine; verify and assert.

4. While here: confirm the existing `env_remove` for `UNDERWRAP_*`
   vars in `frontend-exec` still does the right thing once helpers
   stop inheriting the caller env (no behaviour change expected,
   just verify).

## Related

This ticket and ticket 1400 close two halves of the same realistic
exploit path. Doing both is cheap and removes the only chain in
libsecret mode that I see as practically reachable.

## Acceptance

- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`
- New integration test (or unit test with a stub helper binary):
  set `XDG_DATA_DIRS=/tmp/attacker-controlled` in the caller env,
  invoke a libsecret wrapper, assert dbus-daemon's env (visible via
  `/proc/<dbus-pid>/environ` from the test) does not contain
  `XDG_DATA_DIRS`.
- VM test still green.
- The "Why mount isolation matters" / "Libsecret mode" sections of
  `docs/security.md` updated with one paragraph noting that helper
  processes also get a sanitised environment, so caller-controlled
  service-discovery paths cannot reach the private bus daemon.
