# GitHub via libsecret

This guide covers the more advanced `gh` workflow:

- `gh` stores and retrieves credentials through `libsecret`
- underwrap provides a **private Secret Service** inside the wrapper's namespace
- stored credential material still flows through underwrap's backend

This is the right workflow when you want `gh` to behave more like a normal logged-in CLI instead of treating it as a plain `GH_TOKEN` consumer.

## Why this mode exists

A simple env-var wrapper is enough for many CLIs, but it is awkward for tools that expect to manage credentials through a secret store.

`gh` is a good example:

- `gh auth login` wants to perform an auth flow
- later invocations want to read credentials back
- storing credentials in the user's normal desktop keyring would bypass underwrap's storage model

So underwrap has a dedicated **libsecret mode**.

## What happens in libsecret mode

When a wrapper has `libsecret = true`:

1. `underwrap-exec` authenticates to the daemon as usual
2. the daemon returns the normal response and a pre-authenticated channel for `underwrap-secretsd`
3. `underwrap-ns` creates a private namespace environment
4. it starts a private `dbus-daemon`
5. it starts `underwrap-secretsd`, which implements `org.freedesktop.secrets`
6. the target process sees `DBUS_SESSION_BUS_ADDRESS` for that private bus
7. `gh` talks to `underwrap-secretsd` as if it were a normal Secret Service
8. `underwrap-secretsd` routes secret reads and writes back through the daemon

The result is that `gh` gets its expected libsecret workflow, but the stored credential material still belongs to underwrap's backend.

## A minimal configuration shape

The important wrapper option is:

```nix
libsecret = true;
```

A typical NixOS wrapper looks like this:

```nix
wrappers.gh = {
  target = "${pkgs.gh}/bin/gh";
  secret = "github/bootstrap";
  libsecret = true;
  injection = {
    mode = "env";
    variable = "GH_UNUSED";
  };
};

secrets."github/bootstrap" = {
  mutable = false;
};
```

A few notes:

- the injection variable is effectively unused once `gh` is relying on libsecret
- the secret referenced by `secret` is the wrapper's normal configured secret and can serve as bootstrap material or a placeholder
- the actual libsecret items are stored through the scoped channel handled by the daemon

For the complete option reference, see [Configuration reference](configuration.md).

## A realistic `gh` login flow

A common pattern is:

1. run the wrapper around `gh`
2. perform `gh auth login`
3. let `gh` store its resulting credentials through libsecret
4. future invocations retrieve those credentials from underwrap's backend via `underwrap-secretsd`

Conceptually:

```text
work-gh -> private dbus-daemon + underwrap-secretsd -> gh auth login
```

After that, future wrapper invocations reuse the stored credential material without touching the user's normal session keyring.

## Why this is useful

This gives you a cleaner story than either extreme:

### Better than a plain env-var-only workflow

A plain `GH_TOKEN` wrapper is simple, but it does not model `gh`'s normal stored-login flow very well.

### Better than using the real session keyring directly

If `gh` talked to the host's real `org.freedesktop.secrets` service, credential state would escape underwrap's storage model.

libsecret mode keeps the storage path inside underwrap.

## Security model, in plain English

underwrap is trying to ensure three things here:

1. `gh` sees a Secret Service implementation it can use normally
2. that Secret Service is private to the wrapper invocation
3. the long-lived credential material still flows through underwrap's backend, not through random side channels

The detailed mechanism is more involved than plain env-var injection because `underwrap-secretsd` needs a **persistent authenticated channel** back to the daemon.

That channel is created during the original authenticated wrapper request and then inherited into the private namespace. The target process itself does not keep that file descriptor.

For the full security analysis, see [Security](security.md).

## Limitations and expectations

- This is more complex than the simple token-in-env case.
- It depends on the private D-Bus setup working correctly.
- It is aimed at tools that genuinely expect libsecret semantics.
- It does not make `gh` itself a sandboxed tool; it only changes how credentials are delivered and stored.

## When to use this mode

Use libsecret mode when:

- the tool naturally expects a Secret Service backend
- you want login/store/reuse behavior to feel normal
- you still want the stored material to remain inside underwrap's storage model

Do **not** start here if you are just trying to learn underwrap. Start with [Getting started](getting-started.md) first.

## Related docs

- [Getting started](getting-started.md)
- [Configuration reference](configuration.md)
- [Architecture](architecture.md)
- [Security](security.md)
