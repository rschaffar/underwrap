# GitHub via libsecret

GitHub CLI can store credentials through Secret Service instead of receiving a
static `GH_TOKEN`. underwrap provides a private Secret Service for one wrapper
invocation and persists its canonical state in a backend resource.

A clean wrapper design separates credential management from routine actions:

- one writable authentication wrapper;
- one or more read-only action wrappers;
- one shared libsecret resource.

## Why use libsecret

`gh auth login` manages structured credential state rather than one fixed env
value. Later `gh` commands need to discover and read that state through Secret
Service.

Use env projection for a simple fixed token. Use libsecret only when the target
actually speaks Secret Service or a compatible keyring API.

## Invocation flow

For a libsecret-enabled wrapper, `underwrap-ns`:

1. authenticates fixed fd 20 against the selected instance daemon's
   creation-time Unix peer credentials, then checks out the resource;
2. extracts the resource's libsecret section;
3. creates a CLOEXEC private publication socketpair before the namespace fork;
4. creates the private bus tmpfs and starts dbus-daemon;
5. passes the initial canonical section to `underwrap-secretsd` through a
   one-shot pipe;
6. validates and ACKs generation 0 before secretsd readiness and target exec;
7. points the target at the private bus with `DBUS_SESSION_BUS_ADDRESS`;
8. drains and validates each mutation generation while supervising the target;
9. after valid gate/status/reap evidence and terminal publisher EOF, composes
   the latest fully ACK-written generation and sends UsageSignal or Checkin over
   fd 20.

secretsd has no direct daemon channel. For each D-Bus mutation it retains the
store write lock, clones the accepted store once, mutates that candidate,
canonicalizes and publishes the complete next generation, observes the exact
matching host ACK or failure, and commits only after ACK. Preparation,
canonicalization, rejection, EOF, response mismatch, or transport failure leaves
the accepted live store unchanged. Discarded candidate secret buffers and secret
buffers displaced by commit are zeroized. The host accepts only bounded
exact-canonical state with valid identifiers, aliases/references, and timestamps.
The target never inherits fd 20 or the publication socket.

ACK means the host retained one invocation generation; it is neither caller
response delivery nor backend durability, and the later Checkin can still fail.
If the host completely writes ACK but secretsd dies or the caller misses the
success response, that generation remains eligible after terminal EOF. A
retained publisher endpoint fails closed with no fallback. A complete
JCS-canonical resource is inclusively limited to 2,097,152 bytes. The private
publication body has its separate inclusive 1,048,576-byte limit. The 64-KiB
limit applies only to unrelated control frames; Checkout and Checkin use the
resource channel's complete-resource bound.

## Split authentication and action wrappers

```nix
services.underwrap.instances.work = {
  wrappers.gh-auth = {
    target = "${pkgs.gh}/bin/gh";
    resource = "github/gh";
    libsecret = true;
  };

  wrappers.gh-pr = {
    target = "${ghPrHelper}/bin/gh-pr-helper";
    resource = "github/gh";
    libsecret = true;
  };
};
```

Both wrappers address the same resource but remain distinct
wrapper/resource pairs. In the backend:

- give `gh-auth → github/gh` a writable permission when it must run
  `gh auth login` or mutate credentials;
- give `gh-pr → github/gh` a non-writable permission for routine reads;
- choose an appropriate permission mode independently for each row.

The action helper should fix the repository, subcommand, and allowed argument
surface instead of exposing the full `gh` CLI to automation.

## Initialize credentials

1. Enroll the frontend runtime client and register both pairs.
2. Create or confirm resource `github/gh`.
3. Give `gh-auth` an active writable permission.
4. Run:

   ```bash
   gh-auth auth login
   ```

5. Complete GitHub's login flow.
6. Confirm that the libsecret collection/item tree appears under the
   `github/gh` resource in **Secrets**.
7. Set the `gh-pr` permission to an active non-writable mode and test the narrow
   helper.
8. Return `gh-auth` to `Locked` when routine credential mutation is not needed.

Exact command arguments depend on the GitHub CLI version and authentication
method. Never place a token directly on the command line.

## Read-only behavior

The target-facing Secret Service remains writable so normal keyring clients do
not fail during local mutation. When the permission is non-writable:

- secretsd publishes mutations into invocation-local host-retained state;
- `underwrap-ns` discards the changed generation at teardown;
- one structured writeback-rejected event is emitted;
- one stderr warning is emitted unless the caller set
  `UNDERWRAP_SUPPRESS_READONLY_WARNING=1`.

The backend is still the persistence authority. If a permission becomes
read-only during a writable invocation, finalize rejects the submitted
writeback.

## Compatible client patterns

Libsecret mode is suitable for:

- `gh` and `secret-tool`;
- applications using the C libsecret API;
- GNOME Secret Service clients;
- Go applications using the `99designs/keyring` Secret Service backend.

Some Go clients locate a collection by stable object path. secretsd validates a
target-chosen collection name and uses it as the path segment so those clients
can reopen the same collection. See [secretsd security](secretsd-security.md)
for validation and state limits.

## Security boundaries

- The libsecret resource is shared only because both wrapper configs name it.
- Permission rows remain per runtime client, wrapper, and resource.
- The target can ask its private Secret Service to reveal any item in that
  projected resource; wrapper narrowness still matters.
- A non-writable permission prevents persistence, not local D-Bus mutation.
- The private bus is filesystem-backed inside the mount namespace. Abstract host
  D-Bus sockets are a documented defense-in-depth limitation.

## Related documentation

- [Agent-facing wrappers](agent-facing-wrappers.md)
- [Wrapper author checklist](wrapper-author-checklist.md)
- [Architecture](architecture.md#libsecret-projection)
- [Security](security.md#libsecret)
- [Admin UI](admin-ui.md)
