# RF-0001 — Replaceable staging root can substitute collected files

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

The fixed root `/tmp/.underwrap-ns-directories` and stored absolute paths are defined in `crates/underwrap-ns/src/main.rs:71,365-388` and `crates/underwrap-ns/src/directory_credential.rs:26-40,93-99`.
Mount setup accepts an existing directory and tolerates permission-denied chmod before mounting by pathname (`crates/underwrap-ns/src/mounts.rs:41-79`).
Later materialization, binding and collection resolve names again (`crates/underwrap-ns/src/directory_credential.rs:141-200,255-379`).

Let U be the ordinary caller and X the distinct provisioned target identity.
U must first own the bare staging dentry, normally by creating it while the basename is absent in shared writable/searchable `/tmp`.
A root/X-owned preexisting dentry under sticky `/tmp` is not assumed renameable.
After the private mount, U can rename its underlying bare directory within the same parent and replace the original name.
Sticky-directory ownership permits this; mode 0000 does not transfer ownership or prevent same-parent rename.
Private mount propagation does not privatize underlying dentries.

The strongest source-supported trigger is replacement after normal materialization/binding while the trusted configured target runs.
U supplies searchable directories and readable regular files, for example directories 0755 and files 0644 under the expected `mount-i` layout.
The target's existing bind mounts still select original objects, but later supervisor collection selects the replacement.
No symlink, malicious target, instance-group membership or stolen fd 20 is required.

## Consequence and boundary

The host supervisor retains setup capabilities and collects with filesystem IDs X (`crates/underwrap-ns/src/main.rs:656-668,739-790`; `crates/underwrap-ns/src/target_uid.rs:255-322`).
Other-readable U-owned regular files satisfy DAC and the collector's static checks; collection does not validate mount/inode identity or ownership.
A valid lifecycle can therefore submit substituted files through `crates/underwrap-ns/src/writeback.rs:85-158`.
Persistent injection additionally requires writable Checkout and current finalize permission, a changed result and a complete resource within the size cap.
Backend policy, writability and env checks remain effective (`crates/underwrap-backend/src/runtime_api/invocations.rs:165-246`).

The shipped namespace wrapper is not root and supplies no DAC-override, FOWNER or CHOWN authority (`nix/modules/underwrap.nix:881-904`).
Independent pinned-wrapper inspection found active SYS_ADMIN/SETUID/SETGID, not active SETPCAP after wrapper exec.
Earlier replacement can direct X's creation of its own descendants onto host-backed storage if the replacement root is writable/searchable to X.
Precreating a U-owned `mount-i` is not equivalent: checked chmod can fail without FOWNER.
X-owned 0700 directories/0600 files do not become readable to U merely because U owns their ancestor.
Host-backed storage is not necessarily physical disk.

The module provisions target runtime directories, not protected ownership of these staging dentries (`nix/modules/underwrap.nix:609-682`); daemon `PrivateTmp` at line 234 does not cover caller-launched namespace stages.
The violated scope is caller-to-projection integrity/private storage, not intentional trusted-target disclosure or last-writer-wins.
The similar bus path remains an unverified related concern (`crates/underwrap-ns/src/mounts.rs:112-127`; `crates/underwrap-ns/src/libsecret.rs:224-226,269-270,284-322`).

## Correction and alternatives

Require stable private-filesystem authority across all three phases despite hostile dentry replacement.
An operator-protected mountpoint can mitigate preclaim but is not supplied by the module; ownership checks at one instant alone must not be assumed to solve subsequent relookup.
Review the bus pathname separately without presuming interception.

## Acceptance criteria

- In an authorized disposable NixOS test, use distinct U/X and the shipped wrapper, preclaim the root, then deterministically replace it after binding.
  Compare the target's original view with collected/persisted state; replacement files must not be selected.
- Test pre-materialization replacement separately, checking filesystem identity and ownership rather than equating host visibility with readability.
- Retain normal writable/read-only, size-limit, symlink and lifecycle controls, plus a protected-root negative control.
  No fix acceptance is claimed yet.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
