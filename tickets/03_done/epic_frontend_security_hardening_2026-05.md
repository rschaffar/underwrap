# Epic: Frontend security hardening (2026-05)

## Background

Holistic security review of the frontend (wrapper chain, daemon,
ns helper, secretsd, NixOS frontend module) — see the session
transcript for the full review. Backend / admin UI are out of scope
for this epic; they will be reviewed separately.

The architecture is sound and the implementation matches it closely.
The gaps I found are first-order: clear deviations from stated
invariants and a small set of defence-in-depth opportunities. None
require architectural change.

This epic collects every concrete tightening that came out of the
review, ordered roughly by load-bearing-ness.

## Findings, in priority order

1. **Resource-channel fd leaks into dbus-daemon and secretsd**
   (`crates/frontend-ns/src/libsecret.rs`). Both helper children
   explicitly close `secret_fd` (fd 3) and the readiness/init/addr
   pipes but never the wrapper-resource channel fd. The docstring
   under "Descriptor hygiene" promises this fd does not leak across
   trust boundaries; today it leaks across two. Closing this also
   neutralises any exploit path that goes through dbus auto-activation
   of a caller-supplied service file.
   → `1400_task_libsecret_helper_fd_hygiene`

2. **dbus-daemon and secretsd inherit caller env.** Both helpers are
   forked with `std::process::Command::new(...).exec()` without
   `env_clear()`. `dbus-daemon --session` discovers service-activation
   files via `XDG_DATA_DIRS`, which the caller controls. Closes the
   only realistic exploit path I see in libsecret mode when combined
   with finding 1.
   → `1410_task_libsecret_helper_env_clear`

3. **`/proc` is not remounted in env-only and files-only modes.**
   `prepare_namespace` unshares `CLONE_NEWPID` unconditionally but
   `setup_mount_namespace` (which remounts `/proc`) only runs when
   libsecret is enabled. Without a fresh `/proc` the new PID namespace
   contributes containment, not confidentiality. The doc's framing
   leans on "separate PID space" harder than the reality justifies in
   non-libsecret modes.
   → `1420_task_remount_proc_with_newpid`

4. **`drop_caps` bounding-set loop breaks on `EPERM`**
   (`crates/frontend-ns/src/caps.rs:30-34`). Today the NixOS module
   guarantees `cap_setpcap` on underwrap-ns so this never fires, but
   "no silent fallback to a weaker mode" (security.md §6) cuts the
   other way: a future change that loses `cap_setpcap` would leave
   the bounding set populated rather than abort.
   → `1430_task_drop_caps_eperm_hard_error`

5. **`SO_PEERCRED` + `/proc/<pid>/status` is the classic racy
   combination** (`crates/frontend-underwrapd/src/peer_auth.rs:33-49`).
   PID recycling makes the two-syscall identity check formally racy.
   Not exploitable in this architecture because socket access already
   requires a setgid wrapper-group hop — the racy "second" peer must
   itself be a legitimate wrapper instance — but worth tightening as
   defence-in-depth.
   → `1440_task_so_peerpidfd_runtime_auth`

6. **`recv_fd` doesn't check `MSG_CTRUNC`**
   (`crates/shared-protocol/src/scm_rights.rs:60-99`). One-fd protocol
   makes truncation risk low today; the check is a one-liner that
   protects against silent fd loss if the protocol ever grows.
   → `1450_task_recv_fd_msg_ctrunc_check`

7. **Doc sweep**: TMPDIR / SSH_AUTH_SOCK / DISPLAY passthrough
   implications, and the "PID namespace makes process discovery less
   direct" framing in security.md, both need sharpening to match the
   real protection story. The current doc isn't wrong — it's unevenly
   honest, which is the same problem.
   → `1460_task_security_doc_sharpening`

8. **underwrapd systemd unit hardening + client cert/key mode
   verification.** The daemon process holds the runtime mTLS client
   credential on disk and in memory; a compromise of the daemon
   process on the target host is effectively a compromise of the
   runtime client identity. The unit should be locked down
   aggressively (`NoNewPrivileges`, `ProtectSystem=strict`,
   `PrivateTmp`, `MemoryDenyWriteExecute`, `RestrictAddressFamilies`,
   etc.) and the client cert/key file modes need to be verified to
   actually match the doc's "readable only by the daemon user" claim.
   → `1470_task_underwrapd_systemd_and_cert_modes`

9. **secretsd D-Bus surface — adversarial pass.** secretsd is the one
   place where target-supplied bytes get parsed by our own code
   (zbus + our dbus_service handlers). No known bug, but it's the
   natural boundary where a hostile target meets our code, and
   deserves a deliberate fuzz pass before promoting libsecret mode as
   the recommended path.
   → `1480_task_secretsd_dbus_adversarial_pass`

## Out of scope

- Backend / admin UI hardening (separate review).
- "Make libsecret/files mode the path of least resistance" —
  product-shape change, not security tightening per se.
- Anything that would require changing the same-UID architectural
  posture (dedicated UIDs per wrapper, VM isolation).

## Order of attack

Findings 1 and 2 together close the only realistic exploit chain
in libsecret mode and should be done as a pair. 3 is a small code
change with a doc tightening attached. 4 is principled clean-up.
The rest is defence-in-depth and documentation; do in any order
once 1–4 are in.
