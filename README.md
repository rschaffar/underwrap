# underwrap

underwrap lets a caller invoke a configured CLI capability without handing that caller the reusable credential.
A backend holds resources and policy; a NixOS frontend projects an authorized resource into a short-lived, trusted target process.

> [!WARNING]
> underwrap is pre-release security software and has not received an independent end-to-end security audit.
> Read the [security model](docs/security.md) before relying on its guarantees.

## When it fits

Use underwrap when a local user, automation process, or AI agent needs a bounded operation rather than possession of a token.
For example, a configured helper can accept a reviewed comment on stdin and post it to one fixed repository:

```sh
printf '%s\n' 'reviewed' | agent-gh-comment 123
```

The helper receives the credential; the caller's shell does not.
Prefer a narrow helper over an unrestricted CLI that can print credentials, execute plugins, or redirect requests.
The [wrapper guide](docs/wrappers.md) explains how to review that boundary.

A resource can contain environment variables, credential files, a private Secret Service keyring, or a combination of these projections.
Files and keyring changes persist only when backend policy permits writeback.

Use a simpler secret manager or ordinary environment injection when the caller is already trusted to possess the secret.
Use application authorization or an OS-level gate such as sudo or polkit when you need per-user access decisions or a human prompt.
Underwrap does not supply per-invocation approval.

## How it works

```text
                backend
        resources · policy · admin UI
                   ▲
                   │ HTTPS + mTLS
                   ▼
              local daemon
                   ▲
                   │ Unix socket
caller ──► instance entry ──► namespace stage ──► trusted target
```

The daemon authenticates the instance entry, requests backend authorization, and chooses the configured target.
The wrapper chain authenticates its daemon handoff, creates private namespaces, switches to a dedicated target UID, projects the resource, and executes the target.
The backend rechecks permission mode and writability before accepting changes.
See [Architecture](docs/architecture.md) for identities, responsibilities, and design rationale.

**The caller is untrusted with respect to the secret; the configured target is trusted with it.**
Underwrap is not a behavioral sandbox: a target can print, persist, or transmit its own projection.
Dedicated target UIDs protect the target's process state from caller-owned processes; root, kernel, backend, and target compromise are outside the claim.

Backend permissions identify the mTLS-authenticated runtime client instance, not the local Unix user.
`allowedUsers` installs convenient PATH shims only; any local user can execute the system-wide instance entry point.

## Platform and operating costs

- **Frontend:** NixOS, using the supplied module and privileged wrapper chain.
- **Backend:** Linux, on the same machine or a separate host.
- **Flake outputs:** `x86_64-linux` and `aarch64-linux`; the deployed VM check is available only on `x86_64-linux`.
- **Default hardened posture:** Linux 6.5+, restricted swap options, and a host-wide Yama setting that restricts unprivileged debugging.
  Read the [configuration requirements](docs/configuration.md) before rebuilding a host.
- **Availability:** wrapper authorization depends on the backend network path.
  Encrypted stores need external master-key custody and manual unlock after backend restart.
- **Persistence:** concurrent content mutations are last-writer-wins, not transactional.
  Maintenance and key operations require coordinated access; see [Operations](docs/operations.md).

## Start here

| Task | Guide |
|---|---|
| Complete a first local deployment | [Getting started](docs/getting-started.md) |
| Deploy, enroll clients, manage access, or recover a store | [Operations](docs/operations.md) |
| Configure NixOS and the backend | [Configuration](docs/configuration.md) |
| Design and review a wrapper | [Wrappers](docs/wrappers.md) |
| Understand trust and limitations | [Security](docs/security.md) |
| Understand resource, policy, and API semantics | [Backend](docs/backend.md) |
| Review privileged handoff and projection mechanisms | [Local protocol](docs/local-protocol.md) |
| Build or contribute | [Contributing](CONTRIBUTING.md) |
| Select validation commands and interpret their coverage | [Testing](docs/testing.md) |

For non-sensitive questions and bug reports, use [GitHub issues](https://github.com/rschaffar/underwrap/issues).
For suspected vulnerabilities, follow the [private reporting procedure](SECURITY.md); do not put secrets or exploit details in a public issue.

## License

Licensed under either [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
Unless explicitly stated otherwise, contributions intentionally submitted for inclusion are dual-licensed on the same terms.
