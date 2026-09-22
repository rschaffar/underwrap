# Agent discovery and usage

An agent should discover configured capabilities rather than guess wrapper names, extract credentials, or invoke an underlying authenticated CLI directly.
The portable [Underwrap skill](../.agents/skills/underwrap/SKILL.md) explains the caller workflow, including mapped input/output paths and safe failure handling.
It complements tool-specific skills; it does not authorize service operations or administer backend permissions.

## Discover local wrappers

The NixOS module installs the unprivileged `underwrap-info` helper globally, independently of `allowedUsers`.
It reads only the root-owned public `/etc/underwrap/discovery.json` generated from the same wrapper options as the execution configuration.
It does not execute a target, contact a daemon or backend, inspect credentials, or probe filesystem access.
Discovery works without enrollment or running backend services.

```sh
/run/current-system/sw/bin/underwrap-info
/run/current-system/sw/bin/underwrap-info --json
/run/current-system/sw/bin/underwrap-info --instance work
/run/current-system/sw/bin/underwrap-info documents --instance work --json
```

Without a wrapper argument, all configured wrappers are listed, including those with no convenience alias users.
`--instance` filters the inventory or disambiguates a wrapper name shared by multiple frontend instances.
An unknown instance or wrapper, ambiguous wrapper selection, missing/invalid catalog, or unsupported schema version exits unsuccessfully with a diagnostic on stderr.
An empty known inventory succeeds.
`--help` and `--version` do not need a catalog.
There is no live-policy probe, arbitrary catalog-path override, automatic retry, or target-argument interception.

The examples use `/run/current-system/sw/bin/underwrap-info` explicitly to avoid shell/PATH shadowing.
In particular, the valid instance name `info` creates the different setgid entry point `/run/wrappers/bin/underwrap-info`; it is not the discovery helper.
Convenience wrappers can also collide with system helper names such as `underwrap-info` or `underwrap-keygen`.
For execution, the returned `command` array selects the instance explicitly and avoids ambiguity between convenience aliases.
Pass its elements as argv, append only task-appropriate target arguments, and never evaluate descriptions or JSON as shell code.
Respect any separately configured OS-level gate; inventory is not proof that an entry point is executable by this caller.

### Public descriptions

Add an optional `description` to a wrapper definition:

```nix
services.underwrap.instances.work.wrappers.documents = {
  description = "Search and download team documents through the configured account.";
  target = "${reviewedDocumentsHelper}/bin/documents";
  resource = "documents/team";
  files.paths = [ ".config/documents" ];
  accessPaths = [ "~/Documents/agent-work" ];
};
```

This fragment assumes the instance backend, package and reviewed helper are supplied by the enclosing configuration.
The access root must already exist; discovery does not create it or grant permissions.
Descriptions default to the empty string and describe intended use, not current grants or guaranteed tool behavior.
They do not enter daemon configuration, the privileged handoff, or backend registration.
Descriptions and all discovery fields are public Nix-store and local catalog data: do not include secrets or sensitive identifiers.
Terminal output escapes control characters; JSON preserves description text as data.

### JSON contract

Both inventory and single-wrapper detail return one object with a `wrappers` array.
Consumers should require the supported `schema_version` before interpreting fields.

| Field | Meaning |
| --- | --- |
| `schema_version` | Currently `1` |
| `scope` | Always `local-configuration` |
| `authorization`, `filesystem_access` | Always `not-checked` |
| `instances` | Configured instance names, filtered by `--instance`; includes instances without wrappers |
| `wrappers[].instance`, `.name` | Exact frontend instance and wrapper selectors |
| `.description` | Public operator text; may be empty |
| `.command` | Two-element ordinary-invocation prefix: absolute instance entry point and wrapper selector |
| `.alias.name`, `.alias.users` | Convenience alias and users configured to receive it; not a PATH lookup or ACL |
| `.access_paths` | Configured ID-mapped host path specifications; absolute, `~`, or `~/...` |
| `.projections.env`, `.projections.libsecret` | Enabled projection channels, not their contents |
| `.projections.files` | Configured files projection mount paths, relative to the caller's passwd home |
| `.fixed_home`, `.fixed_tmpdir` | Whether trusted `envSet` supplies these names; values are not exported |

Lists are ordered by instance and wrapper name.
Debug variants are not separate inventory entries; unsafe debug is never an ordinary fallback.
The allowlisted catalog omits target binaries, resource names/values, environment values, backend URLs, identity paths, policy and writeback status.
It describes the installed local configuration, not daemon readiness or which generation a running daemon has loaded.

## Local inputs, outputs and temporary files

The target runs under a dedicated UID, not the caller's UID.
Prefer absolute paths beneath an appropriate configured `access_paths` root, outside every configured files projection, for ordinary task artifacts.
Use a private per-task directory and apply this to inputs, attachments, `@file` arguments, downloads and exports alike.
Resolve `~` against the caller's passwd home rather than assuming the shell's `HOME`; a fixed target HOME does not relocate access-path resolution or files projections.
Existing permissions, ancestor traversal, symlinks, submounts and filesystem support still matter, so a listed path is not a writability guarantee.

A caller-created private `/tmp` directory may be inaccessible to the dedicated target UID; `/tmp` is not categorically forbidden.
Setting `TMPDIR` to a suitable mapped directory can help tools that honor it, but cannot rewrite explicit `/tmp/...` arguments.
Fixed or projected environment values can supersede the inherited `TMPDIR`.
Do not place task artifacts in projected credential directories or solve failures by extracting credentials, broadening permissions/mounts, enabling unsafe debug, or granting backend writeback.
Host-file writes are separate from backend-managed resource persistence, and a target exit status is not a credential-writeback receipt.
The skill contains a conditional workspace example; [Configuration](configuration.md#files-and-explicit-host-access) owns the mount contract.

## Install the reusable skill

The canonical source is `.agents/skills/underwrap/`, with standard `SKILL.md` frontmatter and no external repository-relative dependencies.
The flake exposes `lib.agentSkills.underwrap` as a system-independent source directory.
A downstream Home Manager module receiving the `underwrap` flake input can materialize it in a harness's global skill directory:

```nix
{ underwrap, ... }: {
  home.file.".pi/agent/skills/underwrap".source =
    underwrap.lib.agentSkills.underwrap;
}
```

Choose the destination expected by the harness or profile; the content is not Pi-specific.
For example, the same source can be installed at `.claude/skills/underwrap` instead.
Avoid duplicate installation into multiple directories the same harness scans, and review the skill before enabling it.
Expose the input through the downstream module's normal argument wiring; this repository does not modify another project's global configuration.

For package-based consumers, `packages.<system>.underwrap-agent-skill` has `SKILL.md` at its root and includes the project licenses:

```sh
nix build .#underwrap-agent-skill --no-link --print-out-paths
```

This skill package does not build the Rust workspace or install/activate the frontend.
`packages.<system>.underwrap-info` separately provides the helper binary, but a generated host catalog is still required for discovery.
Pin and update the skill, package and NixOS module together; a skill alone cannot add wrappers or authorize their use.
