# Task: Sharpen security.md framing around passthrough and PID-ns

Part of: `epic_frontend_security_hardening_2026-05`.

## Redesign interaction (read first)

`epic_single_instance_wrapper_redesign_2026-05` rewrites the
**identity / trust-model** sections of `docs/security.md` (setgid +
effective GID, wrapper-group identity, the "never trust
client-supplied wrapper names" rule) in ticket `1580`. This ticket
does **not** own those sections — keep it scoped to:

- §"Environment sanitization" passthrough trade-offs (`TMPDIR`,
  `SSH_AUTH_SOCK`, `DISPLAY`/`WAYLAND_DISPLAY`) — fully
  redesign-independent;
- §"Guarantee under the hardened posture" narrow-helper precondition
  — fully redesign-independent;
- §"What the PID namespace contributes" — **overlaps ticket `1420`**
  (which makes the `/proc` remount unconditional). If 1420 has
  landed, this is the one-paragraph cleanup described below.

Because `1420`, `1440`, `1470`, `1580`, and this ticket all edit
`docs/security.md`, do them as **one coherent documentation pass at
the end** (see the execution plan's Phase 4) rather than five
separate, conflicting edits to the same file. The passthrough and
narrow-helper content here is redesign-independent and could be
drafted early, but commit it as part of that final pass.

## Problem

`docs/security.md` is unusually honest, but two sections are framed
in ways that overstate what the system does in env-only mode, and
one section understates how load-bearing the "narrow helper"
precondition is when combined with default passthrough.

### Passthrough overstatement

§"Environment sanitization" leans hard on `LD_PRELOAD` /
`LD_LIBRARY_PATH` as the canonical risky cases (correctly), but the
`DEFAULT_PASSTHROUGH` list in `crates/frontend-ns/src/env.rs:7-32`
includes:

- `TMPDIR` — caller picks where the target writes transient files,
  including any temp files the target derives from the token.
- `SSH_AUTH_SOCK` — the target gets the caller's ssh-agent and
  every key currently loaded into it.
- `DISPLAY` / `WAYLAND_DISPLAY` — target inherits caller's display
  socket.

These are all "expected for usability" but they meaningfully
constrain what "narrow helper" can promise. A helper that touches
`$TMPDIR` for caching, or relies on `SSH_AUTH_SOCK` for signing,
has a much harder narrowness story than the current framing
implies. The doc doesn't acknowledge this trade-off.

### PID-namespace claim

§"What the PID namespace contributes" lists:

> - giving the target a separate PID space
> - making process discovery less direct from inside the namespace
> - combining well with a fresh `/proc` mount in libsecret mode

Ticket 1420 makes the `/proc` remount unconditional, at which
point all three modes get the namespace contribution uniformly.
This section needs to be rewritten against the new reality and the
"in libsecret mode" qualifier removed.

If 1420 lands first, this is a one-paragraph cleanup. If 1420 is
delayed, this ticket should still land — by clearly framing env
mode as "PID-namespace contributes containment, not confidentiality
of `/proc` reads" and pointing operators to Yama for the
confidentiality piece.

### Narrow-helper precondition

§"Preconditions" already lists "narrow helper" with three sub-bullets.
What it doesn't make obvious is that this precondition is doing
**most** of the work in the env-mode guarantee — Yama protects the
target's environ from same-UID *peers*, but it does nothing against
the target itself emitting or persisting the token. The doc reads
as if narrow-helper is one precondition among equals; in practice
it dominates.

## What to do

In `docs/security.md`:

1. **§"Environment sanitization"** — add a paragraph after the
   bullet list explicitly listing TMPDIR, SSH_AUTH_SOCK, DISPLAY,
   and WAYLAND_DISPLAY as caller-controlled passthrough variables
   the *target* inherits. Spell out:

   - `TMPDIR`: the caller chooses where the target writes transient
     files. Narrow helpers should treat `$TMPDIR` as caller-owned
     and not rely on it for secret persistence.
   - `SSH_AUTH_SOCK`: the target inherits the caller's ssh-agent
     and every loaded key. A helper that signs over agent keys is
     therefore signing with whatever the caller has loaded, not
     with a wrapper-scoped agent.
   - `DISPLAY` / `WAYLAND_DISPLAY`: standard X/Wayland trade-off,
     just name it.

   None of these is wrong by default; they exist for usability.
   The doc should make the trade-off explicit so an operator can
   trim the passthrough list when their wrapper doesn't need it.

2. **§"What the PID namespace contributes"** — rewrite to reflect
   that the namespace contributes the same protection (separate
   PID space + fresh /proc + mount isolation when applicable) in
   every mode. If 1420 has landed, just drop the "in libsecret
   mode" qualifier. If 1420 has not landed yet, frame env mode
   honestly: "in env-only mode the PID namespace contributes
   containment (PID 1 dies, kernel SIGKILLs the rest); Yama
   `ptrace_scope=2` is what gives confidentiality of the target's
   environ against same-UID observers."

3. **§"Guarantee under the hardened posture"** — under
   "Preconditions", lift the "narrow helper" sub-bullet into its
   own paragraph and add: "This precondition is the single most
   load-bearing element of the env-mode guarantee. Yama and
   cleared host env protect the *target's environ* from same-UID
   observers; nothing in the design protects the secret from the
   target *itself*. Use libsecret or files mode wherever the
   target supports it."

4. **§"Final perspective"** — add a one-line pointer to the new
   passthrough paragraph so a reader who reaches the bottom of
   the doc encounters the trade-off explicitly.

## Acceptance

- `docs/security.md` reads truthfully end-to-end.
- A reader of the new "narrow helper" paragraph leaves understanding
  that env-mode safety depends on what their helper does with the
  token, not just on Yama.
- A reader of the new passthrough paragraph leaves with an
  actionable list of variables they should consider trimming for
  their specific wrapper.
- No tests change.
