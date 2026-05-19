# Task: Documentation update for atomic resource blobs

Parent: `430_story_atomic_blob_documentation_and_vm_cutover.md`

## Scope

Update the docs to reflect the new storage layout, protocol, and
wrapper behavior. The old per-key/per-slot/per-item picture is
removed from prose; only the new model is described.

### Files to update

`docs/security.md`:

- "Design invariants" — note that on libsecret mode, secretsd holds
  the full resource blob (including all secret bytes) in memory for
  the lifetime of the PID namespace. Reaffirm that
  `PR_SET_DUMPABLE=0` is set before reading the blob.
- Update or remove the per-call secretsd channel section; replace
  with a single subsection describing Checkout / Checkin /
  UsageSignal and the server-side write gate.
- Add a short subsection on the always-writable namespace mount and
  why server-side gating is sufficient (the target is already inside
  a private namespace; bind-RO only added defense against accidental
  writes inside the namespace, which the server-side reject now
  covers symmetrically without breaking write-test patterns).

`docs/architecture.md`:

- Replace the per-key bundle-snapshot section with one-blob-per-
  resource and the three IPC verbs.
- Update the libsecret-mode block diagram if present.

`docs/backend.md`:

- Update the data-model section — resource blob is one storage
  key per resource (`resources/<resource_id>`).
- Document the new admin endpoints from task 404.
- Note last_used_at on pair records.

`examples/config.yaml`:

- Comment near the `files:` section: the namespace mount is always
  rw; the server gates persistence based on the grant. Mention
  `UNDERWRAP_SUPPRESS_READONLY_WARNING`.

`CLAUDE.md`:

- Replace the libsecret architecture block with the new three-verb
  protocol.
- Add `UNDERWRAP_SUPPRESS_READONLY_WARNING` to the wrapper env
  whitelist note.
- Update "Key files to understand first" to point at the new
  wrapper-resource channel handler and the resource blob schema.
- Remove references to `frontend-secretsd/src/channel.rs` (gone)
  and update references to `frontend-ns/src/directory_credential.rs`.

`docs/gh-libsecret.md` (if it exists and references the per-call
protocol):

- Refresh to match.

## Acceptance criteria

- [ ] Every file listed above has been updated; no doc references
      the deleted per-key protocol or the bind-RO mount.
- [ ] Diagrams (ascii or otherwise) match the new IPC.
- [ ] Cross-references between docs are correct.
- [ ] `examples/config.yaml` still parses (`cargo test` covers it).

## Out of scope

- New conceptual docs beyond what the existing structure asks for.
- Operator-facing onboarding rewrites — separate epic if needed.
