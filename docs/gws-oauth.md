# Google Workspace via OAuth derivation

This guide covers the other advanced workflow underwrap supports well:

- the backend stores OAuth credential material
- the wrapper does **not** receive the long-lived refresh token
- on each invocation, the backend derives a short-lived access token and returns only that

This is the right model for CLIs such as `gws` where a mutable OAuth workflow is a better fit than a static API token.

## Why this mode exists

Static token injection is simple, but it is not a good fit for every credential type.

OAuth refresh tokens are different from plain API tokens:

- they are long-lived and high value
- they may need to be exchanged repeatedly
- the useful credential for the target process is often the **derived access token**, not the refresh token itself

underwrap's derivation support exists so the wrapper can receive the short-lived token it needs, while the refresh-token material stays in backend storage.

## The basic shape

A typical wrapper configuration looks like this:

```nix
wrappers.gws = {
  target = "${pkgs.gws}/bin/gws";
  secret = "google/workspace-creds";
  injection = {
    mode = "env";
    variable = "GOOGLE_WORKSPACE_CLI_TOKEN";
  };
  derivation = "oauth_refresh_to_access_token";
  requireUnlock = true;
};

secrets."google/workspace-creds" = {
  mutable = true;
};
```

The important parts are:

- `secret` points at stored OAuth material
- `derivation` tells underwrap to derive an access token on read
- `mutable = true` allows the stored credential material to change over time

## What is stored

The backend stores the OAuth material needed for token exchange, for example a JSON document containing fields such as:

- `client_id`
- `client_secret`
- `refresh_token`

The exact shape depends on the upstream workflow that created it.

## What the wrapper receives

The wrapper receives only the derived access token.

That means:

- the refresh token stays in backend storage
- the target process gets the credential it actually needs
- rotation and refresh remain a backend concern instead of a wrapper concern

## Why this is a good fit for `gws`

`gws` is a good example because it often wants a usable access token right now, not a pile of OAuth metadata.

underwrap can sit in the middle and turn:

```text
stored refresh-token material
```

into:

```text
short-lived access token for this invocation
```

without changing the user-facing wrapper workflow very much.

## Security and operational benefits

Compared with handing the target the refresh token directly, this gives you a better separation of concerns:

- the backend handles token exchange
- the target sees a narrower credential
- refresh-token material stays in one controlled storage path
- remote backend mode can keep that material off the target machine entirely

## Hardening details

The current implementation hardcodes the Google OAuth token endpoint instead of trusting a stored `token_uri` field. That is intended to reduce the risk of a tampered secret turning token exchange into an SSRF or credential-exfiltration path.

That deeper design reasoning is documented in [Security](security.md).

## A practical deployment pattern

This workflow pairs naturally with the remote backend:

- store Google OAuth material on your workstation
- run `underwrap-backendd` there
- forward the backend socket to the target machine over SSH
- let the target wrapper receive only the derived access token

That gives you a cleaner story than storing the refresh token on the target host.

See [Remote backend](remote-backend.md).

## When to use this mode

Use OAuth derivation when:

- the underlying credential is mutable
- the target process really only needs a short-lived access token
- you want refresh-token material to remain in backend storage

Do not use it for simple static-token workflows. Those are easier to reason about and easier to operate.

## Related docs

- [Getting started](getting-started.md)
- [Configuration reference](configuration.md)
- [Remote backend](remote-backend.md)
- [Security](security.md)
