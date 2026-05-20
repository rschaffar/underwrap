# Story: one resource per wrapper on the frontend

## Motivation

Today a wrapper can project N resources via `WrapperDef.resources:
HashMap<String, ResourceUse>`. The semantics that actually survive in the
daemon are "at most one resource per slot" (`env`, `files`, `libsecret`),
because a single namespace has one private D-Bus, env-var names from
different resources can collide, and `files` mount paths can overlap. The
current code silently picks one when slots collide
(`wrapper_libsecret_resource` uses `HashMap::iter().find_map`, which is
non-deterministic across runs), with no diagnostic.

The cleaner shape is: **one wrapper projects exactly one resource**, and
that resource can opt into any combination of the three slots. This
matches the libsecret constraint without papering over it, removes a
silent failure mode, and shrinks the wrapper-flow protocol meaningfully.

## Scope: frontend only

The collapse is **frontend-side only**. The backend pair table
(`PairsDocument`, keyed by `(wrapper, resource)`) and the grant key
(`(client_instance, wrapper, resource)`, see commit `a867e9f`) **stay as
they are**. Reason: different frontend instances may legitimately map the
same wrapper name to different resources, and — more importantly — the
resource is the part of the grant key that pins authorization to a
specific secret. Collapsing the grant key to `(instance, wrapper)` would
let a frontend-side config change silently redirect an existing grant to
a different secret. That's a trust-boundary downgrade and we do not want
it.

So the invariant lives at the frontend config layer: **a `WrapperDef`
has one `resource`.** The backend continues to see full
`(wrapper, resource)` granularity in the pair table and grants.

## Target shape

```yaml
wrappers:
  gh-authorized:
    target: /usr/bin/gh
    socket: /run/underwrap/gh-authorized.sock
    group: underwrap-gh-authorized
    resource: github/token
    env: true
    files:
      paths:
        - .config/gws
    libsecret: false
```

`ResourceUse` is deleted. `WrapperDef` carries `resource: String` plus
flat `env: bool`, `files: Option<FilesUse>`, `libsecret: bool`.

## Simplifications enabled

**Config / daemon helpers** (`wrapper_bindings.rs`, `startup.rs`):

- `wrapper_libsecret_resource`, `wrapper_env_resources`,
  `wrapper_file_bindings`, `wrapper_all_resources`,
  `resource_has_projection`, `FileProjection` all collapse into a
  single derivation from the wrapper's one resource.
- `startup.rs` resource-loop validation becomes a straight-line check.
- The "duplicate env var across resources" runtime check in
  `wrapper_flow/read.rs:159` is no longer possible to hit — drop it.

**Wrapper-flow protocol** (`shared-protocol/src/message/wrapper.rs`):

- `WrapperResponse::ReadOk { env, resources: Vec<ReadResourceBinding> }`
  collapses. The single binding's fields (`resource_id`, `access`,
  `libsecret`, `files_target_paths`) inline directly into `ReadOk`.
- `ReadResourceBinding` struct is deleted.
- The SCM_RIGHTS handshake sends exactly **one** channel fd after
  `ReadOk`, not one-per-resource (see `wrapper_flow/channels.rs:29`).
- Per-resource grant loop in `wrapper_flow/read.rs` (and
  `approval_round.rs`) becomes a single grant lookup.

**Register flow** (`frontend-underwrapd/src/server.rs:56`):

- Inner loop iterates `wrapper.resources` to enumerate pairs to
  register; collapses to a single pair per wrapper.

**NixOS module** (`nix/modules/underwrap.nix`):

- `services.underwrap.instances.<n>.wrappers.<w>` exposes
  `resource`/`env`/`files`/`libsecret` at the wrapper level (no
  `resources` submodule). Module-level assertion that `resource` is
  set.

**Docs**:

- `examples/config.yaml` — re-shape the annotated example.
- `docs/security.md` — update any sections that describe the
  multi-resource projection (the slot table and libsecret section).
- `AGENTS.md` (this repo's CLAUDE.md) — the libsecret architecture
  block still reads correctly under the new constraint, but the
  "Identity model" / "Grant model" sections should mention the 1:1
  frontend mapping.

**VM test** (`nix/tests/`):

- Any wrapper definition with `resources = { ... }` becomes the flat
  shape. The test still exercises env / files / libsecret because each
  test wrapper can still touch all three slots via its one resource.

## Out of scope

- Backend pair table shape — unchanged.
- `AgentGrant` key — unchanged.
- Admin SPA pair views — unchanged (still per-pair across instances).
- Audit event resource field — unchanged (still names the resource).

## Children

- [[010_task_flatten_wrapper_config_schema]]
- [[020_task_frontend_daemon_collapse]]
- [[030_task_wrapper_flow_protocol_collapse]]
- [[040_task_docs_nixos_module_vm_test]]
