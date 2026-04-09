# GitHub via libsecret

This guide shows the clean first-release pattern for `gh` with underwrap:

- declare one reusable libsecret credential namespace
- bind it read-write to an auth/login wrapper
- bind it read-only to narrower execution wrappers

That lets multiple wrappers share one `gh` credential store inside the same instance while keeping write authority explicit.

## Why this mode exists

Some tools want a Secret Service provider rather than a single env token.

`gh auth login` is the clearest example:

- it wants to store and retrieve credentials through Secret Service
- the stored data is not a single token value
- later `gh` executions should be able to reuse that state

underwrap provides a private Secret Service namespace for that workflow.

## What happens in libsecret mode

When a wrapper uses a credential declared with:

```nix
mode = "libsecret";
```

underwrap:

1. authenticates the wrapper over its wrapper socket
2. creates a scoped daemon channel for that credential namespace
3. launches a private D-Bus session bus inside the wrapper namespace
4. launches `underwrap-secretsd` on that private bus
5. points the target at that private bus via `DBUS_SESSION_BUS_ADDRESS`

The target then sees a normal Secret Service provider, but all storage flows through underwrap's backend.

## Recommended pattern: split auth and action wrappers

Example:

```nix
services.underwrap.instances.work = {
  credentials."github/gh" = {
    mode = "libsecret";
  };

  wrappers.gh-auth = {
    target = "${pkgs.gh}/bin/gh";
    use."github/gh".access = "read-write";
    unlock = {
      mode = "optional";
      maxSeconds = 1800;
    };
    unlockKey = "ssh-ed25519 AAAA...";
  };

  wrappers.gh-pr = {
    target = "/nix/store/...-gh-pr-helper/bin/gh-pr-helper";
    use."github/gh".access = "read-only";
    unlock = {
      mode = "always";
      maxSeconds = 1800;
    };
  };
};
```

This is a good first-release shape because it keeps the credential namespace reusable while separating:

- the wrapper that may mutate the store
- the wrappers that may only read from it

## Access levels

For a `libsecret` credential binding:

- `access = "read-write"` allows Secret Service writes/deletes/updates
- `access = "read-only"` allows reads only

That means you can give `gh auth login` one wrapper and normal operational actions another wrapper, both sharing the same stored credentials.

## Why this is cleaner than a bootstrap secret

There is no separate bootstrap secret in the user-facing model.

The declared credential itself is the namespace root.

That means the config stays honest:

- one credential ref
- one shared libsecret store
- one or more wrappers with explicit permissions

## When to use libsecret mode

Use it when the target genuinely expects Secret Service behavior, such as:

- `gh auth login`
- tools that already integrate with libsecret/keyring APIs

Do not use it for simple token injection. If the target only needs a fixed token in an env var, env mode is simpler and easier to reason about.

## Related docs

- [Getting started](getting-started.md)
- [Configuration reference](configuration.md)
- [Agent-facing wrappers](agents.md)
- [Security](security.md)
