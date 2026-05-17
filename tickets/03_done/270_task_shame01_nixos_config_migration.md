# Task: Migrate shame01 NixOS config to the new architecture

Parent: `epic_admin_ui_spa_rebuild_and_multiuser.md` (no story; final standalone deployment ticket)

## Scope

Update `~/nix/my_nix_os/hosts/shame01/configuration.nix` to deploy the new architecture.

**Do not start this ticket until tickets 200–262 are all in `tickets/03_done/`** and CI/VM tests are green. This is the deployment cutover.

### Changes

- Drop `systemd.services.underwrap-admin-ui` entirely.
- Drop `users.users.underwrap-admin-ui` and the matching group.
- Drop sops secret `underwrap-admin-ui-backend-token` (no longer needed).
- Move sops secret `underwrap-admin-ui-session-key` ownership to the `underwrap-backend` user (backend now signs session cookies). Rename to `underwrap-backend-session-key`.
- Update `systemd.services.underwrap-backendd`:
  - Admin listener moves from `127.0.0.1:19443` (plain HTTP) to a Tailscale-reachable address (e.g. `100.x.x.x:18444` or `[::]:18444`) with TLS configured.
  - Admin TLS cert/key: reuse the existing backend server cert if appropriate (same hostname), or provision a separate cert via sops.
  - `admin.session_key_file` points at the renamed sops secret.
  - Old `admin.tokens.robert.token_file` removed.
  - Old inline `stores.robert = { backend = "plain"; dir = ...; }` migrated to a declarative store via the new `services.underwrap-backend.declarativeStores.robert` option (from ticket 232).
- Drop the `underwrapAdminUiPackage` `let` binding and import at the top of `configuration.nix`.
- Update `ReadWritePaths` for the backend service to include `/var/lib/underwrap-backend/stores.d/` (dynamic stores) and `/var/lib/underwrap-backend/keys/` (encrypted-store keys).

### Runbook (in this ticket body so it's discoverable after close)

1. Take a backup of `/var/lib/underwrap-backend/secrets/` from shame01.
2. Apply the new NixOS config (`sudo nixos-rebuild switch --flake ...`). The old `underwrap-admin-ui` service goes away on switch.
3. SSH to shame01 and run `sudo -u underwrap-backend underwrap-backendd admin recover-operator` (added in ticket 214) to create the first admin operator on the migrated store. Alternatively, open the new HTTPS admin UI in a browser if the bootstrap sentinel is not yet set, and bootstrap via the UI.
4. Verify HTTPS access to the admin UI works on the Tailscale hostname.
5. Verify wrapper clients (`underwrap-robert@deepthought`, `underwrap-robert@smallthought`) still authenticate against the service listener — no changes expected there.
6. Verify the declarative `robert` store is visible in the Stores tab and the UI shows edit/delete disabled with the declarative tooltip.

## Acceptance criteria

- [ ] shame01 boots with the new config (test in a `nixos-rebuild test` boot first if practical).
- [ ] Browser can reach the admin UI over HTTPS on the Tailscale name.
- [ ] First admin successfully bootstrapped or recovered.
- [ ] Existing wrappers continue to read secrets from the backend.
- [ ] The old `underwrap-admin-ui` service no longer exists on the host.
- [ ] Sops secrets cleaned up (no orphaned `underwrap-admin-ui-*` keys remain).
- [ ] Runbook documented above and validated against the actual run.
