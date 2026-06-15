# Story 1000: Runtime directory for isolated targets — give X its own, opt into the caller's

## Context

Under mandatory target-UID isolation, every wrapped target runs as a dedicated
`underwrap-target-*` uid X. `XDG_RUNTIME_DIR` is currently derived as
`<xdg_runtime_dir_base>/<caller-uid>` (the **caller's** runtime dir), so the
uid-X target cannot use it: the caller's `/run/user/<uid>` is `0700` and
caller-owned.

The first attempt (now on `main`, commits `0a7c959` + `1a42767`) papered over
this by idmapping the caller's `/run/user/<caller-uid>` caller↔X into the
namespace, default-on. That surfaced a real bug — `mount_setattr(IDMAP)` →
`EINVAL` on a live desktop session, because `/run/user/<uid>` carries fuse
submounts (xdg-document-portal `/doc`, gvfsd-fuse `/gvfs`) that don't support
idmapped mounts, and the idmap was applied recursively.

Stepping back, that approach was a category mismatch. idmap is a
file-ownership-translation tool built for caller-owned **config/state
directories** (`~/.config/gh`). `/run/user/<uid>` is a **live session runtime
dir**: IPC sockets plus foreign (fuse) filesystems. Idmapping the whole thing by
default also (a) leaks target scratch into the caller's real session dir, (b) is
fragile w.r.t. submounts, and (c) depends on the caller having a logind session.

The key realization: once we run the target as a *different* uid X, the runtime
dir is a per-uid resource, so **X should get X's own runtime dir.** Forcibly
idmapping the caller's runtime dir back was papering over the uid switch.
Reaching the caller's *session sockets* (wayland / D-Bus / ssh-agent — the
browser-auth wrappers) is a separate, narrower need.

## Decision

Split the two needs that were conflated:

1. **Default — `XDG_RUNTIME_DIR` is X's own provisioned runtime dir.**
   A per-target-uid directory (e.g. `<runtimeBase>/<target-user>`), owned by X,
   mode `0700`, provisioned at the **system level** by the NixOS module (which
   already creates the target users). `underwrap-ns` sets `XDG_RUNTIME_DIR` to
   it, derived from the *executing* uid X (resolved from the root-owned
   target-uid table — never the wire), not the caller. No idmap. Works without a
   caller session; no leak into the caller's session; correct per-uid semantics.

   Why the module must provision it: logind only creates `/run/user/<uid>` for
   login sessions, so a nologin system uid X never gets one; and `underwrap-ns`
   cannot create it at runtime (it holds `cap_sys_admin,setpcap,setuid,setgid`
   but **not** `CAP_DAC_OVERRIDE`/`CAP_CHOWN`, and `/run/user` is root-owned and
   logind's tree). Use a dedicated base (e.g. `/run/underwrap-runtime`) rather
   than `/run/user/<X>` to stay out of logind's tree.

2. **Opt-in — `accessRuntimeDir` (default `false`) gives the target the
   caller's session.** For wrappers that genuinely need the caller's session
   sockets (e.g. `aws-sso`/`acli` browser auth via firefox → wayland + session
   D-Bus): point `XDG_RUNTIME_DIR` at the caller's `/run/user/<caller-uid>` and
   idmap it caller↔X **non-recursively** (so fuse submounts that can't be
   idmapped are dropped instead of failing the whole mount). This is where idmap
   actually belongs. Carries the documented libsecret-bus caveat: it makes the
   caller's real session bus reachable at `$XDG_RUNTIME_DIR/bus` (a functional
   concern — `DBUS_SESSION_BUS_ADDRESS` still selects the brokered bus; the
   brokered secret is not exposed to the caller).

This **supersedes** the default-on idmap (`1a42767`) and **repurposes** the
`accessRuntimeDir` flag (`0a7c959`) — it flips back to default-off with the new
caller-session meaning. The CLAUDE.md threat-model section (`e8b2591`) stays.

## Relationship to the threat model

Consistent with `CLAUDE.md` "Threat model and non-goals": the caller is the
adversary; the target is trusted. Giving X its own runtime dir is not
sandboxing — it is the honest reflection of "this runs as X." The default does
not expose any protected asset to the caller. The opt-in *adds* target access to
the caller's own resources (never a regression in the protected direction).

## Child tickets

- `1010_task_nix_provision_target_runtime_dir.md` — NixOS module provisions a
  per-target-uid runtime dir and plumbs the base.
- `1020_task_ns_xdg_runtime_dir_from_target_uid.md` — `underwrap-ns` points
  `XDG_RUNTIME_DIR` at the executing uid's provisioned dir by default.
- `1030_task_accessruntimedir_opt_in_caller_session.md` — repurpose
  `accessRuntimeDir` to the default-off caller-session opt-in; non-recursive
  idmap.
- `1040_task_runtime_dir_tests.md` — unit + VM coverage for both modes,
  including the fuse/foreign-submount case and the session-less caller.
- `1050_task_runtime_dir_docs.md` — documentation, **including the design
  rationale** (see that ticket for the required "why" and its location).

## Rejected alternatives (document these in 1050)

- **Idmap the caller's `/run/user` by default** (the current `main` state):
  category mismatch; submount-fragile; leaks scratch into the caller's session;
  needs a caller session. Demoted to the explicit opt-in.
- **Private per-invocation tmpfs for `XDG_RUNTIME_DIR`**: works for scratch and
  is robust, but it is a namespace-local hack for "X has no runtime dir," is
  ephemeral per-run, and is a non-canonical path. Provisioning a real per-uid
  dir at the system level is cleaner and is what a runtime dir actually is.
- **A shared `1777` runtime base keyed on the caller uid** (an external
  suggestion): world-writable, collides across distinct target uids, wrong
  ownership model.

## Acceptance

- A wrapper with no extra config gets a writable `XDG_RUNTIME_DIR` owned by its
  target uid; scratch writes do **not** appear in the caller's `/run/user`.
- A wrapper with `accessRuntimeDir = true` can reach the caller's session
  sockets even on a desktop session with fuse portals mounted (no
  `mount_setattr` `EINVAL`).
- A caller with no logind session can still run wrappers (default path does not
  depend on `/run/user/<caller-uid>`).
- `cargo test --workspace`, `clippy -D warnings`, `fmt --check`, and the VM test
  all green. Docs (incl. rationale) updated.
