# Operations

This guide owns backend deployment, browser administration, enrollment, maintenance, and recovery.
For a first local deployment use [Getting started](getting-started.md); for data and policy contracts use [Backend](backend.md).
Underwrap is pre-release security software without an independent end-to-end audit.
Operator custody, deployment coordination, and target trust are prerequisites, not capabilities supplied by the UI.

## Deploy the backend

The backend runs on Linux; the privileged frontend is supported through the NixOS module.
A central deployment has a backend host, NixOS frontend hosts making outbound HTTPS connections, and operator browsers connecting to the backend's HTTPS origin.
Choose the hostname, port, server trust, service identity, state paths, store kinds, and runtime client mappings before installation.
Restrict network exposure to the intended frontend and operator networks.
The VM tests use a single host and loopback connections, not proof of separate-host DNS, firewall, routing, or partition behavior.

The `underwrap-backend` flake package embeds the built SPA.
Source builds require SPA preparation first; follow [Contributing](../CONTRIBUTING.md) rather than deploying an unprepared checkout.
The frontend NixOS module does not install a production backend service.

Run the backend under a dedicated account, conventionally `underwrap-backend`, that owns its registry and data directories and can read the server key.
Use explicit paths from [Configuration](configuration.md#backend-yaml), create the configuration directory before writing YAML, and keep configuration root-controlled.
A typical layout is root-owned `/etc/underwrap-backend/` with service-group read access and private service-owned `/var/lib/underwrap-backend/` containing `stores/`, `clients/`, `users/`, `bootstrap.json`, and `data/`.
The tutorial provides concrete directory and TLS installation commands.
Do not put private keys, brokered values, or master keys in Nix-generated configuration.

Supply a deployment-reviewed service unit that runs `underwrap-backend --config /etc/underwrap-backend/config.yaml` and applies a restrictive umask, `LimitCORE=0`, no capabilities, `NoNewPrivileges`, `PrivateTmp`, `ProtectHome`, and `ProtectSystem=strict` with only the required writable state paths.
Review address-family and syscall restrictions against that deployment rather than copying an untested universal sandbox.
The application does not enforce TLS-key modes or the user-directory mode, so service ownership and permissions are load-bearing.
Bootstrap before exposing the listener.

### Server TLS

The server certificate must cover every hostname or IP used by browsers and daemons in its SANs.
Install the chain and matching private key at the YAML paths, the public CA certificate at each frontend's `backend.caFile` when using private PKI, and trust the CA in operator browsers.
The backend does not need the CA signing key.
Keep signing-key custody separate, removing it from the backend host when not needed there.

For a disposable demonstration, the following Smallstep CLI recipe runs in a new private directory on a trusted issuance host.
It creates unencrypted key files, so do not use it as a production key-custody policy; `step` is available as Nix package `step-cli` and is installed by the tutorial.
Do not reuse an existing directory or overwrite existing keys.

```bash
umask 077
mkdir -m 0700 underwrap-pki
cd underwrap-pki
step certificate create \
  underwrap-backend-ca ca.pem ca.key \
  --profile root-ca --insecure --no-password
step certificate create \
  backend.example.internal server.pem server.key \
  --profile leaf --ca ca.pem --ca-key ca.key \
  --san backend.example.internal --san 127.0.0.1 \
  --not-after 8760h --insecure --no-password
cd ..
```

A configured frontend CA bundle is its complete server trust set and excludes bundled WebPKI roots.
Omit `backend.caFile` for a server verified through bundled public roots.
This server verification is distinct from runtime client identity: the backend requires TLS private-key possession and an active enrolled leaf fingerprint, not client certificate subject, SAN, chain, or validity dates as the principal.
Browsers use operator login, not runtime client certificates.

Use direct backend-owned TLS or Layer-4 TLS passthrough.
An HTTPS Layer-7 proxy URL can pass endpoint validation, but ordinary TLS termination does not preserve the runtime peer certificate on the backend's own connection.
There is no trusted forwarded-certificate header, proxy identity, plaintext exception, or backend Unix-socket transport.

### First operator bootstrap

`bootstrap-user` is offline backend-host administration, not a network setup endpoint.
The browser bootstrap page gives instructions and accepts no initial credentials.
Stop the backend listener and every other user-registry writer, including other offline commands, and use the active configuration and backend service identity.
Bootstrap requires an empty operator collection and absent sentinel.

The following helper uses Python 3 to read and confirm a password without terminal echo and supplies one line on the backend CLI's stdin.
It turns getpass's echoing fallback warning into an error, so a terminal without hidden input is rejected.
The backend CLI itself reads a normal stdin line and has no hidden-input prompt.
Run this in a trusted interactive terminal without tracing or session recording; replace the non-secret service identity, config path, and username as appropriate.

```bash
sudo -u underwrap-backend python3 -c '
import getpass
import os
import subprocess
import warnings

os.umask(0o077)
warnings.simplefilter("error", getpass.GetPassWarning)
password = getpass.getpass("New operator password: ")
confirmation = getpass.getpass("Confirm password: ")
if not password or password != confirmation or "\n" in password or "\r" in password:
    raise SystemExit("Password is empty, differs, or contains a line ending")
subprocess.run(
    ["underwrap-backend", "--config", "/etc/underwrap-backend/config.yaml",
     "admin", "bootstrap-user", "--username", "alice"],
    input=password + "\n", text=True, encoding="utf-8", check=True,
)
'
```

A trusted password manager supplying exactly one line to stdin is another option.
Never put the password in argv, exported environment, YAML, shell history, logs, or an ordinary temporary file.
Python and the backend hold transient plaintext; this procedure is not a memory-erasure guarantee.

The stable advisory bootstrap lock coordinates participating bootstrap processes, not every trusted filesystem writer.
Bootstrap persists the user before the sentinel and does not roll back a valid user if sentinel persistence fails.
If the command reports that partial outcome, preserve the user and investigate the sentinel-path failure rather than deleting first-use state or repeatedly bootstrapping.
After successful provisioning, start the service and verify ordinary browser login.
All authenticated operators have full administrative authority; there are no roles or per-store operator grants.

### Recover total operator login loss

Recovery is distinct from first-use bootstrap and requires backend-host filesystem authority.

1. Identify the active configuration and deployed service identity, stop the backend, and verify that its HTTPS listener and every user-registry writer are stopped.
2. Choose a distinct new username after trimming and lowercasing; recovery refuses an existing normalized name rather than overwriting it.
3. Use the same safe stdin helper above, changing only `bootstrap-user` to `recover-user` and the username to the new recovery name.
4. Require command success before restarting the backend, then sign in as the recovery user through ordinary HTTPS login.
5. Use **Users** to repair or create the intended account, sign out, and verify that another intended operator can sign in.
6. Delete the temporary recovery user only after another account is verified; the last-user guard prevents removing the final operator.

Recovery adds a password-verifier record without reading or changing the bootstrap sentinel.
Do not delete the sentinel or user registry, or generate and edit password hashes manually, as a shortcut.

## Runtime client identities

Each frontend instance uses one private combined certificate-chain-and-key PEM at `/var/lib/underwrap-<instance>/client-<instance>.identity.pem`.
The frontend module creates its daemon user/group `underwrap-<instance>` and private state directory; do not confuse the full daemon config with the entry sidecar.
Generate on the frontend:

```text
sudo underwrap-keygen --instance-config /etc/underwrap/daemon/<instance>.yaml
```

Keygen prepares one mode-`0400` same-directory file, applies ownership, syncs it, and atomically replaces the identity before printing only the public certificate.
Config-derived daemon ownership takes precedence; absent an ordinary local-account match, ownership falls back to an existing identity and then the invoking identity.
Account lookup errors are not ordinary-not-found fallback, and nonstandard ownership must be checked before use.
The daemon rejects a present identity owned by another UID or wider than `0600`; when parent metadata is available it also checks owner and mode no wider than `0700`.

A failure before replacement preserves the old complete identity.
A stdout failure after replacement can report command failure although the new identity is installed; do not assume rollback or enroll unrelated output.
Resolve which public certificate belongs to the installed identity through trusted tooling, or deliberately repeat generation and complete enrollment of that successful output.
Never print or transfer the combined private identity to obtain its public portion.
Installation and enrollment are not one transaction, and there is no parent-directory fsync, power-loss durability, or secure-media-erasure guarantee.
See [Security](security.md) for the identity boundary and implementation qualifications.

### Enroll an instance

1. Create the target store first and ensure an encrypted store is unlocked.
2. Generate the frontend identity and create a **Client instances** record with its public certificate, display name, selected store, `Active` status, and auto-create choice.
3. If auto-create is off, create each configured resource and wrapper/resource pair manually.
4. Restart `underwrapd-<instance>.service` on the frontend after enrollment.
5. Verify the configured pairs and permission rows; startup registration seeds `Locked` rows, not usable access.
6. Add resource values and explicitly save the intended permission mode and writable flag before testing a narrow wrapper.

Auto-create allows startup registration to add missing resources and pairs; registration is upsert-only and does not remove stale configuration assets or rewrite existing provenance.
A daemon can start without an identity and retry backend-client initialization on requests, but that does not repeat startup registration.
The restart remains mandatory after enrollment.
The **Get a wrapper working** page connects these steps and branches on the explicit auto-create choice; it does not create permissions or run host commands automatically.
Backend display names are not frontend service-instance names: use the actual configured `<instance>` when restarting its daemon.
Enrollment, inventory counts, and usage timestamps do not establish successful representative target behavior.

### Client-wide access controls

In **Wrapper permissions**, select the store and use **Client access** directly beneath the intended runtime client instance's column heading.
Choose **Individual settings**, **Lock all**, or **Unlock all** there.
Controls remain available for mapped instances when there are no visible pairs or the store's permission contents are unavailable; an override does not unlock an encrypted store.
**Client Instances** owns enrollment, identity status and mapping, with links to the mapped store's Wrapper permissions page.
Changes apply through the backend immediately on subsequent opens/finalizes, without Save or daemon restart; only entering Unlock all requires confirmation.
Unlock all starts read-only and exposes **Allow files/libsecret writeback** for immediate writable-flag changes.
It includes every registered pair in the mapped store, including missing or denying ordinary rows, but never supplies timed unsafe-debug approval.
Individual rows remain saved; returning to Individual settings restores their effect.
See [Backend](backend.md#client-wide-override) for the complete override and writeback contract.

Controls retain the last acknowledged setting while a request is pending and reconcile a failed update through a fresh client-list read.
If reconciliation fails, the UI labels state unknown and requires refresh before another change rather than optimistically presenting a grant.
These controls do not stop running targets, recall delivered values, drain operations, or replace maintenance coordination.
Remapping carries the override to the new store; review the displayed old/new store identities, broad-access consequences, and best-effort old-permission cleanup before Save.

### Replace an instance certificate

1. Successfully generate and install a new combined identity on the frontend with the same keygen command.
2. Edit the same backend runtime client instance, paste the successful public certificate output, and select **Save**.
3. Run `sudo systemctl restart underwrapd-<instance>.service` on the frontend.
4. Verify a representative authorized wrapper.

The running daemon caches its old identity until restart.
After **Save**, the backend rejects that old identity, so enrollment-guidance failures before restart are an expected interruption.
This is not live reload or a transactional cutover.

Disabling or deleting a client prevents its certificate from authenticating; remapping changes its selected store.
Permission cleanup in the old store happens after the mapping or deletion commit and is best effort.
A successful response does not certify cleanup: inspect stale permission rows under serialized administration before reusing or remapping principals.
Cleanup failure does not undo the new mapping or grant the remapped client access through its old mapping.

## Stores and key custody

The **Stores** page uses server-provided lifecycle and action fields.
`ready` means a plain backend is available, `unlocked` an encrypted backend is available, and `locked` an encrypted backend has no installed key.
`busy` means migration owns lifecycle publication; `recovery-required` means trusted-host migration recovery is required.
Locked, busy, and recovery-required contents are unknown, not proven empty; a false contents boolean is not an inspection result.
Store names link to their resources and permissions; mapped-client counts open an explicit client filter for remapping and deletion blockers.
Rare migration, rotation, and deletion actions are grouped under **Maintenance**.
Locked-store guidance opens Unlock for its exact store id; recovery-required guidance points to trusted-host work, not automatic recovery.

### Create or migrate to encrypted storage

Save the key before submitting the mutation, not after creating or migrating the store.
The browser first obtains a fresh backend-generated key without mutating storage, holds it in transient component state, and requires exact untrimmed re-entry.

1. Copy the displayed key to a separately controlled external password manager or vault, recording the store and intended generation.
2. Verify the external copy and re-enter the exact 64-character lowercase hexadecimal value.
3. Submit only after the comparison succeeds; successful create or migration does not reveal another copy.
4. Verify the store by its stable id and perform a controlled lock/unlock and representative read before treating custody as independently recoverable.

Plain creation accepts no key.
Plain-to-encrypted migration requires the same exact key as encrypted creation; encrypted-to-plain migration requires an unlocked source and neither generates nor accepts a key.
Explicit JSON `key_hex: null` is invalid rather than equivalent to omission; only unlock tolerates surrounding key whitespace.

Mismatch or cancellation before submission sends no mutation request.
An error or lost response after submission is ambiguous: retain the same key and refresh authoritative store state before deciding whether to retry.
For migration, same-key retry is eligible only while the unchanged store id still identifies the original source backend in an eligible state.
A committed target, missing row, busy, locked, or recovery-required source must be reconciled instead.
Discarding an ambiguously retained key requires explicit acknowledgement that it may protect committed or recoverable data.

No-cache responses and avoidance of application query/mutation caches do not prove external custody or erasure from browser internals, clipboard, allocator memory, screenshots, traces, or failure artifacts.
Exact re-entry is one browser comparison, not an independent recovery test.
Do not record real key dialogs or attach them to reports.

### Restart, lock, and unlock

Every backend restart destroys browser sessions and starts encrypted stores locked.
Sign in again, unlock each required store with its externally held key, and verify representative reads before resuming traffic.
Lock drops the registry's installed backend reference; admitted operations can retain shared ownership, and the master key zeroizes only when its final reference drops.
Lock is neither immediate erasure of every key copy nor an operation-drain mechanism.

### Migrate a store

First establish [maintenance quiescence](#maintenance-and-concurrency), then choose the migration action on the selected store.
Migration preserves its stable id, metadata, runtime client mappings, storage ids, control documents, resources, and logical bytes while changing backend kind and canary treatment.
Plain-to-encrypted success leaves the store unlocked; encrypted-to-plain success leaves it ready.

Migration publishes `busy`, blocks new storage leases, and waits up to 30 seconds for admitted complete operations to drain.
It does not prevent an invocation opened earlier from finalizing later, so quiescence is still required.
A drainage timeout returns `503 store_busy` with the source unchanged.
No registry lock spans drainage or filesystem work, and the post-drain copy/publication window has no asynchronous cancellation point.
See [Backend](backend.md) for lifecycle coordination and bounded source-fingerprint verification.

The exact sibling workspace is `.<store-id>.underwrap-migration-<16-lowercase-hex-id>/`, containing a non-secret versioned manifest and `stage/`, then possibly `backup/`.
Workspaces and new staged directories must be non-symlink directories with exact mode `0700`; backup inherits the source mode beneath the private workspace, and mode drift requires recovery.
Plain-to-encrypted failure can retain plaintext in `backup/`; encrypted-to-plain failure can retain plaintext in `stage/` or the canonical path.
Successful plain-to-encrypted migration removes all intentional plaintext migration paths before success, not recoverable media blocks.

The ordinary [migration phase errors](backend.md#migration) require verified source and record restoration plus complete secret-bearing workspace cleanup.
Failure to establish source authority, including fingerprint capture or revalidation failure before displacement, is not merely an ordinary copy-read error.
Ambiguous split state, source mismatch, failed rollback, failed cleanup, or an exact workspace found at startup requires `migration_recovery_required` and preserves remaining artifacts.
Do not repeatedly retry or delete the marker workspace to clear this state.

### Rotate an encrypted key

Rotation creates its replacement key during mutation and returns it once afterward, unlike create and migration's pre-submission custody flow.
Quiesce all access first: rotation serializes store lifecycle operations but does not drain ordinary I/O, so concurrent reads can fail and writes can be lost.
Save the returned key immediately in external generation-specific custody and independently verify the later key before any old-generation cleanup.
Response success or a browser acknowledgement is not proof of custody.
The pending dialog disables dismissal and blocks in-app navigation, with a best-effort unload prompt; crashes, forced closure, and lost responses remain possible.
A known pre-effect rejection requires renewed acknowledgement before retry.
An ambiguous or malformed response disables retry in that dialog and directs the operator to trusted-host reconciliation; store metadata cannot establish key delivery.
Do not reopen or retry rotation after an unknown outcome until generation state and external key custody are reconciled.

Each rotation creates a unique mode-`0700` sibling `.<store-id>.underwrap-rotation-<16-lowercase-hex-id>/`.
Before swap it holds `stage/`; successful rotation leaves the complete old generation in `backup/` and the new generation at the canonical path.
Successful rollback can leave an unpublished new `stage/`; failed rollback can leave `backup/` and `stage/`, or `backup/` and a promoted canonical generation.
Later rotation, restart, resource deletion, and store deletion never automatically select, overwrite, or remove these retained workspaces.
They cost unbounded disk space and remain decryptable with their corresponding old keys, including stale resource values.
A lost key response or `rotation_recovery_required` needs [stopped-host generation recovery](#rotation-recovery-and-selective-cleanup), not a claim that the new key was delivered.

## Resources and permissions

In **Secrets**, select the store, create or rename a resource, then edit its env, files, or structured libsecret entries.
Resource ids are stable across a rename, but frontend configuration names must still match the intended resource name.
In **Wrapper permissions**, create its wrapper/resource pair and edit the exact runtime client instance's permission row.
The backend checks references in order for serialized requests; disabled clients remain valid references.
These checks are separate from later writes, not transactions or automatic graph repair.

### Browser selection and text editing

An unresolved or failed store list is not an empty list.
The requested URL selection becomes effective only after validation against the authoritative store list; stale or removed selections are canonicalized before store-detail queries proceed.
Changing the effective store resets store-scoped dialogs and nested editing state rather than carrying a mutation into a different store.
Store selection preserves unrelated URL state and follows browser back/forward navigation; these presentation rules do not make a requested URL authoritative.
Navigation retains store context and highlights the active section, while Stores and Client Instances remain global inventories unless explicitly filtered.
Store-scoped creation intents do not follow an invalid selection into a different store.
Wait for the selected store to resolve before editing; URL state is not backend authorization.
Metadata filters match resource names/ids, env keys, file paths, wrappers, and runtime client metadata without revealing values or fetching unopened Secret Service trees.
A resource with no env/files entries is not described as wholly empty merely because its libsecret tree has not been opened.

The browser secret editor is text-only and encodes exact UTF-8 without Unicode normalization or intentional line-ending conversion, using chunk-safe transport encoding.
Missing data, empty bytes, malformed base64, and non-UTF-8 bytes are distinct states.
Empty text is valid and copyable, and successful reveal/copy preserves a leading U+FEFF (UTF-8 BOM), including a BOM-only value.
Malformed or non-UTF-8 values are not revealed or copied, and the UI has no binary editor or download mode.
Backend storage still accepts arbitrary secret bytes.
For env/files, **Create** rejects a known same-slot key collision, **Rename** preserves material without fetching it, and **Replace value** pins the existing key and requires new material.
An intentional empty write requires **Store an empty value**; leaving a value field blank alone is not replacement, and Delete remains separate.
Known-key collision checks prevent accidental overwrite in the displayed inventory, not concurrent changes or last-writer-wins races.

For a new libsecret item, leave its optional id blank for a generated 32-character lowercase hexadecimal id; a custom id must have that exact shape.
Collection ids and alias names accept 1–128 ASCII letters, digits or underscores, including a leading underscore; use labels for descriptive text.
Old incompatible identifiers remain visible and existing read/delete routes remain available for repair, but new items or aliases cannot extend an incompatible collection.
These admission checks do not change the separate [publication size and graph requirements](local-protocol.md#framing-and-state-validation).

Libsecret attributes accept one `key=value` per nonblank line, split at the first delimiter, trim both sides, allow empty values, and keep keys case-sensitive.
Blank lines and CRLF input are handled without creating extra attributes.
A missing delimiter, empty trimmed key, or repeated exact trimmed key produces a line-numbered error and sends no request; duplicate errors also identify the original line.
Names such as `__proto__` and `constructor` remain ordinary own keys.
This is browser textarea validation, not a new restriction on backend or D-Bus attribute maps.

### Permission modes and writeback

The [permission matrix and client override](backend.md#permission-model) define admission and effective writability.
Cells in **Wrapper permissions** show saved individual modes and writable flags; column headers contain client-wide controls and identify unavailable client metadata.
Overridden saved settings use neutral badges, dashed outlines and explicit override labels, rather than disappearing or becoming uneditable.
Under Unlock all, unsafe-debug badges retain their warning/expiry colors because the separate timed approval still applies; ordinary admission and the saved writable flag are overridden.
An uncertain update is labelled separately rather than presenting the previous override as confirmed.
An absent row is distinguished from an explicit Locked row without changing Individual settings' default deny.
Saving or removing an individual row does not remove a client override, and opposing Save/Remove requests cannot overlap within the dialog.
The backend rechecks current settings, variant, and expiry on both open and finalize; expiry is lazy, not a background sweep or an approval prompt.
The effective writable flag allows files/libsecret persistence, not target disclosure or admin writes.
The target-facing surfaces stay writable even when persistence is disabled.
Env remains operator-owned, but finalize's loaded-snapshot comparison is not compare-and-swap and cannot protect against a racing admin edit between load and whole-blob save.
Serialize mutations as described below.

### Unsafe debug

`UnsafeDebugUntil` gives the daemon-selected debug tool the complete configured env/files/libsecret projection for the exact runtime client instance and wrapper/resource pair.
It can show environment information, run an interactive shell, or execute arbitrary commands that print, persist, or transmit that projection.
Any local user can reach the system-wide entry while this tuple is authorized; the UI does not know which frontend instance name supplies it.
Verify that deployment before enabling debug, and use an OS-level gate if local-user restriction or a prompt is required.
Writable false does not reduce disclosure; writable true additionally permits future-state changes through accepted files/libsecret finalize.

The confirmation proposes an exact local expiry with a 15-minute default; **Save** submits that selection, while confirmation alone leaves existing backend permissions and overrides unchanged.
Use the shortest practical interval and return to a safer mode immediately after the incident task.
Do not use debug as routine presence verification.

## Maintenance and concurrency

There is no universal quiesce button or shipped maintenance coordinator.
Before migration, rotation, backup, deletion, or overlapping resource/policy changes, identify every frontend instance mapped to the store, automation scheduler, writable target, admin browser, API writer, and trusted offline writer.
Agree which single operator retains administration during the task.

For a deployment under your control, pause its callers and schedulers, block new invocation entry through the deployment's existing OS-level controls, and wait for active wrapper invocations to finish.
Stopping frontend daemon services can help prevent new admissions, but first account for active targets, interrupted finalization, and other hosts; it is not by itself proof of drainage or rollback.
Verify the selected store has no remaining participating writers before beginning.
Locking permission rows is a denial control, not a wait mechanism, and can cause in-flight writeback rejection.
Do not re-enable traffic until resulting state has been observed.

Migration and rotation need the backend running for their admin operations, so retain its listener only for the designated operator while other access is quiesced through deployment-specific controls.
Offline account recovery, filesystem generation recovery, and a consistent offline backup require the backend stopped as well.
For upgrades, prepare reviewed artifacts, establish this boundary, record non-secret configuration/version metadata, stop the service, take the chosen backup, deploy, restart, unlock encrypted stores, and verify representative operations before resuming traffic.
Do not infer downgrade compatibility from the ability to reinstall an older binary.

Atomic replacement makes one stored value visible whole; it does not make multiple document edits transactional.
Concurrent runtime finalize, admin edits, registration, resource/pair/permission deletion, and client remapping can lose updates or create stale references.
Store-collection serialization and short live-registry publication locks differ from client-registry persistence locking; neither is a global coordination mechanism.
Client remap/delete cleanup is postcommit best effort, and failed store creation can leave backing directories or a canary before registry commit.
There is no CAS, tombstone, global post-delete exclusion, automatic retry, repair, or garbage collector.
Use authoritative rereads and serialized reconciliation rather than assuming an error means no effect.

### Backup scope

Underwrap ships no universal backup/restore executable or verified production restore procedure.
Use a deployment-reviewed backup system under a stopped-writer boundary, and validate a restore in isolation before depending on it.
Include the selected canonical store directories with control documents and resource data, store and client registries, operator records, bootstrap sentinel and its retained lock file, active YAML, and ownership/mode metadata.
Include all store directories actually referenced by records, not just the configured root for new stores.
Do not remove the stable lock inode from a running deployment.

Inventory migration and rotation siblings separately: they are outside the active APIs and may contain plaintext or old-key-readable ciphertext.
Do not omit or delete potentially recoverable generations simply because an API lists only the canonical store.
Protect copied password verifiers against offline guessing and plaintext data against disclosure.
Keep backend TLS identity, CA signing-key custody, and frontend private runtime identities in separately controlled backup domains appropriate to their distinct impersonation powers.
Do not copy those credentials into resource stores.

Maintain an external key-to-generation inventory and independent access to every key required by retained encrypted backups; ciphertext without its matching key is not a recoverable backup.
Keys must not be stored beside the data they protect.
In-memory sessions and unlocked-key state are not restored by copying disk files; a restarted backend needs login and encrypted-store unlock.
Atomic file replacement and file sync do not imply parent-directory fsync, power-loss durability, or secure erasure.

### Decommissioning

1. Pause callers and serialize all writers, then lock relevant permission rows and disable the departing runtime client instance to stop future use.
2. Stop its frontend daemon and account for in-flight targets and finalization before removing frontend configuration and rebuilding.
3. Inspect and remove permission rows, then dependent pairs, then unwanted resource contents before deleting a resource; include aliases and empty libsecret collections.
4. Remove or remap clients before store deletion and inspect best-effort stale-row cleanup explicitly.
5. Decide retention and external key custody before removing any canonical directory, private frontend identity, TLS key, backup, or recovery sibling through trusted-host controls.

Resource deletion is blocked by every typed env/file/libsecret entry, alias, collection including empty collections, item, pair, and permission row.
Blocker samples contain identifiers rather than values, but identifiers can themselves be sensitive and must not be copied into logs or reports.
Once unblocked, deletion removes the active empty key before catalog removal; after any error reread both rather than reconstructing a blob based on assumed rollback.

Store deletion removes the store registry record, not YAML configuration, its data directory or retained rotation siblings.
Force-delete cleanup guidance names the absolute data pathname and supplies a POSIX-shell command with one literal quoted operand and an option terminator.
Relative record paths are anchored to the backend process working directory without requiring the directory to exist, resolving symlinks, or normalizing `.` and `..` components.
If that pathname is empty or cannot be resolved or represented exactly as UTF-8 without NUL, deletion fails before removing the registry record rather than returning lossy guidance.
Verify the selected path on the backend host and decide backup, retention and key custody before manually running any removal command.
The suggested command covers only that data pathname, not retained generations or external backups, and removal is not secure erasure.
Mapped clients block deletion; unavailable contents are not known empty, and a locked store requires explicit force where the server permits it.
Force is not erasure or migration recovery.
Disabling auto-create and removing unused frontend definitions prevents a later startup registration from recreating assets.
Revocation and unlinking do not erase retained generations, external backups, target-written copies, or media blocks.

## Recovery on the backend host

Underwrap ships neither a generation inspector nor an automatic selector, promoter, or cleanup command.
The procedures below require qualified trusted-host work and, where exhaustive verification is required, offline tooling built against the matching release's `StorageBackend` implementations.
If you cannot obtain and validate that tooling or identify a candidate's matching key, stop and preserve state rather than substitute a canary-only check or guessed shell recovery.
There is no operated recovery-support commitment or response SLA.

### Migration recovery

Use this only for an interrupted migration, `migration_recovery_required`, or startup detection of an exact migration workspace.

1. Stop the backend and every process accessing the selected store, registry, canonical path, or migration siblings, and verify the listener and writers are stopped.
2. Inventory the persisted store record, canonical path, and every exact `.<store-id>.underwrap-migration-<16-lowercase-hex-id>` sibling without deletion, overwrite, symlink traversal, or normalizing near-matches into that namespace.
3. Preserve every canonical, `stage/`, and `backup/` generation and validate the non-secret manifest when possible; a missing or malformed manifest is recovery state, not authority.
4. Reject symlink and non-directory generation roots; require exact mode `0700` for migration workspaces and new stage roots, preserve source/backup metadata, and use fresh occupancy-checked non-overwriting quarantine names beneath private mode-`0700` parents.
5. With matching-release offline backend tooling, establish each candidate's backend kind from the valid manifest, persisted record, and source/target role rather than pathname alone; open it with the matching external key if encrypted, verify the encrypted canary or plain canary absence, list its complete id set, and read every logical value.
6. Select a fully verified generation whose control documents, resources, and logical bytes match the intended source or target state, stopping without guessing if none can be verified.
7. Quarantine any unselected canonical generation under a fresh private sibling and promote the selected generation with checked non-overwriting same-filesystem renames.
8. Move every remaining exact migration workspace to a fresh nonmatching mode-`0700` quarantine sibling while preserving all contents, and verify no exact workspace remains to trigger startup detection.
9. Atomically replace the existing store record, keeping its file stem, name, description, creation time, and canonical directory unchanged and changing only the backend kind to match the selected generation.
10. Restart, unlock if encrypted, and verify the stable store projection, representative control/resource reads, runtime client mappings, and a representative authorized runtime read.
11. On any failure stop again and preserve all canonical, quarantined, and workspace state before considering another previously verified candidate.
12. Remove only explicitly selected obsolete migration artifacts after successful verification, leaving unrelated retained rotation workspaces untouched.

Source-fingerprint checks and direct per-value comparisons in normal migration protect rollback decisions, but do not provide a shipped offline inspector or prove collision impossibility against a malicious trusted host.
Cleanup may already have partially removed an obsolete generation before reporting failure.
Recovery cannot restore missing media, arbitrary corruption, an unavailable generation key, or writes never flushed before power loss; it offers no transaction, CAS, parent-directory fsync, or secure-erasure guarantee.

### Rotation recovery and selective cleanup

Keep all access quiesced and the backend stopped while selecting or moving generations.
Do not confuse rotation's indefinitely retained siblings with migration's startup-blocking namespace.

1. Enumerate only exact `.<store-id>.underwrap-rotation-<16-lowercase-hex-id>` workspaces and identify a candidate `backup/` corresponding to an externally held key.
2. Preserve canonical, stage, and backup generations; reject symlink or non-directory candidates and use matching-release offline tooling to open and exhaustively read the selected encrypted generation, including its canary, complete id set, resources, and control documents, before trusting its completeness.
3. Choose a fresh private sibling for an existing canonical generation; destinations must satisfy both `test ! -e PATH` and `test ! -L PATH`, because a dangling symlink is occupied.
4. Refuse a canonical symlink, including a dangling one; if canonical exists require a directory and preserve it with a checked non-overwriting no-target-directory move, otherwise leave it absent rather than creating an empty store.
5. Recheck canonical is unoccupied and promote the verified backup with the same checked same-filesystem move, verifying source absence and destination-directory presence afterward; never delete or overwrite a path to make room.
6. Start the backend, unlock the selected generation with its key, and verify the canary plus representative resource/control reads and intended runtime behavior through ordinary APIs.
7. If verification fails, stop again and preserve every path before restoring or trying another previously verified candidate.

A suitable move primitive is `mv --no-clobber --no-target-directory SOURCE DESTINATION`, but its exit status alone is insufficient: recheck the postconditions and occupied paths under stopped-writer authority.
There is no automatic generation selection or promotion, and a missing canonical directory must not be replaced by an empty generation.

Cleanup is separate from recovery.
First verify a later active key through lock or restart, unlock, canary, and representative resource/control reads.
Then stop the backend, re-enumerate exact paths, delete only explicitly selected obsolete workspaces or preserved generations, and confirm canonical and all other retained workspaces are unchanged before restarting.
Do not infer safe cleanup from HTTP 200 or a browser checkbox.
No procedure promises recovery from a missing key, media loss, arbitrary corruption, partial prior cleanup, or power loss, nor coordinates unquiesced writers or guarantees response delivery.

### Invalid stored environment recovery

An outside-contract resource containing a forced HOME/XDG env name blocks env projection and ordinary per-key deletion; content blockers also prevent ordinary resource deletion.
There is no normal admin repair or migration operation for this state.
Quiesce the backend and all writers, then use trusted backend-host access and appropriate matching-backend tooling to restore or replace the affected resource from known-good state, or rebuild the affected store from known-good inputs.
Preserve potentially recoverable material and keys before intervention.
Do not weaken env-name validation or delete catalog records as a shortcut.

## Diagnose failures without exposing values

### Connection and authorization diagnosis

On the frontend, inspect service state and identifier-only diagnostics with `systemctl status underwrapd-<instance>.service` and `journalctl -u underwrapd-<instance>.service`.
Check configured URL, server SAN, trust bundle, and reachability before inspecting enrollment.
For a private-CA hostname endpoint, a verification-sensitive TLS probe is:

```bash
openssl s_client -brief \
  -connect backend.example.internal:18443 \
  -servername backend.example.internal \
  -verify_hostname backend.example.internal \
  -verify_return_error \
  -CAfile /etc/underwrap/ca.pem </dev/null
```

`-servername` selects SNI; it does not verify the hostname.
For the tutorial's IP endpoint retain `-brief` and use `-connect 127.0.0.1:18443 -verify_ip 127.0.0.1 -verify_return_error -CAfile /etc/underwrap/ca.pem` instead.
This checks server TLS, not enrollment, runtime authorization, or persistence; it needs no private runtime identity.
Do not disable verification to work around a mismatch.

For enrollment errors, verify identity existence and metadata without opening it, the active backend client fingerprint, selected store, and the required daemon restart after enrollment or replacement.
For missing assets, verify names and auto-create policy and restart for startup registration, or create missing resources/pairs manually.
For locked or expired access, check the client override, exact permission row, and encrypted-store state.
`client-locked` identifies Lock all; changing an individual row does not remove that override.
For effective-GID mismatch, use the root-installed setgid entry point instead of connecting directly to the socket.
Do not weaken peer authentication or edit the target-UID table manually.

### Writeback diagnosis

A successful target exit, local Secret Service ACK, or changed `last_used_at` is not a backend persistence receipt.
Eligible nonzero and signal exits can finalize too, while collection or finalize errors remain diagnostic and preserve the target's exit/signal status.
Underwrap does not expose a separate machine-visible finalize result to the caller.

1. Confirm the exact resource, enabled projections, initial writable hint, and backend client override, permission mode, expiry, and effective writable flag at finalize time.
2. Check namespace lifecycle, collection, publication, and size diagnostics before assuming a network failure; an oversized final resource suppresses both Checkin and UsageSignal.
3. Inspect `writeback_rejected` reason metadata: `permission_readonly`, `server_rejected`, or `resource_too_large`.
4. Inspect daemon checkout/checkin/usage and backend decision diagnostics, accounting for a refused Checkin followed by UsageSignal rather than assuming one terminal verb.
5. Quiesce competing writers and perform an authorized backend read or a purpose-built presence/equality check against expected non-secret metadata to establish the resulting state without printing material.

Files and libsecret stay writable locally even when changes are discarded, and a permission change during invocation can reject a submitted writeback.
Publication ACK means host retention, not caller receipt or backend durability.
A timeout or cancellation releases local capacity but does not prove that a backend effect did not commit.
The daemon's bounded [runtime-client scheduling](architecture.md#runtime-client-scheduling) does not promise automatic retry, ordered independent invocations, or exactly-once delivery.
Do not blindly rerun a mutating target after an ambiguous outcome.
See [Local protocol](local-protocol.md) for lifecycle and publication eligibility and [Backend](backend.md) for finalize semantics.

### Logs and reporting

Daemon audit events are human-readable tracing on stdout, normally collected by the service manager, not a JSON audit stream or durable ledger.
`RUST_LOG` controls filtering and defaults to `info` when absent or invalid; there is no daemon-configured audit file or journal sink.

| Event | Level | Fields |
| --- | --- | --- |
| `read_allowed` | INFO | `wrapper`, backend resource id `resource`, `peer_uid` |
| `read_denied` | WARN | `wrapper`, `reason`, `peer_uid` |
| `auth_denied` | WARN | `socket`, `reason`, `peer_uid`, `peer_gid` |
| `resource_checkout` | INFO | `wrapper`, `resource` |
| `resource_checkin` | INFO | `wrapper`, `resource` |
| `resource_usage_signal` | INFO | `wrapper`, `resource` |
| `resource_checkin_denied` | WARN | `wrapper`, `resource`, `reason` |

Other daemon diagnostics do not extend this audit-helper schema.
Generic backend failures return HTTP 500, code `internal`, and message `the backend could not complete the request`.
Their `admin internal failure` or `runtime internal failure` events contain `response_code`, static operation `context`, and `error_type`, not arbitrary source display text or a unique correlation id.
Detailed backend diagnostics are limited to individually audited source classes with fixed safe public responses, not permission to log arbitrary error chains.

Namespace `writeback_rejected` is a separate native-journal event, with a JSON stderr fallback if the journal socket is unavailable.
`UNDERWRAP_SUPPRESS_READONLY_WARNING=1` suppresses only the read-only human warning and is consumed before the target; it does not suppress audit or the fallback event.
Resource-too-large stderr diagnostics are unsuppressible and contain safe size metadata, not resource values.
Avoid copying identifiers containing sensitive operator input, blocker samples, raw HTTP bodies, keys, credentials, browser captures, or resource content into reports.

For suspected vulnerabilities, follow [Security reporting](../SECURITY.md).
Use [GitHub issues](https://github.com/rschaffar/underwrap/issues) for non-sensitive questions and bug reports.

## Users, sessions, and logout

Use **Users** for operator creation, rename, deletion, or password reset, and **Profile** for the signed-in operator's password change.
All operators are equally privileged, and deleting the last user is blocked.
Passwords persist before submitting-session refresh: if refresh fails after a self-service change, sign in again with the new password.
Other pre-change sessions invalidate lazily on their next admin request; resets and deletion refresh none of the affected sessions.
Backend restart loses all in-memory sessions.

Logout first performs a tolerant whoami request to prime a fresh CSRF token, then posts session revocation.
The UI clears cached admin state and replace-navigates to login only after success.
A priming or logout failure preserves session/cache/URL state and shows a visible, non-overlapping Retry action rather than claiming sign-out.
This is not cross-tab coordination or automatic retry; do not treat closing a tab as confirmed session revocation.
