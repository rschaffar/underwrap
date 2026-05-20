# Task: flatten wrapper config schema

Parent: [[000_story_one_resource_per_wrapper_frontend]]

## Goal

Reshape `WrapperDef` so each wrapper declares exactly one resource, with
the three projection slots (`env`, `files`, `libsecret`) flat on the
wrapper itself.

## Changes

`crates/shared-protocol/src/config.rs`:

- Delete `ResourceUse`.
- `WrapperDef` becomes:
  ```rust
  pub struct WrapperDef {
      pub target: PathBuf,
      #[serde(default)]
      pub logical_name: Option<String>,
      pub resource: String,
      #[serde(default)]
      pub env: bool,
      #[serde(default)]
      pub files: Option<FilesUse>,
      #[serde(default)]
      pub libsecret: bool,
      pub socket: PathBuf,
      pub group: String,
  }
  ```
- Add `#[serde(deny_unknown_fields)]` so a stray `resources:` block in
  an old config produces a loud parse error rather than silent defaults.
- Update the in-file tests (`full_wrapper_config`,
  `resource_use_rejects_unknown_fields`, `files_projection_requires_paths`)
  to match the new shape. The "rejects unknown fields" test should
  cover both the resource-level rejection and a top-level wrapper-key
  rejection.

## Acceptance

- `cargo test -p underwrap-protocol` passes.
- A config containing the old `resources:` map fails to parse with a
  message that points at the unknown field.
- A config missing `resource:` fails to parse.

## Notes

This task only touches the schema. Everything that consumes
`wrapper.resources` will be broken after this lands; it gets fixed in
[[020_task_frontend_daemon_collapse]] and
[[030_task_wrapper_flow_protocol_collapse]]. Land them together (or in
one PR), since the workspace will not compile in between.
