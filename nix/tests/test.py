"""underwrap VM test — local + remote instances on one machine."""

import base64
import json
import shlex
import time
import urllib.parse


def _seg(value):
    """Percent-encode a single URL path segment. Slashes inside the
    value (e.g. resource names like `github/token`) become `%2F` so
    they don't collide with the path delimiter."""
    return urllib.parse.quote(value, safe="")

# Helper for running commands as testuser.
def su(cmd):
    return f"su testuser -s /bin/sh -c '{cmd}'"

# Spoof script path is passed as an env var by the Nix test driver.
import os
SPOOF_SCRIPT = os.environ.get("SPOOF_SCRIPT", "/dev/null")
PEER_PROBE_SCRIPT = os.environ.get("PEER_PROBE_SCRIPT", "/dev/null")
PYTHON3 = os.environ.get("PYTHON3", "python3")

# Each daemon now runs a single HTTPS listener that serves both the
# runtime service API (mTLS-authenticated, `/v1/...`) and the admin
# SPA + JSON API (session-cookie + CSRF authenticated, `/v1/admin/...`).
ADMIN_LOCAL = "https://127.0.0.1:18443"
ADMIN_REMOTE = "https://127.0.0.1:18444"
CA_CERT = "/etc/underwrap-test-ca.pem"
ADMIN_BOOTSTRAP_USER = "tester"
ADMIN_BOOTSTRAP_PASSWORD = "hunter2-long"

# Per-admin session-cookie state, populated by `bootstrap_admin_session`
# during boot. `_COOKIE_JAR[url]` is curl's cookie file on the VM
# (carries `__Host-underwrap-session` and `__Host-underwrap-csrf` cookies);
# `_CSRF_TOKEN[url]` is the HMAC authenticity token from the most
# recent admin response's `X-CSRF-Token` header.
_COOKIE_JAR = {}
_CSRF_TOKEN = {}


def _cookie_jar_path(url):
    safe = url.replace("https://", "").replace(":", "-").replace("/", "_")
    return f"/tmp/underwrap-admin-cookies-{safe}.txt"


def _extract_csrf_response_header(headers_text):
    """Return the value of the `X-CSRF-Token` response header from a
    curl `-D-` dump, or `None` if missing."""
    for line in headers_text.splitlines():
        # Case-insensitive header match.
        if ":" in line and line.split(":", 1)[0].strip().lower() == "x-csrf-token":
            return line.split(":", 1)[1].strip()
    return None


def bootstrap_admin_session(url):
    """Create the first operator on an admin listener and capture its
    session cookies + CSRF authenticity token. Must be called exactly
    once per backend boot — subsequent bootstrap POSTs would return 409.
    """
    cookie_jar = _cookie_jar_path(url)
    headers_file = f"{cookie_jar}.headers"
    payload = json.dumps({
        "username": ADMIN_BOOTSTRAP_USER,
        "password": ADMIN_BOOTSTRAP_PASSWORD,
    })
    cmd = [
        "curl", "-fsS",
        "--cacert", CA_CERT,
        "-c", cookie_jar,
        "-D", headers_file,
        "-X", "POST",
        "-H", f"Origin: {url}",
        "-H", "Content-Type: application/json",
        "-d", payload,
        f"{url}/v1/admin/auth/bootstrap",
    ]
    machine.succeed(" ".join(shlex.quote(p) for p in cmd))

    headers = machine.succeed(f"cat {shlex.quote(headers_file)}")
    csrf = _extract_csrf_response_header(headers)
    if not csrf:
        raise AssertionError(
            f"bootstrap_admin_session: no X-CSRF-Token response header in {headers_file}"
        )

    _COOKIE_JAR[url] = cookie_jar
    _CSRF_TOKEN[url] = csrf


def login_admin_session(url):
    """Re-authenticate against an already-bootstrapped backend, e.g.
    after a daemon restart that wiped the in-memory session state.
    Captures a fresh session cookie + CSRF authenticity token from
    `/v1/admin/auth/login`."""
    cookie_jar = _cookie_jar_path(url)
    headers_file = f"{cookie_jar}.headers"
    payload = json.dumps({
        "username": ADMIN_BOOTSTRAP_USER,
        "password": ADMIN_BOOTSTRAP_PASSWORD,
    })
    cmd = [
        "curl", "-fsS",
        "--cacert", CA_CERT,
        "-c", cookie_jar,
        "-D", headers_file,
        "-X", "POST",
        "-H", f"Origin: {url}",
        "-H", "Content-Type: application/json",
        "-d", payload,
        f"{url}/v1/admin/auth/login",
    ]
    machine.succeed(" ".join(shlex.quote(p) for p in cmd))
    headers = machine.succeed(f"cat {shlex.quote(headers_file)}")
    csrf = _extract_csrf_response_header(headers)
    if not csrf:
        raise AssertionError(
            f"login_admin_session: no X-CSRF-Token response header in {headers_file}"
        )
    _COOKIE_JAR[url] = cookie_jar
    _CSRF_TOKEN[url] = csrf


def admin_api(url, path, payload=None, method="POST"):
    """Make an authenticated admin API call. Uses the session cookies
    populated by `bootstrap_admin_session` for the listener at `url`
    and adds an `X-CSRF-Token` header on every state-changing request
    so the backend's CSRF double-submit check passes.
    """
    is_state_changing = method not in ("GET", "HEAD", "OPTIONS")
    cookie_jar = _COOKIE_JAR.get(url)
    csrf = _CSRF_TOKEN.get(url)
    if cookie_jar is None or csrf is None:
        raise AssertionError(
            f"admin_api({url!r}, …): bootstrap_admin_session was never "
            f"called for this listener — cookies + CSRF token are unset",
        )

    cmd = [
        "curl", "-fsS",
        "--cacert", CA_CERT,
        "-b", cookie_jar,
        "-X", method,
        "-H", f"Origin: {url}",
    ]
    if is_state_changing:
        cmd += ["-H", f"X-CSRF-Token: {csrf}"]
    if payload is not None:
        cmd += ["-H", "Content-Type: application/json", "-d", json.dumps(payload)]
    cmd += [f"{url}{path}"]
    quoted = " ".join(shlex.quote(part) for part in cmd)
    out = machine.succeed(quoted)
    return json.loads(out) if out.strip() else None


def admin_write_secret(url, store, wrapper, secret_id, value):
    """Admin-side secret write. `secret_id` is the
    `<resource_name>/<slot>/<key>` path used in test fixtures; we
    split it from the right (slot + key are single segments) and
    translate the resource name to its backend id. `wrapper` is kept
    in the signature for call-site readability but is not on the
    new wire shape."""
    del wrapper  # kept for call-site readability only
    resource_name, slot, key = secret_id.rsplit("/", 2)
    rid = _resource_id_for(url, resource_name)
    return admin_api(
        url,
        f"/v1/admin/stores/{store}/secrets/{_seg(rid)}/{_seg(slot)}/{_seg(key)}",
        {"material_b64": base64.b64encode(value.encode()).decode()},
        method="PUT",
    )


# Auto-created resources are addressed by a stable backend id
# (`res_<hex>`), not by their operator-facing name. The test scripts
# think in names; the maps below are populated after the auto-create
# pass in `boot_and_service_checks` and consulted by the helpers
# below to translate name → id.
LOCAL_RESOURCE_IDS: dict[str, str] = {}
REMOTE_RESOURCE_IDS: dict[str, str] = {}


def _resource_id_for(url, name):
    table = LOCAL_RESOURCE_IDS if url == ADMIN_LOCAL else REMOTE_RESOURCE_IDS
    try:
        return table[name]
    except KeyError as e:
        raise AssertionError(
            f"resource {name!r} not in id-map for {url}: {table}"
        ) from e


def admin_upsert_grant(url, store, client_instance, wrapper, resource, mode, *, writable=False, reason=None):
    """Upsert a wrapper permission for a
    `(client_instance, wrapper, resource)` tuple. `resource` is the
    operator-facing name; it is translated to the stable backend id
    before the request is sent.

    `mode` is a dict that serializes WrapperApprovalMode, e.g.:
      {"kind": "locked"}
      {"kind": "unlocked"}
      {"kind": "unlocked-until", "until_epoch": 1234}
    """
    rid = _resource_id_for(url, resource)
    body = {"mode": mode, "writable": writable}
    if reason is not None:
        body["reason"] = reason
    return admin_api(
        url,
        f"/v1/admin/stores/{store}/wrapper-permissions/{_seg(client_instance)}/{_seg(wrapper)}/{_seg(rid)}",
        body,
        method="PUT",
    )


def admin_delete_grant(url, store, client_instance, wrapper, resource):
    rid = _resource_id_for(url, resource)
    return admin_api(
        url,
        f"/v1/admin/stores/{store}/wrapper-permissions/{_seg(client_instance)}/{_seg(wrapper)}/{_seg(rid)}",
        method="DELETE",
    )


def admin_get_blob(url, store, resource):
    """Read a resource blob's canonical bytes directly off the
    backend's on-disk store and return them base64-encoded.

    The admin API no longer exposes a `blob/get` endpoint — the SPA
    edits secrets per-key — so we go through
    the storage backend's on-disk layout: a `PlainBackend` stores each
    resource blob at `<dir>/resources/<resource_id>`. Picking the
    right `<dir>` per (url, store) keeps the byte-equal comparison
    semantics callers rely on.
    """
    resource_id = _resource_id_for(url, resource)
    storage_dir = _storage_dir_for(url, store)
    blob_path = f"{storage_dir}/resources/{resource_id}"
    return machine.succeed(
        f"base64 -w0 {shlex.quote(blob_path)}"
    ).strip()


# `(url, store)` → on-disk PlainBackend root for the resources tree.
# The paths match the systemd-tmpfiles entries in `nix/tests/node.nix`.
_STORAGE_DIRS = {
    (ADMIN_LOCAL, "local"): "/var/lib/underwrap-backend-local/secrets",
}


def _storage_dir_for(url, store):
    try:
        return _STORAGE_DIRS[(url, store)]
    except KeyError as e:
        raise AssertionError(
            f"no storage_dir registered for (url={url!r}, store={store!r}); "
            f"add it to _STORAGE_DIRS in test.py"
        ) from e


def admin_list_pairs(url, store):
    return admin_api(
        url,
        f"/v1/admin/stores/{store}/pairs",
        method="GET",
    )["pairs"]


def find_pair(pairs, wrapper, resource):
    """Locate the pair record for `(wrapper, resource_name)`. The wire
    shape carries `resource_id`, so we resolve `resource` (a name)
    against the appropriate id-map before matching."""
    if resource in LOCAL_RESOURCE_IDS:
        resource_id = LOCAL_RESOURCE_IDS[resource]
    elif resource in REMOTE_RESOURCE_IDS:
        resource_id = REMOTE_RESOURCE_IDS[resource]
    else:
        return None
    for p in pairs:
        if p["wrapper"] == wrapper and p["resource_id"] == resource_id:
            return p
    return None


# Wrapper -> resource id mapping for the local instance. Each test
# wrapper has a single declared resource; the grant key includes the
# resource so the daemon can distinguish per-resource policy.
LOCAL_WRAPPER_RESOURCES = {
    "test-echo": "test/token",
    "test-echo2": "test/other",
    "test-locked": "test/locked-tok",
    "test-sleep": "test/dumpable-tok",
    "test-descendant": "test/descendant-tok",
    "test-iso": "test/iso-tok",
    "test-iso-home": "test/iso-home-tok",
    "test-runtime-dir": "test/runtime-tok",
    "test-runtime-dir-denied": "test/runtime-denied-tok",
    "test-iso-dir": "test/iso-dir-tok",
    "test-iso-libsecret": "test/iso-libsecret-store",
    "test-crash": "test/crash-tok",
    "test-libsecret": "test/libsecret-store",
    "test-caps": "test/caps-tok",
    "test-ns-pids": "test/ns-pids-store",
    "test-ns-pids-env": "test/ns-pids-env-store",
    "test-dir-write": "test/dir-a",
    "test-dir-read": "test/dir-a",
    "test-dir-readonly-write": "test/dir-a",
    "test-dir-sleep": "test/dir-a",
    "test-dir-symlink": "test/dir-a",
    "test-dir-write-then-sleep": "test/dir-a",
    "test-dir-touch": "test/dir-a",
    "test-dir-env-probe": "test/dir-a",
    "test-dir-env-override-probe": "test/dir-a",
}

# Each wrapper now projects exactly one resource, so no secondary
# grants are needed.
LOCAL_WRAPPER_EXTRA_RESOURCES: dict[str, list[str]] = {}

REMOTE_WRAPPER_RESOURCES = {
    "remote-echo": "remote/token",
}

# Resolved at boot from `GET /v1/admin/client-instances`. The backend
# assigns runtime-client ids dynamically (see
# `crates/underwrap-backend/src/runtime_clients.rs`), so we look them
# up by their `current_store_name`.
LOCAL_CLIENT_INSTANCE = None
REMOTE_CLIENT_INSTANCE = None


def resolve_client_instance(url, store_name):
    """Find the runtime client instance id whose current store maps to
    `store_name`. The backend exposes this list at
    `GET /v1/admin/client-instances`."""
    items = admin_api(url, "/v1/admin/client-instances", None, method="GET")["items"]
    for item in items:
        if item["current_store_name"] == store_name:
            return item["id"]
    raise AssertionError(
        f"no client instance found for store {store_name!r} at {url}: {items}"
    )


def grant_local_unlocked(wrapper, resource=None):
    """Shortcut: grant `(local-client, wrapper, resource)` with mode
    Unlocked. Most VM-test scenarios just want the wrapper to work
    end-to-end without ceremony."""
    if resource is None:
        resource = LOCAL_WRAPPER_RESOURCES[wrapper]
    admin_upsert_grant(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        wrapper,
        resource,
        {"kind": "unlocked"},
        writable=True,
    )
    for extra in LOCAL_WRAPPER_EXTRA_RESOURCES.get(wrapper, []):
        admin_upsert_grant(
            ADMIN_LOCAL,
            "local",
            LOCAL_CLIENT_INSTANCE,
            wrapper,
            extra,
            {"kind": "unlocked"},
            writable=True,
        )


def boot_and_service_checks():
    # ============================================================
    # BOOT + SERVICE CHECKS
    # ============================================================

    machine.wait_for_unit("multi-user.target")
    machine.wait_for_unit("underwrap-backend.service")
    machine.wait_for_unit("underwrap-backend-local.service")

    # Wait for the unified HTTPS listeners to start accepting before
    # bootstrapping. The bootstrap-status endpoint is the cheapest GET
    # the backend exposes.
    for url in (ADMIN_LOCAL, ADMIN_REMOTE):
        machine.wait_until_succeeds(
            f"curl -fsS --cacert {CA_CERT} {url}/v1/admin/auth/bootstrap/status >/dev/null"
        )

    # Create the first operator on each backend and capture session
    # cookies + CSRF token. Every subsequent admin_api(...) call
    # depends on these globals being populated.
    bootstrap_admin_session(ADMIN_LOCAL)
    bootstrap_admin_session(ADMIN_REMOTE)

    # underwrapd-local / underwrapd-tunnel only come up after the
    # backends are reachable (they issue an empty `POST /v1/wrappers`
    # register call at startup), so wait for them now.
    machine.wait_for_unit("underwrapd-local.service")
    machine.wait_for_unit("underwrapd-tunnel.service")

    machine.succeed("id testuser")
    machine.succeed("id underwrap-local")
    machine.succeed("id underwrap-tunnel")

    # Assert the per-store admin list surfaces the names that each
    # daemon's `stores_declarative_dir` actually contains.
    local_stores = admin_api(ADMIN_LOCAL, "/v1/admin/stores", None, method="GET")["items"]
    remote_stores = admin_api(ADMIN_REMOTE, "/v1/admin/stores", None, method="GET")["items"]
    assert [s["name"] for s in local_stores] == ["local"]
    assert [s["name"] for s in remote_stores] == ["remote"]

    # Resolve the runtime client instance ids for both daemons. The
    # backend assigns these dynamically when the client is enrolled,
    # so we look them up by their `current_store_name` rather than
    # hard-coding ids in the VM fixture.
    global LOCAL_CLIENT_INSTANCE, REMOTE_CLIENT_INSTANCE
    LOCAL_CLIENT_INSTANCE = resolve_client_instance(ADMIN_LOCAL, "local")
    REMOTE_CLIENT_INSTANCE = resolve_client_instance(ADMIN_REMOTE, "remote")
    machine.log(
        f"resolved client instances: local={LOCAL_CLIENT_INSTANCE} "
        f"remote={REMOTE_CLIENT_INSTANCE}"
    )

    # Resource and wrapper/resource-pair records are now first-class
    # entities; wrappers auto-create them on register when the
    # per-instance `auto_create_wrapper_assets` flag is on. The flag
    # defaults off, so flip it on for both instances and restart the
    # daemons to fire a fresh register call.
    for url, instance_id, daemon_unit in (
        (ADMIN_LOCAL, LOCAL_CLIENT_INSTANCE, "underwrapd-local.service"),
        (ADMIN_REMOTE, REMOTE_CLIENT_INSTANCE, "underwrapd-tunnel.service"),
    ):
        admin_api(
            url,
            f"/v1/admin/client-instances/{instance_id}",
            {"auto_create_wrapper_assets": True},
            method="PUT",
        )
        machine.succeed(f"systemctl restart {daemon_unit}")
        machine.wait_for_unit(daemon_unit)

    # Give the daemons a moment to finish the startup register call.
    machine.sleep(2)

    local_resources = admin_api(
        ADMIN_LOCAL, "/v1/admin/stores/local/resources", method="GET"
    )["resources"]
    local_pairs = admin_api(
        ADMIN_LOCAL, "/v1/admin/stores/local/pairs", method="GET"
    )["pairs"]
    remote_resources = admin_api(
        ADMIN_REMOTE, "/v1/admin/stores/remote/resources", method="GET"
    )["resources"]
    remote_pairs = admin_api(
        ADMIN_REMOTE, "/v1/admin/stores/remote/pairs", method="GET"
    )["pairs"]

    # Populate the name → id maps that the wrapper-permission helpers
    # and `find_pair` consult. Auto-create assigns each resource a
    # stable `res_<hex>` id that differs from the operator-facing name.
    LOCAL_RESOURCE_IDS.clear()
    LOCAL_RESOURCE_IDS.update({r["name"]: r["id"] for r in local_resources})
    REMOTE_RESOURCE_IDS.clear()
    REMOTE_RESOURCE_IDS.update({r["name"]: r["id"] for r in remote_resources})

    assert any(
        r["name"] == "test/token"
        and r["created_by"].get("kind") == "client-instance"
        and r["created_by"].get("id") == LOCAL_CLIENT_INSTANCE
        for r in local_resources
    ), f"missing local resource record for test/token with client-instance provenance: {local_resources}"
    test_token_id = LOCAL_RESOURCE_IDS["test/token"]
    assert any(
        p["wrapper"] == "test-echo"
        and p["resource_id"] == test_token_id
        and p["created_by"].get("id") == LOCAL_CLIENT_INSTANCE
        for p in local_pairs
    ), f"missing local pair record (test-echo, test/token={test_token_id}): {local_pairs}"
    assert any(
        r["name"] == "remote/token"
        and r["created_by"].get("id") == REMOTE_CLIENT_INSTANCE
        for r in remote_resources
    ), f"missing remote resource record: {remote_resources}"
    remote_token_id = REMOTE_RESOURCE_IDS["remote/token"]
    assert any(
        p["wrapper"] == "remote-echo" and p["resource_id"] == remote_token_id
        for p in remote_pairs
    ), f"missing remote pair record (remote-echo, remote/token={remote_token_id}): {remote_pairs}"
    machine.log(
        "PASS: wrapper assets auto-created with client-instance provenance after flag flip"
    )

    # services.underwrap.hardenTargetHost defaults true; the module pins
    # kernel.yama.ptrace_scope=2 so a same-UID caller cannot inspect the
    # wrapper chain or the target process state. Confirm it took effect.
    scope = machine.succeed("cat /proc/sys/kernel/yama/ptrace_scope").strip()
    assert scope == "2", f"expected ptrace_scope=2 from hardenTargetHost, got {scope}"
    machine.log("PASS: kernel.yama.ptrace_scope=2 enforced by hardenTargetHost")



def local_socket_permissions():
    # ============================================================
    # LOCAL INSTANCE: one socket / one group / one setgid binary
    # ============================================================
    # The single-instance redesign collapses the old per-wrapper
    # group/socket/binary fan-out into exactly one of each per instance.
    # The wrapper name is now a client-supplied selector validated by the
    # daemon; the kernel gate is the single instance-group egid.

    perms = machine.succeed("stat -c %a /run/underwrap-local").strip()
    assert perms == "755", f"expected 755, got {perms}"

    # Exactly one wrapper socket per instance: mode 0660, owned by the
    # daemon user, group = the instance group. The daemon does NOT chown
    # the socket (no @chown in its syscall filter); it relies on its
    # primary group being the instance group and verifies the gid at bind.
    # Assert both owner and group so a regression in that inheritance —
    # not just the group — is caught.
    socket = "/run/underwrap-local/wrapper.sock"
    perms = machine.succeed(f"stat -c %a {socket}").strip()
    assert perms == "660", f"expected 660, got {perms}"
    owner = machine.succeed(f"stat -c %U {socket}").strip()
    assert owner == "underwrap-local", f"expected daemon-user owner, got {owner}"
    grp = machine.succeed(f"stat -c %G {socket}").strip()
    assert grp == "underwrap-local", f"expected instance group, got {grp}"
    count = machine.succeed(
        "ls /run/underwrap-local/*.sock 2>/dev/null | wc -l"
    ).strip()
    assert count == "1", f"expected exactly one instance socket, got {count}"

    # The single instance group exists; the old per-wrapper groups do not.
    machine.succeed("getent group underwrap-local")
    machine.fail("getent group underwrap-local-test-echo")
    machine.fail("getent group underwrap-local-test-echo2")

    # One setgid entrypoint per instance, owned by the instance group.
    stat = machine.succeed("stat -c '%A %G' /run/wrappers/bin/underwrap-local").strip()
    assert "underwrap-local" in stat, f"wrong group: {stat}"
    assert "s" in stat.lower(), f"setgid not set: {stat}"
    # The old per-wrapper setgid binaries are gone.
    machine.fail("test -e /run/wrappers/bin/local-test-echo")

    # Instance configs: full DaemonConfig + minimal underwrap-exec sidecar.
    machine.succeed("test -f /etc/underwrap/daemon/local.yaml")
    machine.succeed("test -f /etc/underwrap/instances/local.yaml")
    # The old per-wrapper sidecar directory is gone.
    machine.fail("test -e /etc/underwrap/wrappers/local-test-echo.yaml")

    # underwrap-ns has the capabilities it needs for namespace setup and
    # attempts bounding-set cleanup where the kernel permits it.
    caps = machine.succeed("getcap /run/wrappers/bin/underwrap-ns").strip()
    assert "cap_sys_admin" in caps, f"expected cap_sys_admin: {caps}"
    assert "cap_setpcap" in caps, f"expected cap_setpcap: {caps}"


def local_store_and_basic_ops():
    # ============================================================
    # LOCAL INSTANCE: store secrets + basic operations
    # ============================================================

    # Admin writes through the per-secret PUT endpoint are
    # unconditional; the global `writes_enabled` toggle is gone and
    # per-grant `writable` controls only wrapper-side writes.

    # Every wrapper invocation needs a grant for
    # `(client_instance, wrapper, resource)`. Pre-seed Unlocked grants
    # for the full wrapper/resource fixture so the subsequent test sections
    # can drive the wrappers without per-test boilerplate. Tests that
    # exercise the *absence* of a grant (or `Locked` / `UnlockedUntil`)
    # delete or replace these as they go.
    for wrapper, resource in [
        ("test-echo", "test/token"),
        ("test-echo2", "test/other"),
        ("test-locked", "test/locked-tok"),
        ("test-sleep", "test/dumpable-tok"),
        ("test-descendant", "test/descendant-tok"),
        ("test-iso", "test/iso-tok"),
        ("test-iso-home", "test/iso-home-tok"),
        ("test-runtime-dir", "test/runtime-tok"),
        ("test-runtime-dir-denied", "test/runtime-denied-tok"),
        ("test-crash", "test/crash-tok"),
        ("test-libsecret", "test/libsecret-store"),
        ("test-caps", "test/caps-tok"),
        ("test-ns-pids", "test/ns-pids-store"),
        ("test-ns-pids-env", "test/ns-pids-env-store"),
    ]:
        grant_local_unlocked(wrapper, resource)

    admin_write_secret(ADMIN_LOCAL, "local", "test-echo", "test/token/env/MY_TOKEN", "ghp_testvmtoken")
    admin_write_secret(ADMIN_LOCAL, "local", "test-echo2", "test/other/env/OTHER_SECRET", "other_secret_val")
    admin_write_secret(ADMIN_LOCAL, "local", "test-locked", "test/locked-tok/env/LOCKED_TOKEN", "locked_secret")
    admin_write_secret(ADMIN_LOCAL, "local", "test-sleep", "test/dumpable-tok/env/DUMPABLE_SECRET", "dumpable_test_secret")
    admin_write_secret(ADMIN_LOCAL, "local", "test-descendant", "test/descendant-tok/env/DESC_SECRET", "desc_secret_val")
    admin_write_secret(ADMIN_LOCAL, "local", "test-iso", "test/iso-tok/env/ISO_SECRET", "iso_secret_val")
    admin_write_secret(ADMIN_LOCAL, "local", "test-iso-home", "test/iso-home-tok/env/HOME_ISO_SECRET", "home_iso_val")
    admin_write_secret(ADMIN_LOCAL, "local", "test-runtime-dir", "test/runtime-tok/env/RUNTIME_SECRET", "runtime_val")
    admin_write_secret(ADMIN_LOCAL, "local", "test-runtime-dir-denied", "test/runtime-denied-tok/env/RUNTIME_SECRET", "runtime_val")
    admin_write_secret(ADMIN_LOCAL, "local", "test-crash", "test/crash-tok/env/CRASH_SECRET", "crash_secret_value")
    admin_write_secret(ADMIN_LOCAL, "local", "test-caps", "test/caps-tok/env/CAPS_SECRET", "caps_test_val")
    admin_write_secret(ADMIN_LOCAL, "local", "test-ns-pids-env", "test/ns-pids-env-store/env/NSPIDS_SECRET", "nspids_env_val")

    # Admin writes may update operator-managed credential refs centrally.
    admin_write_secret(ADMIN_LOCAL, "local", "test-echo", "test/token/env/MY_TOKEN", "overwrite")
    admin_write_secret(ADMIN_LOCAL, "local", "test-echo2", "test/other/env/OTHER_SECRET", "updated_val")


def local_unprivileged_wrapper_invocation():
    # ============================================================
    # LOCAL INSTANCE: unprivileged user wrapper invocation
    # ============================================================

    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo"))
    assert "MY_TOKEN=overwrite" in output, f"expected MY_TOKEN: {output}"
    machine.log("PASS: unprivileged user can use wrapper")

    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo2 hello"))
    assert "hello" in output
    machine.log("PASS: second wrapper works")

    # Daemon resolved identity from GID.
    logs = machine.succeed(
        "journalctl -u underwrapd-local.service --no-pager"
        " | sed 's/\\x1b\\[[0-9;]*m//g'"
        " | grep 'uid: 1099'"
    )
    assert "read request" in logs
    machine.log("PASS: daemon resolved identity for unprivileged user")


def per_user_bare_name_path():
    # ============================================================
    # PER-USER PATH INTEGRATION: convenience shims
    # ============================================================
    # The single `underwrap-<instance>` setgid binary in /run/wrappers/bin is the
    # real entry point. The module additionally installs per-user shims
    # (`<wrapper>` and `debug-<wrapper>`) into each authorized user's
    # profile bin. The shims are plain, UNPRIVILEGED shell scripts that
    # `exec` into `underwrap-<instance> [debug] <wrapper> "$@"` — they carry no
    # setgid bit and no capabilities.
    profile_bin = "/etc/profiles/per-user/testuser/bin"
    machine.succeed(f"test -d {profile_bin}")
    machine.log(f"PASS: per-user profile bin exists at {profile_bin}")

    # A real + debug shim exists for wrappers across both instances.
    for name in ["test-echo", "debug-test-echo", "remote-echo", "debug-remote-echo"]:
        machine.succeed(f"test -e {profile_bin}/{name}")
    machine.log("PASS: per-user bin contains real + debug shims across both instances")

    # The shims are unprivileged scripts (no setgid bit) that forward to
    # the right underwrap-<instance> entrypoint.
    for name, want_debug in [("test-echo", False), ("debug-test-echo", True)]:
        shim = machine.succeed(f"readlink -f {profile_bin}/{name}").strip()
        mode = machine.succeed(f"stat -c %A {shim}").strip()
        assert "s" not in mode.lower(), f"shim {name} unexpectedly setgid: {mode}"
        body = machine.succeed(f"cat {shim}")
        assert "underwrap-local" in body, f"shim {name} does not forward to underwrap-local: {body}"
        assert ("debug" in body) == want_debug, \
            f"shim {name} debug-subcommand mismatch: {body}"
    machine.log("PASS: shims are unprivileged scripts forwarding to underwrap-<instance>")

    # The per-user dir is a bare-name namespace: it must not expose the old
    # instance-prefixed names.
    rc = machine.execute(f"test -e {profile_bin}/local-test-echo")[0]
    assert rc != 0, "per-user bin unexpectedly contains prefixed name"

    # Login shell for testuser resolves the bare name through this dir.
    which_out = machine.succeed("su -l testuser -c 'command -v test-echo'").strip()
    assert which_out == f"{profile_bin}/test-echo", \
        f"expected bare 'test-echo' to resolve via per-user bin, got: {which_out}"

    # Bare-name invocation works end-to-end and agrees with the direct
    # `underwrap-local <wrapper>` form.
    output = machine.succeed("su -l testuser -c 'test-echo'")
    assert "MY_TOKEN=overwrite" in output, \
        f"expected MY_TOKEN via bare-name invocation: {output}"
    direct = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo"))
    assert "MY_TOKEN=overwrite" in direct, f"expected MY_TOKEN via underwrap-local: {direct}"
    machine.log("PASS: bare-name shim and underwrap-local entrypoint agree")


def local_verify_capabilities_dropped():
    # ============================================================
    # LOCAL INSTANCE: verify capabilities are dropped for target
    # ============================================================

    caps_output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-caps")).strip()
    machine.log(f"Target capabilities: {caps_output}")
    # Effective/permitted/inheritable/ambient caps must all be zero after
    # underwrap-ns drops privilege before exec. The bounding set may remain
    # non-zero on kernels that reject bounding-set pruning, but PR_SET_NO_NEW_PRIVS
    # prevents later privilege gain via exec.
    for line in caps_output.split("\n"):
        line = line.strip()
        if line.startswith("Cap") and ":" in line:
            name, value = line.split(":", 1)
            name = name.strip()
            value = value.strip()
            if name in ("CapEff", "CapPrm", "CapInh", "CapAmb"):
                assert value == "0000000000000000", \
                    f"FAIL: target has non-zero capability {name}: {value}"
    machine.log("PASS: target process has zero capabilities")


def local_isolation():
    # ============================================================
    # LOCAL INSTANCE: isolation
    # ============================================================

    machine.fail(su("cat /var/lib/underwrap-backend-local/secrets/test/token"))
    machine.log("PASS: unprivileged user cannot read secret files")

    # Wrapper output contains only its own secret.
    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo"))
    assert "MY_TOKEN=overwrite" in output
    assert "OTHER_SECRET" not in output
    assert "LOCKED_TOKEN" not in output
    machine.log("PASS: wrapper output contains only its own secret")


def local_spoofing_resistance():
    # ============================================================
    # LOCAL INSTANCE: spoofing resistance
    # ============================================================

    machine.succeed(su(f"{PYTHON3} {SPOOF_SCRIPT}"))
    machine.log("PASS: wrapper socket spoofing blocked by permissions")


def local_daemon_restart_persistence():
    # ============================================================
    # LOCAL INSTANCE: daemon restart persistence
    # ============================================================

    machine.succeed("systemctl restart underwrapd-local.service")
    machine.wait_for_file("/run/underwrap-local/wrapper.sock")
    time.sleep(0.5)
    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo"))
    assert "MY_TOKEN=overwrite" in output
    machine.log("PASS: secrets persist across restart")


def _status_uids(pid):
    """Read the four uid fields (real, effective, saved, fs) from
    `/proc/<pid>/status` as root. The status `Uid:` line is
    `Uid:\\t<real>\\t<eff>\\t<saved>\\t<fs>`."""
    line = machine.succeed(f"awk '/^Uid:/{{print $2, $3, $4, $5}}' /proc/{pid}/status").strip()
    return [int(x) for x in line.split()]


def _status_caps(pid):
    """Read the four capability sets (Eff/Prm/Inh/Amb) from
    `/proc/<pid>/status` as root, as a name→hex-string dict."""
    out = machine.succeed(f"grep -E '^Cap(Eff|Prm|Inh|Amb):' /proc/{pid}/status")
    caps = {}
    for line in out.splitlines():
        name, value = line.split(":", 1)
        caps[name.strip()] = value.strip()
    return caps


def _peer_probe(pid):
    """Run the non-ancestor peer probe (ptrace + process_vm_readv) as
    `testuser` against `pid` and return the parsed RESULT: dict. The
    probe is a freshly spawned peer — NOT an ancestor of the target."""
    out = machine.succeed(su(f"{PYTHON3} {PEER_PROBE_SCRIPT} {pid}")).strip()
    results = {}
    for line in out.splitlines():
        if not line.startswith("RESULT:"):
            continue
        key_val = line.split(":", 1)[1]
        if "=" in key_val or "(" in key_val:
            # e.g. PTRACE_DENIED(errno=1) — keep the bare token as the key.
            token = key_val.split("(", 1)[0]
            results[token] = key_val
        else:
            results[key_val] = "true"
    return results


def _assert_fd_reopen_denied(pid):
    """As `testuser`: re-opening the isolated target's open objects via
    `/proc/<pid>/{cwd,root,exe}` and `/proc/<pid>/fd/*` must be denied
    (ptrace-gated readlink, `fs/proc/base.c`). This is the positive
    boundary property: the caller cannot re-open the target's open
    secret file or socket. Matrix #17."""
    for link in ("cwd", "root", "exe"):
        rc = machine.execute(
            su(f"readlink /proc/{pid}/{link} >/dev/null 2>&1")
        )[0]
        assert rc != 0, (
            f"FAIL: same-caller process could readlink /proc/{pid}/{link} of the "
            f"target — fd/path re-open must be ptrace-denied"
        )
    # The fd symlinks are individually ptrace-gated; readlink of fd 0 must fail.
    rc = machine.execute(su(f"readlink /proc/{pid}/fd/0 >/dev/null 2>&1"))[0]
    assert rc != 0, (
        f"FAIL: same-caller process could re-open /proc/{pid}/fd/0 of the target"
    )
    machine.log(f"PASS: fd/path re-open of isolated target PID {pid} denied to the caller (#17)")


def _assert_world_readable_secret_free(pid, secret):
    """As `testuser`: the world-readable `/proc` residue
    (`cmdline`/`status`/`cgroup`) is readable regardless of uid, and the
    less-certain `mounts`/`mountinfo` may or may not be — but the secret
    marker must appear in NONE of them. Encodes the honest scope: the
    boundary hides secret-bearing files, not process metadata; secrets
    never live in argv or mount paths. Matrix #18."""
    for f in ("cmdline", "status", "cgroup"):
        rc, out = machine.execute(su(f"cat /proc/{pid}/{f} 2>/dev/null"))
        assert rc == 0, (
            f"FAIL: world-readable /proc/{pid}/{f} unexpectedly denied to the caller "
            f"(it is not ptrace-gated)"
        )
        assert secret not in out, (
            f"FAIL: secret {secret!r} leaked via world-readable /proc/{pid}/{f}"
        )
    for f in ("mounts", "mountinfo"):
        _rc, out = machine.execute(su(f"cat /proc/{pid}/{f} 2>/dev/null"))
        assert secret not in out, (
            f"FAIL: secret {secret!r} leaked via /proc/{pid}/{f} (mount paths must be secret-free)"
        )
    machine.log(
        f"PASS: world-readable /proc residue of PID {pid} is readable but secret-free (#18)"
    )


def _assert_no_leaked_fds(pid):
    """As root, read the isolated target's `/proc/<pid>/fd/*` and assert
    no privileged fd leaked across the exec boundary: nothing resolves to
    a socket / pipe / anon_inode (the channel, ExecBundle pipe, idmap
    `ns/user`, dbus/secretsd fds) and nothing points into the backend
    secrets tree. The target may legitimately hold stdio and its own
    on-disk script/binary fd. BONUS (2645 fd hygiene / matrix #20)."""
    links = machine.succeed(
        f"for f in /proc/{pid}/fd/*; do readlink \"$f\" 2>/dev/null || true; done"
    ).strip()
    for link in links.splitlines():
        link = link.strip()
        if not link:
            continue
        assert not (
            link.startswith("socket:")
            or link.startswith("pipe:")
            or link.startswith("anon_inode:")
        ), f"FAIL: isolated target PID {pid} leaked a privileged fd: {link!r}"
        assert "/secrets/" not in link, (
            f"FAIL: isolated target PID {pid} holds an fd into the backend secrets tree: {link!r}"
        )
    machine.log(f"PASS (bonus): isolated target PID {pid} inherited no leaked privileged fds (#20)")


def _assert_target_proc_access(host_pid):
    # Every target now runs under a dedicated per-target uid X, not as the
    # caller. That UID boundary makes ptrace_may_access fail for both READ-mode
    # files (environ/maps/smaps/auxv) and ATTACH-mode memory access from another
    # same-caller process.
    read_files = ["environ", "maps", "smaps", "auxv"]
    read_rcs = {
        f: machine.execute(
            f"su testuser -s /bin/sh -c 'cat /proc/{host_pid}/{f} >/dev/null 2>&1'"
        )[0]
        for f in read_files
    }
    rc_mem = machine.execute(
        f"su testuser -s /bin/sh -c 'dd if=/proc/{host_pid}/mem bs=1 count=1 >/dev/null 2>&1'"
    )[0]
    machine.log(
        f"same-caller access to target PID {host_pid}: READ={read_rcs} mem(ATTACH)={rc_mem}"
    )
    for f in read_files:
        assert read_rcs[f] != 0, (
            f"FAIL: target's /proc/{host_pid}/{f} is readable by another caller-owned "
            f"process (rc=0); the dedicated target UID boundary must deny it."
        )
    assert rc_mem != 0, (
        f"FAIL: same-caller process can read /proc/{host_pid}/mem — the ATTACH-mode "
        f"class must be denied by the UID boundary plus ptrace_scope=2"
    )
    machine.log(
        f"PASS: target PID {host_pid} — READ and ATTACH classes both denied to "
        f"the same-caller process (dedicated UID boundary)"
    )


def _assert_descendant_environ(child_pid, *, secret):
    # A subprocess the target spawns via a clean exec resets to dumpable, but it
    # still runs as the dedicated target uid X. The same-caller probe must be
    # denied for the whole subtree, not only the direct target.
    rc, out = machine.execute(
        f"su testuser -s /bin/sh -c 'tr \"\\0\" \"\\n\" < /proc/{child_pid}/environ 2>/dev/null'"
    )
    assert rc != 0, (
        f"FAIL: target descendant /proc/{child_pid}/environ is readable by a "
        f"same-caller process (rc={rc}); the UID boundary must cover the whole subtree"
    )
    assert secret not in out, (
        f"CRITICAL: caller recovered secret {secret!r} from descendant environ"
    )
    machine.log(
        f"PASS: descendant PID {child_pid} — environ denied to the same-caller "
        f"process (UID boundary covers the subtree)"
    )


def local_pid_namespace_proc_isolation():
    # ============================================================
    # LOCAL INSTANCE: mandatory per-target UID /proc baseline
    # ============================================================
    # Every wrapper target runs as a dedicated underwrap-target-* uid. This
    # baseline asserts that both a direct env-mode target and a clean-exec
    # descendant are denied to another process owned by the original caller.

    # --- (1) direct target: must be DENIED by the UID boundary ---
    machine.succeed(
        "su testuser -s /bin/sh -c '"
        "setsid /run/wrappers/bin/underwrap-local test-sleep 300 </dev/null >/dev/null 2>&1 &"
        "'"
    )
    time.sleep(2)

    # As root (test instrumentation only), find the host-visible PID of the
    # `sleep` target. It now runs as a dedicated uid X, not as testuser.
    host_pid = machine.succeed(
        "pgrep -f '[s]leep 300' || "
        "echo NONE"
    ).strip().split("\n")[0]

    if host_pid in ("NONE", ""):
        diag = machine.succeed(
            "ps -eo pid,user,comm,args | grep -E 'sleep 300|underwrap' | grep -v grep || echo 'no matching processes'"
        ).strip()
        assert False, (
            "FAIL: root could not locate the test-sleep target. Root sees all PID "
            "namespaces, so a missing target means the wrapper did not start -- not "
            f"isolation. matching processes:\n{diag}"
        )

    machine.log(f"Found direct target sleep process at host PID {host_pid}")
    cmdline = machine.succeed(
        f"tr '\0' ' ' < /proc/{host_pid}/cmdline 2>/dev/null || echo UNKNOWN"
    ).strip()
    assert "sleep" in cmdline, f"Host PID {host_pid} is not a sleep process: {cmdline}"

    _assert_target_proc_access(host_pid)

    machine.execute("pkill -KILL -f '[s]leep 300' || true")
    time.sleep(0.5)

    # --- (2) descendant: clean exec still must be DENIED ---
    machine.succeed(
        "su testuser -s /bin/sh -c '"
        "setsid /run/wrappers/bin/underwrap-local test-descendant </dev/null >/dev/null 2>&1 &"
        "'"
    )
    time.sleep(2)

    child_pid = machine.succeed(
        "pgrep -f '[s]leep 4242' || echo NONE"
    ).strip().split("\n")[0]
    if child_pid in ("NONE", ""):
        diag = machine.succeed(
            "ps -eo pid,user,comm,args | grep -E 'sleep 4242|descendant-target|underwrap' | grep -v grep || echo 'no matching processes'"
        ).strip()
        assert False, (
            "FAIL: root could not locate the test-descendant child (sleep 4242) -- "
            f"the descendant fixture did not start. matching processes:\n{diag}"
        )
    machine.log(f"Found descendant child process at host PID {child_pid}")

    _assert_descendant_environ(child_pid, secret="desc_secret_val")

    # The non-exec'ing wrapper supervisor stays non-dumpable (PR_SET_DUMPABLE=0
    # and never re-execs), so its environ is denied to same-caller processes too.
    wrapper_pids = machine.succeed(
        "pgrep -u testuser -f 'underwrap-local|underwrap' || echo none"
    ).strip()
    if wrapper_pids != "none":
        for pid_str in wrapper_pids.split("\n"):
            pid_str = pid_str.strip()
            if not pid_str or pid_str in (host_pid, child_pid):
                continue
            rc = machine.execute(
                f"su testuser -s /bin/sh -c 'cat /proc/{pid_str}/environ >/dev/null 2>&1'"
            )[0]
            assert rc != 0, (
                f"FAIL: same-caller read /proc/{pid_str}/environ (a non-dumpable "
                f"wrapper supervisor must stay protected)"
            )
        machine.log("PASS: wrapper supervisor processes also protected (dumpable=0)")

    machine.execute("pkill -KILL -f '[s]leep 4242' || true")
    machine.execute("pkill -KILL -f descendant-target || true")
    time.sleep(0.5)

def local_target_uid_isolation():
    # ============================================================
    # LOCAL INSTANCE: per-target uid isolation (story 2600 / 2670)
    # ============================================================
    # An env-mode target AND its descendant run as a dedicated sandbox uid X
    # (!= caller, != 0), so a same-caller process is denied the WHOLE subtree's
    # /proc secret surface. `test-iso` is a shell (the direct target) that
    # backgrounds a child `sleep 5252`; both run as X.
    machine.succeed(
        "su testuser -s /bin/sh -c '"
        "setsid /run/wrappers/bin/underwrap-local test-iso </dev/null >/dev/null 2>&1 &"
        "'"
    )
    time.sleep(2)

    # Find the descendant (sleep 5252) and, via its PPid, the direct target (the
    # iso-target shell). Root sees all PID namespaces and all uids.
    child_pid = machine.succeed(
        "pgrep -f '[s]leep 5252' || echo NONE"
    ).strip().split("\n")[0]
    assert child_pid not in ("NONE", ""), (
        "FAIL: could not find the test-iso descendant (sleep 5252) — the isolated "
        "fixture did not start. Check `journalctl -u underwrapd-local` for a uid-switch "
        "or table error."
    )
    direct_pid = machine.succeed(
        f"awk '/^PPid:/{{print $2}}' /proc/{child_pid}/status"
    ).strip()
    assert direct_pid and direct_pid != "0", (
        f"FAIL: could not resolve the direct target (PPid of {child_pid})"
    )
    machine.log(f"isolated direct target PID {direct_pid}, descendant PID {child_pid}")

    def real_uid(pid):
        return int(machine.succeed(
            f"awk '/^Uid:/{{print $2}}' /proc/{pid}/status"
        ).strip())

    # --- Authority: the subtree runs as a DEDICATED sandbox uid X ---
    x_uid = real_uid(child_pid)
    assert x_uid == real_uid(direct_pid), (
        f"FAIL: descendant uid {x_uid} != direct target uid {real_uid(direct_pid)} — "
        f"the subtree must share the dedicated uid"
    )
    assert x_uid != 0, "FAIL: isolated target runs as root (uid 0) — escalation"
    assert x_uid != 1099, (
        "FAIL: isolated target runs as the caller (uid 1099) — no boundary; the uid "
        "switch did not happen"
    )
    x_name = machine.succeed(f"getent passwd {x_uid} | cut -d: -f1").strip()
    assert x_name.startswith("underwrap-target-"), (
        f"FAIL: isolated target uid {x_uid} resolves to {x_name!r}, not a dedicated "
        f"underwrap-target-* sandbox user — the table authority is wrong"
    )
    # The sandbox user must be a member of ONLY its own group (never an instance
    # group), else a uid-X target could satisfy a wrapper-socket egid gate.
    groups = machine.succeed(f"id -Gn {x_name}").strip().split()
    assert groups == [x_name], (
        f"FAIL: sandbox user {x_name} is in groups {groups}, expected only [{x_name}] "
        f"— it must not belong to any instance group"
    )
    machine.log(
        f"PASS: isolated subtree runs as dedicated uid {x_uid} ({x_name}), member of "
        f"only its own group"
    )

    # --- Identity #10/#15: real==eff==saved==X, and zero capabilities ---
    # Reading /proc/<pid>/status as root proves getuid()==geteuid()==X (and
    # saved/fs too) without an in-target fixture — the non-relabel model
    # (2650 is WON'T DO): the target displays as the dedicated user, never the
    # caller. The capability sets must all be zero (drop_caps before exec).
    for pid_label, pid in (("direct", direct_pid), ("descendant", child_pid)):
        uids = _status_uids(pid)
        assert uids == [x_uid, x_uid, x_uid, x_uid], (
            f"FAIL: {pid_label} target uid fields {uids} != all-X ({x_uid}); "
            f"getuid()==geteuid()==saved==fs==X must hold"
        )
    caps = _status_caps(direct_pid)
    for name, value in caps.items():
        assert value == "0000000000000000", (
            f"FAIL: isolated target retains capability {name}={value}; all sets must be zero"
        )
    machine.log(
        f"PASS: isolated subtree shows real==eff==saved==fs=={x_uid} and zero capabilities (#10/#15)"
    )

    # --- Protection: same-caller process denied for the WHOLE subtree ---
    # READ-mode (environ/maps/smaps/auxv) + the /proc/<pid>/mem ATTACH vector.
    _assert_target_proc_access(direct_pid)
    _assert_target_proc_access(child_pid)
    rc, out = machine.execute(
        f"su testuser -s /bin/sh -c 'tr \"\\0\" \"\\n\" < /proc/{child_pid}/environ 2>/dev/null'"
    )
    assert "iso_secret_val" not in out, (
        "CRITICAL: caller recovered the isolated target's secret from the descendant "
        "environ"
    )
    machine.log(
        "PASS: caller cannot recover the isolated target's secret anywhere in its subtree"
    )

    # --- #5/#6: explicit ATTACH-class syscalls from a non-ancestor peer ---
    # A separately spawned testuser process (NOT an ancestor) attempts
    # ptrace(PTRACE_ATTACH) and process_vm_readv against the direct target and
    # its descendant. Both are denied by the uid gate (and ptrace_scope=2), and
    # neither recovers the secret. The ancestor variant lives in
    # local_ancestor_attack; this is the same-host peer variant the matrix adds.
    for pid_label, pid in (("direct", direct_pid), ("descendant", child_pid)):
        probe = _peer_probe(pid)
        assert "PTRACE_DENIED" in probe, (
            f"CRITICAL: peer ptrace(PTRACE_ATTACH) on the {pid_label} isolated target "
            f"was not denied: {probe}"
        )
        assert "PTRACE_ALLOWED" not in probe, (
            f"CRITICAL: peer could ptrace-attach the {pid_label} isolated target: {probe}"
        )
        assert "VM_READV_DENIED" in probe, (
            f"CRITICAL: peer process_vm_readv on the {pid_label} isolated target was not "
            f"denied: {probe}"
        )
        assert "VM_READV_SECRET_LEAKED" not in probe and "VM_READV_ALLOWED" not in probe, (
            f"CRITICAL: peer process_vm_readv reached the {pid_label} isolated target's "
            f"memory: {probe}"
        )
    machine.log("PASS: non-ancestor peer ptrace + process_vm_readv both DENIED (#5/#6)")

    # --- #17: fd/path re-open of the target's open objects is denied ---
    _assert_fd_reopen_denied(direct_pid)
    _assert_fd_reopen_denied(child_pid)

    # --- #18: world-readable /proc residue is readable but secret-free ---
    _assert_world_readable_secret_free(direct_pid, "iso_secret_val")
    _assert_world_readable_secret_free(child_pid, "iso_secret_val")

    # --- #8: root CAN read (documented limitation, not a defect) ---
    # CAP_SYS_PTRACE bypasses the uid gate, so root reads the isolated target's
    # environ (and recovers the secret). Asserting the success encodes the true
    # threat model: the boundary is against UNPRIVILEGED same-uid callers, not root.
    root_env = machine.succeed(
        f"tr '\\0' '\\n' < /proc/{direct_pid}/environ 2>/dev/null || "
        f"tr '\\0' '\\n' < /proc/{child_pid}/environ"
    )
    assert "iso_secret_val" in root_env, (
        "FAIL: root could not read the isolated target's environ — the documented "
        "root-can-read limitation does not hold; the fixture or assumption is wrong"
    )
    machine.log("PASS: root reads the isolated target's environ (documented limitation, #8)")

    # --- Bonus (2645): setsid severed the controlling tty; fresh IPC ns; no
    # leaked fds. These shrink the target's reach over the caller a little
    # further — defense-in-depth by-products, NOT a goal (see docs/security.md
    # "the target attacking back at the caller" non-goal).
    # setsid: /proc/<pid>/stat session field == own pid, tty_nr == 0.
    stat = machine.succeed(f"cat /proc/{direct_pid}/stat")
    after_comm = stat.rpartition(")")[2].split()
    # Fields after comm: [0]=state [1]=ppid [2]=pgrp [3]=session [4]=tty_nr ...
    session, tty_nr = after_comm[3], after_comm[4]
    assert session == direct_pid, (
        f"FAIL (bonus): isolated target session {session} != its pid {direct_pid} — setsid did not run"
    )
    assert tty_nr == "0", (
        f"FAIL (bonus): isolated target tty_nr {tty_nr} != 0 — controlling tty not severed (#19)"
    )
    machine.log("PASS (bonus): isolated target is a session leader with no controlling tty (#19)")

    # Fresh IPC namespace (CLONE_NEWIPC): the target's ipc ns differs from root's.
    target_ipc = machine.succeed(f"readlink /proc/{direct_pid}/ns/ipc").strip()
    host_ipc = machine.succeed("readlink /proc/self/ns/ipc").strip()
    assert target_ipc != host_ipc, (
        f"FAIL (bonus): isolated target shares the host IPC namespace ({target_ipc})"
    )
    machine.log("PASS (bonus): isolated target has a fresh IPC namespace (2645)")

    # fd hygiene: no leaked privileged fds inherited across the exec boundary.
    _assert_no_leaked_fds(direct_pid)

    # The direct target is PID 1 of its namespace (ignores SIGTERM) and runs as
    # uid X (a testuser pkill cannot reach it): SIGKILL as root from the host ns.
    machine.execute("pkill -KILL -f '[s]leep 5252' || true")
    machine.execute("pkill -KILL -f iso-target || true")
    time.sleep(0.5)


def local_target_uid_idmapped_home():
    # ============================================================
    # LOCAL INSTANCE: idmapped directory for an isolated target (story 2640)
    # ============================================================
    # An isolated target runs as the dedicated uid X but, through an idmapped
    # `~/iso-data` mount that maps ONLY caller<->X, it reads a caller-owned file
    # as owner and its writeback lands on disk owned by the CALLER (not X).
    # Back iso-data with a tmpfs (owned by the caller): tmpfs fully supports
    # idmapped mounts and lives in init_user_ns, so the test is deterministic
    # regardless of what filesystem the VM's /home happens to be on. Production
    # homes (ext4/btrfs/xfs) support idmapped mounts too; this only pins the test
    # environment (see story 2660's "deterministic /home backing" note).
    machine.succeed("mkdir -p /home/testuser/iso-data")
    machine.execute("umount /home/testuser/iso-data 2>/dev/null || true")
    machine.succeed(
        "mount -t tmpfs -o mode=0755,uid=1099,gid=100 tmpfs /home/testuser/iso-data"
    )
    machine.succeed(
        "su testuser -s /bin/sh -c '"
        "printf %s caller-content-2640 > /home/testuser/iso-data/from-caller && "
        "rm -f /home/testuser/iso-data/from-target'"
    )
    # Runs synchronously: the target reads from-caller, writes from-target, exits.
    # A failure here (e.g. idmap read denied) propagates and fails the test.
    machine.succeed(
        "su testuser -s /bin/sh -c '/run/wrappers/bin/underwrap-local test-iso-home'"
    )
    out = machine.succeed("cat /home/testuser/iso-data/from-target").strip()
    assert out == "caller-content-2640", (
        f"FAIL: idmapped target did not reproduce the caller file content "
        f"(got {out!r}) — the idmapped read did not work"
    )
    owner = machine.succeed("stat -c %U /home/testuser/iso-data/from-target").strip()
    assert owner == "testuser", (
        f"FAIL: idmapped writeback landed owned by {owner!r}, expected testuser — "
        f"the caller<->X write mapping is wrong"
    )
    machine.log(
        "PASS: idmapped dir (2640) — isolated target read the caller's file as "
        "owner; its writeback is owned by the caller on disk"
    )


def local_target_uid_runtime_dir():
    # ============================================================
    # LOCAL INSTANCE: accessRuntimeDir idmaps the caller's runtime dir
    # ============================================================
    # The dedicated target uid cannot reach the caller's 0700 runtime dir
    # (XDG_RUNTIME_DIR points at <xdgRuntimeDirBase>/<caller-uid>). The
    # accessRuntimeDir flag idmaps that dir caller<->X into the namespace, like
    # an accessPaths entry resolved from the caller's uid at invocation. Provision
    # the caller-owned runtime dir exactly as logind would.
    machine.succeed(
        "mkdir -p /run/underwrap-test-runtime/1099 && "
        "chown testuser:users /run/underwrap-test-runtime/1099 && "
        "chmod 0700 /run/underwrap-test-runtime/1099 && "
        "rm -rf /run/underwrap-test-runtime/1099/uw-runtime-probe"
    )
    # With the flag: the target mkdirs + writes under XDG_RUNTIME_DIR, and the
    # marker lands on disk owned by the caller (idmapped write).
    out = machine.succeed(
        "su testuser -s /bin/sh -c '/run/wrappers/bin/underwrap-local test-runtime-dir'"
    ).strip()
    assert out == "runtime_dir_ok", (
        f"FAIL: accessRuntimeDir target could not use XDG_RUNTIME_DIR (got {out!r})"
    )
    owner = machine.succeed(
        "stat -c %U /run/underwrap-test-runtime/1099/uw-runtime-probe/marker"
    ).strip()
    assert owner == "testuser", (
        f"FAIL: runtime-dir writeback owned by {owner!r}, expected testuser — "
        f"the caller<->X mapping is wrong"
    )
    # Without the flag: the dedicated uid cannot traverse the caller's 0700
    # runtime dir, so the identical target fails and writes nothing.
    machine.succeed("rm -rf /run/underwrap-test-runtime/1099/uw-runtime-probe")
    rc, _ = machine.execute(
        "su testuser -s /bin/sh -c '/run/wrappers/bin/underwrap-local test-runtime-dir-denied'"
    )
    assert rc != 0, (
        "FAIL: target without accessRuntimeDir wrote under the caller's runtime "
        "dir — the flag is not load-bearing"
    )
    rc2, _ = machine.execute(
        "test -e /run/underwrap-test-runtime/1099/uw-runtime-probe/marker"
    )
    assert rc2 != 0, (
        "FAIL: target without accessRuntimeDir still created the runtime-dir marker"
    )
    machine.log(
        "PASS: accessRuntimeDir — isolated target wrote under XDG_RUNTIME_DIR "
        "(owned by the caller); the same target without the flag was denied"
    )


def local_target_uid_files_libsecret():
    # ============================================================
    # LOCAL INSTANCE: files + libsecret projection under mandatory target UID
    # ============================================================
    # An isolated (dedicated-uid X) target can still project a files slot and a
    # libsecret slot. The X-owned staging / private bus is created by the child
    # AFTER the switch to X (so the uid-X target can read/write it), and the
    # supervisor reads it back through the single setfsuid(X) window. This is the
    # carved-out remainder of 2630.

    # --- Files slot under mandatory target UID ---
    grant_local_unlocked("test-iso-dir")
    # Back ~/iso-files with a tmpfs (idmap-capable, caller-owned) so the test is
    # deterministic regardless of the VM's /home filesystem (see iso-home test).
    machine.execute("umount /home/testuser/iso-files 2>/dev/null || true")
    machine.succeed("mkdir -p /home/testuser/iso-files")
    machine.succeed(
        "mount -t tmpfs -o mode=0755,uid=1099,gid=100 tmpfs /home/testuser/iso-files"
    )

    # Write run: the uid-X target writes a file into the X-owned staging; the
    # supervisor persists it to the backend through the setfsuid(X) window.
    machine.succeed(
        "su testuser -s /bin/sh -c '/run/wrappers/bin/underwrap-local test-iso-dir write'"
    )
    blob_files = json.loads(
        base64.b64decode(admin_get_blob(ADMIN_LOCAL, "local", "test/iso-dir-tok"))
    )["files"]
    assert "iso-files/dir-a/token" in blob_files, (
        "FAIL: isolated files-slot writeback did not persist to the backend blob: "
        f"{sorted(blob_files.keys())}"
    )
    machine.log("PASS: isolated files-slot writeback persisted via the setfsuid(X) window")

    # Read run: the blob is materialized into the X-owned staging and the uid-X
    # target reads it back, proving the round-trip end to end.
    out = (
        machine.succeed(
            "su testuser -s /bin/sh -c '/run/wrappers/bin/underwrap-local test-iso-dir read'"
        )
        .strip()
        .split("\n")[-1]
        .strip()
    )
    assert out == "iso_files_secret_val", (
        f"FAIL: isolated files-slot read did not return the persisted secret: {out!r}"
    )
    machine.log("PASS: isolated files-slot read round-trips the persisted content")

    # While a files target runs, a same-caller host process must not see
    # the staged secret — the staging tmpfs lives only in the private mount ns.
    machine.succeed(
        "su testuser -s /bin/sh -c '"
        "setsid /run/wrappers/bin/underwrap-local test-iso-dir sleep </dev/null >/dev/null 2>&1 &'"
    )
    time.sleep(1)
    host_token = machine.execute(
        "su testuser -s /bin/sh -c 'cat /home/testuser/iso-files/dir-a/token 2>/dev/null'"
    )[1]
    assert "iso_files_secret_val" not in host_token, (
        "FAIL: same-caller host process read the target's staged secret"
    )
    host_staging = machine.succeed(
        "find /tmp/.underwrap-ns-directories -type f 2>/dev/null || true"
    ).strip()
    assert host_staging == "", (
        f"FAIL: host view exposes isolated staging files: {host_staging}"
    )
    machine.log("PASS: files staging is invisible to a same-caller host process")
    # sleep 5353 is the isolated direct target (PID 1 of its namespace, runs as
    # uid X), so it ignores SIGTERM and a testuser pkill can't reach it: SIGKILL
    # as root from the host (ancestor) namespace.
    machine.execute("pkill -KILL -f '[s]leep 5353' || true")
    machine.execute("pkill -u testuser -x underwrap-ns || true")
    time.sleep(0.5)
    machine.execute("umount /home/testuser/iso-files 2>/dev/null || true")

    # --- libsecret slot under mandatory target UID ---
    grant_local_unlocked("test-iso-libsecret")
    import tempfile
    import os

    with tempfile.NamedTemporaryFile(mode="w", suffix=".secret", delete=False) as f:
        f.write("iso_libsecret_val")
        secret_file = f.name
    machine.copy_from_host(secret_file, "/tmp/.iso-libsecret-secret")
    os.unlink(secret_file)
    machine.succeed(
        "chmod 600 /tmp/.iso-libsecret-secret && chown testuser /tmp/.iso-libsecret-secret"
    )
    machine.succeed(
        'su testuser -s /bin/sh -c "'
        "cat /tmp/.iso-libsecret-secret | "
        "/run/wrappers/bin/underwrap-local test-iso-libsecret store "
        '--label=IsoToken service iso-svc account iso-acct"'
    )
    machine.succeed("rm -f /tmp/.iso-libsecret-secret")
    machine.log("PASS: isolated libsecret store via secretsd as uid X")

    iso_out = machine.succeed(
        'su testuser -s /bin/sh -c '
        '"/run/wrappers/bin/underwrap-local test-iso-libsecret lookup '
        'service iso-svc account iso-acct"'
    ).strip()
    iso_last = iso_out.split("\n")[-1].strip()
    assert iso_last == "iso_libsecret_val", (
        f"FAIL: isolated libsecret lookup returned {iso_last!r}, expected iso_libsecret_val"
    )
    machine.log("PASS: isolated libsecret lookup over the private bus")

    # The libsecret state persisted to the backend (single-blob storage).
    iso_ls_id = LOCAL_RESOURCE_IDS["test/iso-libsecret-store"]
    have = machine.succeed(
        "test -f "
        f"/var/lib/underwrap-backend-local/secrets/resources/{iso_ls_id} "
        "&& echo ok || echo none"
    ).strip()
    assert have == "ok", (
        f"FAIL: isolated libsecret state not persisted at resources/{iso_ls_id}"
    )
    machine.log("PASS: isolated libsecret state persisted to the backend")

    # While running, the private bus must be invisible to a same-caller host process.
    machine.succeed(
        'su testuser -s /bin/sh -c "'
        "setsid /run/wrappers/bin/underwrap-local test-iso-libsecret lookup "
        'service iso-svc account iso-acct </dev/null >/dev/null 2>&1 &"'
    )
    time.sleep(1)
    iso_bus = machine.succeed(
        "find /tmp/.underwrap-ns-bus -type s 2>/dev/null || true"
    ).strip()
    assert iso_bus == "", f"FAIL: isolated private bus socket visible on host: {iso_bus}"
    machine.log("PASS: libsecret private bus invisible to a same-caller host process")
    machine.execute("pkill -u testuser -x underwrap-ns || true")
    time.sleep(0.5)


def local_target_uid_authority():
    # ============================================================
    # LOCAL INSTANCE: target-uid authority (stories 2620/2630/2660)
    # ============================================================
    # The dedicated sandbox uid is chosen by underwrap-ns from a ROOT-OWNED
    # target-path -> uid table, NEVER from anything the caller or underwrap-exec
    # supplies (the ExecBundle carries no uid field — see the protocol round-trip
    # test exec_bundle_carries_no_target_identity). This mirrors "the daemon, not
    # the wrapper, chooses the target binary": a compromised unprivileged stage
    # cannot redirect the cap_setuid switch to root or another service. These are
    # the static authority gates.

    # underwrap-ns carries cap_setuid + cap_setgid (to perform the mandatory
    # target uid switch) on top of cap_sys_admin.
    caps = machine.succeed("getcap /run/wrappers/bin/underwrap-ns").strip()
    for needed in ("cap_setuid", "cap_setgid", "cap_sys_admin"):
        assert needed in caps, (
            f"FAIL: underwrap-ns is missing {needed} (needed for the uid switch): {caps}"
        )
    machine.log(f"PASS: underwrap-ns holds cap_setuid+cap_setgid+cap_sys_admin ({caps})")

    # The authority table is the SOLE source of truth for which uid a target runs
    # as. Root-owned and not writable by anyone but root, else an unprivileged
    # user could redirect a target to an arbitrary (e.g. root) uid.
    table = "/etc/underwrap/target-uids"
    owner = machine.succeed(f"stat -c '%U:%G' {table}").strip()
    assert owner == "root:root", f"FAIL: {table} owner {owner!r} != root:root"
    mode = machine.succeed(f"stat -c %a {table}").strip()
    assert mode == "644", (
        f"FAIL: {table} mode {mode!r} != 644 — must not be group/other-writable"
    )
    machine.log(f"PASS: {table} is root:root {mode} (sole, unforgeable uid authority)")

    # Each row maps an absolute target path to a dedicated sandbox user/group
    # ("<path>\t<user>\t<group>"), never to root.
    contents = machine.succeed(f"cat {table}")
    iso_rows = [l for l in contents.splitlines() if l.strip()]
    assert iso_rows, f"FAIL: {table} is empty but wrappers exist"
    for row in iso_rows:
        fields = row.split("\t")
        assert len(fields) == 3, f"FAIL: malformed authority row: {row!r}"
        path, user, group = fields
        assert path.startswith("/"), f"FAIL: non-absolute target path in table: {row!r}"
        assert user.startswith("underwrap-target-"), (
            f"FAIL: authority row maps {path} to non-sandbox user {user!r}"
        )
        assert user != "root" and group != "root", (
            f"FAIL: authority row maps {path} to root: {row!r}"
        )
    machine.log(
        f"PASS: target-uid table maps {len(iso_rows)} target path(s) to dedicated "
        f"underwrap-target-* users (never root)"
    )


def local_target_uid_readonly_writeback():
    # ============================================================
    # LOCAL INSTANCE: read-only-grant writeback drop under mandatory target UID
    # ============================================================
    # Proves the read-only drop for a files target in the mandatory uid-X model:
    # the target writes a CHANGED value into its X-owned staging, the supervisor
    # reads the staging back through the single setfsuid(X) window, sees the
    # change, and DROPS it because the grant is read-only — the backend blob is
    # unchanged and the structured audit event fires. So the cross-uid read still
    # happens (exercising the setfsuid window); only the persist is refused.
    DIR = "test/iso-dir-tok"

    # Fresh caller-owned tmpfs for ~/iso-files (idmap-capable; see iso-home test).
    machine.execute("umount /home/testuser/iso-files 2>/dev/null || true")
    machine.succeed("mkdir -p /home/testuser/iso-files")
    machine.succeed(
        "mount -t tmpfs -o mode=0755,uid=1099,gid=100 tmpfs /home/testuser/iso-files"
    )

    # Seed a known baseline via a WRITABLE write, then flip the grant to read-only.
    grant_local_unlocked("test-iso-dir")  # writable=True
    machine.succeed(
        "su testuser -s /bin/sh -c '/run/wrappers/bin/underwrap-local test-iso-dir write'"
    )
    baseline = admin_get_blob(ADMIN_LOCAL, "local", DIR)

    admin_upsert_grant(
        ADMIN_LOCAL, "local", LOCAL_CLIENT_INSTANCE, "test-iso-dir", DIR,
        {"kind": "unlocked"}, writable=False,
    )

    # Snapshot the journal cursor so we can scope the audit-event grep.
    cur_out = machine.succeed("journalctl -n 1 -o export --no-pager")
    cursor = next(
        l[len("__CURSOR="):] for l in cur_out.splitlines() if l.startswith("__CURSOR=")
    )

    # `rewrite` writes a DISTINCT value, so the post-run blob differs from the
    # baseline — the change the read-only grant must drop.
    stderr_capture = machine.succeed(
        su(
            "/run/wrappers/bin/underwrap-local test-iso-dir rewrite 2>/tmp/iso-ro-stderr.log "
            "&& cat /tmp/iso-ro-stderr.log"
        )
    )
    time.sleep(0.5)

    after = admin_get_blob(ADMIN_LOCAL, "local", DIR)
    assert after == baseline, (
        "FAIL: isolated read-only grant persisted a change — the backend blob changed"
    )

    journal_after = machine.succeed(
        f"journalctl --no-pager --after-cursor={shlex.quote(cursor)} "
        "SYSLOG_IDENTIFIER=underwrap-ns --output=export || true"
    )
    assert "AUDIT=writeback_rejected" in journal_after, (
        f"FAIL: isolated read-only drop missing writeback_rejected event: {journal_after!r}"
    )
    assert "REASON=grant_readonly" in journal_after, (
        f"FAIL: isolated read-only drop missing grant_readonly reason: {journal_after!r}"
    )
    assert f"RESOURCE_NAME={DIR}" in journal_after, (
        f"FAIL: isolated read-only drop journal event missing resource name: {journal_after!r}"
    )
    assert "grant is read-only" in stderr_capture, (
        f"FAIL: isolated read-only drop missing human-readable stderr warning: {stderr_capture!r}"
    )
    assert f"resource '{DIR}'" in stderr_capture, (
        f"FAIL: isolated read-only drop stderr warning missing resource name: {stderr_capture!r}"
    )
    machine.log(
        "PASS: read-only grant drops the change after reading the X-owned staging "
        "through the setfsuid(X) window; journal records grant_readonly"
    )

    machine.execute("umount /home/testuser/iso-files 2>/dev/null || true")


def local_ancestor_attack():
    # ============================================================
    # LOCAL INSTANCE: adversarial ancestor attack (CRITICAL TEST)
    # ============================================================
    # A PARENT process spawns a wrapper (test-iso) and tries to inspect the
    # dedicated-uid-X target subtree it spawned. The UID boundary (story 2600)
    # makes `ptrace_may_access` fail the UID gate for BOTH the READ-mode files
    # (/proc/<pid>/environ) and the ATTACH-mode vectors (ptrace,
    # /proc/<pid>/mem), so even an ancestor cannot recover the secret. The unique
    # `sleep 5252` marker also means the attack locates exactly this run's target,
    # never a stale process.

    ANCESTOR_SCRIPT = os.environ.get("ANCESTOR_ATTACK_SCRIPT", "/dev/null")
    attack_output = machine.succeed(
        f"su testuser -s /bin/sh -c '{PYTHON3} {ANCESTOR_SCRIPT}'"
    ).strip()
    machine.log(f"Ancestor attack results: {attack_output}")

    # Parse results.
    result_lines = [l for l in attack_output.split("\n") if l.startswith("RESULT:")]
    results = {}
    for line in result_lines:
        key_val = line.split(":", 1)[1]
        if "=" in key_val:
            k, v = key_val.split("=", 1)
            results[k] = v
        else:
            results[key_val] = "true"

    machine.log(f"Parsed attack results: {results}")

    # The attack must actually have located the isolated target — otherwise the
    # probes below are vacuous and a setup failure would masquerade as a pass.
    assert "TARGET_PID" in results, (
        "FAIL: ancestor could not locate the isolated test-iso descendant "
        f"(sleep 5252) — the fixture did not start. Results: {results}"
    )

    # The ancestor MUST NOT be able to read the secret.
    assert "SECRET_LEAKED" not in results, (
        "CRITICAL: ancestor process recovered the isolated target's secret via "
        "/proc/environ — the UID boundary failed!"
    )

    # For an isolated target the READ-mode environ is denied by the UID gate.
    # `ENVIRON_READABLE_NO_SECRET` would mean the gate let us read but the secret
    # happened to be absent — still a boundary failure for an isolated target.
    assert "ENVIRON_DENIED" in results, (
        "FAIL: the isolated target's /proc/environ was not denied to its "
        f"same-host ancestor (UID boundary expected to deny). Results: {results}"
    )
    machine.log("PASS: ancestor /proc/environ access DENIED (UID boundary)")

    # ATTACH-mode vectors must also be denied (UID gate + ptrace_scope=2).
    assert "PTRACE_ALLOWED" not in results, (
        "CRITICAL: ancestor could ptrace-attach to the isolated target"
    )
    assert "MEM_READABLE" not in results, (
        "CRITICAL: ancestor could read the isolated target's /proc/<pid>/mem"
    )
    machine.log("PASS: ancestor ptrace-attach and /proc/mem both DENIED")
    machine.log("PASS: adversarial ancestor attack did not recover secret")

    # Clean up as root (the target subtree runs as uid X, so a testuser pkill
    # could not reach it; SIGKILL because the isoScript shell is PID 1 of its
    # namespace and ignores SIGTERM).
    machine.execute("pkill -KILL -f '[s]leep 5252' || true")
    machine.execute("pkill -KILL -f iso-target || true")
    time.sleep(0.5)


def local_core_dump_protection():
    # ============================================================
    # LOCAL INSTANCE: core dump protection
    # ============================================================

    machine.succeed("mkdir -p /tmp/cores")
    machine.succeed("echo '/tmp/cores/core.%e.%p' > /proc/sys/kernel/core_pattern")

    machine.execute(su("/run/wrappers/bin/underwrap-local test-crash"))

    core_files = machine.succeed("ls /tmp/cores/ 2>/dev/null || true").strip()
    assert core_files == "", f"FAIL: core dump produced: {core_files}"
    machine.log("PASS: no core dump on crash")


def local_symlink_confused_deputy_attack():
    # ============================================================
    # LOCAL INSTANCE: symlink confused-deputy attack must fail
    # ============================================================
    # Create a misleadingly-named symlink to the single instance
    # entrypoint. In the redesigned model the wrapper is selected by the
    # *argument* (`underwrap-local <wrapper>`), never by argv[0]; argv[0] is fully
    # user-controlled and carries no authority. So invoking the symlink and
    # passing `test-echo` must resolve test-echo's secret regardless of the
    # symlink's name.

    machine.succeed(
        "ln -sf /run/wrappers/bin/underwrap-local /tmp/totally-not-test-echo"
    )
    # argv[0] = /tmp/totally-not-test-echo, wrapper arg = test-echo.
    output = machine.succeed(su("/tmp/totally-not-test-echo test-echo")).strip()
    assert "MY_TOKEN=overwrite" in output, \
        f"FAIL: argv[0] symlink did not resolve the named wrapper (test-echo): {output}"
    # It must not have leaked a different wrapper's secret.
    assert "OTHER_SECRET" not in output, \
        f"FAIL: argv[0] symlink leaked the wrong secret: {output}"
    machine.succeed("rm -f /tmp/totally-not-test-echo")
    machine.log("PASS: argv[0] carries no authority; wrapper selection is by name argument")


def local_proc_isolation_env_mode():
    # ============================================================
    # LOCAL INSTANCE: /proc isolation in env-only mode
    # ============================================================
    # Every mode now unshares a mount namespace and remounts /proc inside
    # the new PID namespace, so even an env-only wrapper (no libsecret, no
    # files) must only see namespace-local PIDs — not host PIDs.

    ns_pids_output = machine.succeed(
        su("/run/wrappers/bin/underwrap-local test-ns-pids-env")
    ).strip()
    visible_pids = [line.strip() for line in ns_pids_output.split("\n")
                    if line.strip().isdigit()]
    machine.log(f"PIDs visible inside env-mode namespace: {visible_pids}")

    # A fresh /proc against the new PID namespace exposes only the target
    # (PID 1) and its few transient children — definitely not host PIDs.
    assert len(visible_pids) < 20, \
        f"FAIL: too many PIDs visible ({len(visible_pids)}) — env-mode /proc may show host PIDs: {visible_pids}"
    assert "1" in visible_pids, \
        f"FAIL: PID 1 not visible in env-mode namespace — /proc mount may be wrong: {visible_pids}"
    machine.log("PASS: /proc in env-only mode shows only namespace-local PIDs")


def local_proc_isolation_libsecret_mode():
    # ============================================================
    # LOCAL INSTANCE: /proc isolation in libsecret (mount namespace) mode
    # ============================================================
    # In libsecret mode, /proc is remounted inside the new PID namespace.
    # The target should only see namespace-local PIDs, not host PIDs.

    ns_pids_output = machine.succeed(
        su("/run/wrappers/bin/underwrap-local test-ns-pids")
    ).strip()
    # Extract just the numeric PIDs (ignore log lines from secretsd).
    visible_pids = [line.strip() for line in ns_pids_output.split("\n")
                    if line.strip().isdigit()]
    machine.log(f"PIDs visible inside namespace: {visible_pids}")

    # In a proper PID namespace with fresh /proc, we should see only a few
    # PIDs (PID 1 = target, plus dbus-daemon, secretsd, maybe a few more).
    # Definitely NOT hundreds of host PIDs.
    assert len(visible_pids) < 20, \
        f"FAIL: too many PIDs visible ({len(visible_pids)}) — /proc may show host PIDs: {visible_pids}"
    # PID 1 must be visible (it's the target or the namespace init).
    assert "1" in visible_pids, \
        f"FAIL: PID 1 not visible in namespace — /proc mount may be wrong: {visible_pids}"
    machine.log("PASS: /proc in libsecret mode shows only namespace-local PIDs")


def local_libsecret_mode():
    # ============================================================
    # LOCAL INSTANCE: libsecret mode (Secret Service via D-Bus)
    # ============================================================

    # Store a secret via secret-tool through the libsecret wrapper.
    # Write the secret value to a temp file using Python's file write (not
    # a shell command) to prevent it appearing in the systemd journal.
    # Shell command lines are logged by PAM session tracking.
    import tempfile, os
    with tempfile.NamedTemporaryFile(mode='w', suffix='.secret', delete=False) as f:
        f.write('my_gh_token_value')
        secret_file = f.name
    machine.copy_from_host(secret_file, "/tmp/.secretsd-test")
    os.unlink(secret_file)
    machine.succeed("chmod 600 /tmp/.secretsd-test && chown testuser /tmp/.secretsd-test")
    machine.succeed(
        'su testuser -s /bin/sh -c "'
        'cat /tmp/.secretsd-test | '
        '/run/wrappers/bin/underwrap-local test-libsecret store '
        '--label=TestToken service gh account github.com"'
    )
    machine.succeed("rm -f /tmp/.secretsd-test")
    machine.log("PASS: secret-tool store via libsecret wrapper")

    # Retrieve the secret via secret-tool through the libsecret wrapper.
    output = machine.succeed(
        'su testuser -s /bin/sh -c '
        '"/run/wrappers/bin/underwrap-local test-libsecret lookup service gh account github.com"'
    ).strip()
    # The output may include secretsd log lines on stderr mixed in;
    # the actual secret is the last line.
    last_line = output.split("\n")[-1].strip()
    assert last_line == "my_gh_token_value", f"expected 'my_gh_token_value', got '{last_line}'"
    machine.log("PASS: secret-tool lookup via libsecret wrapper")

    # Verify the secret IS in the backend storage (via the daemon).
    # Under the atomic-blob storage layout the whole libsecret keyring
    # lives in a single file at `resources/<resource_id>` where
    # `<resource_id>` is the stable backend id (not the name).
    libsecret_resource_id = LOCAL_RESOURCE_IDS["test/libsecret-store"]
    backend_blob = machine.succeed(
        "test -f "
        f"/var/lib/underwrap-backend-local/secrets/resources/{libsecret_resource_id} "
        "&& echo ok || echo none"
    ).strip()
    assert backend_blob == "ok", (
        f"expected libsecret resource blob at resources/{libsecret_resource_id}"
    )
    machine.log("PASS: libsecret data stored in backend")

    # Verify the private bus is NOT reachable from outside the namespace.
    # Run a libsecret wrapper in the background, then try to connect to the
    # bus socket from a host process. The socket should not exist on the host.
    machine.succeed(
        'su testuser -s /bin/sh -c "'
        'setsid /run/wrappers/bin/underwrap-local test-libsecret lookup service gh account github.com '
        '</dev/null >/dev/null 2>&1 &"'
    )
    time.sleep(1)
    # The bus socket is on a namespace-private tmpfs — it should not be visible.
    bus_files = machine.succeed(
        "find /tmp/.underwrap-ns-bus/ -type s 2>/dev/null || echo NONE"
    ).strip()
    assert bus_files == "NONE" or bus_files == "", \
        f"FAIL: private bus socket visible on host: {bus_files}"
    machine.log("PASS: private bus socket not reachable from host")

    # Also check that no dbus sockets under /tmp are accessible.
    tmp_bus = machine.succeed(
        "find /tmp/ -path '*underwrap*' -type s 2>/dev/null || echo NONE"
    ).strip()
    assert tmp_bus == "NONE" or tmp_bus == "", \
        f"FAIL: bus socket found in /tmp: {tmp_bus}"
    machine.log("PASS: no bus sockets in /tmp")

    machine.execute("pkill -u testuser -x underwrap-ns || true")
    time.sleep(0.5)

    # Find ALL files on the system containing the secret, outside the backend.
    # Exclude: backend dir (correct storage), /nix/ (test script literals),
    # /proc, /sys, /dev.
    #
    # These are real `-prune`s, not `-not -path` filters: pruning stops find
    # from descending into the tree at all. The Nix store in particular holds
    # the vast majority of inodes on the VM disk, and `-not -path '/nix/*'`
    # would still stat every one of them (only the grep -exec was skipped),
    # which made this single assertion take many minutes. Pruning makes the
    # leak scan dramatically faster and lower-I/O without changing what it
    # asserts.
    leak_files = machine.succeed(
        "find / "
        "-path '/nix' -prune -o "
        "-path '/proc' -prune -o "
        "-path '/sys' -prune -o "
        "-path '/dev' -prune -o "
        "-type f "
        "-not -path '/var/lib/underwrap-backend-local/secrets/*' "
        "-exec grep -l 'my_gh_token_value' {} + 2>/dev/null || echo NONE"
    ).strip()
    machine.log(f"SECRET LEAK CHECK: files containing secret outside backend: {leak_files}")
    assert leak_files == "NONE", f"SECRET LEAKED to files outside backend: {leak_files}"
    machine.log("PASS: no secret material leaked outside backend")


def local_libsecret_two_slot_compatibility():
    # ============================================================
    # LOCAL INSTANCE: two items sharing `service` but differing in `username`
    # ============================================================
    # Models gh's login pattern: a user-slot item keyed by
    # (service, username=<account>) plus an active-slot item keyed by
    # (service, username=""). secretsd must hold both as distinct items.

    import tempfile, os
    with tempfile.NamedTemporaryFile(mode='w', suffix='.secret', delete=False) as f:
        f.write('libsecret_two_slot_value')
        secret_file = f.name
    machine.copy_from_host(secret_file, "/tmp/.two-slot-secret")
    os.unlink(secret_file)
    machine.succeed("chmod 600 /tmp/.two-slot-secret && chown testuser /tmp/.two-slot-secret")

    # User slot: (service=gh:github.com, username=rschaffar).
    machine.succeed(
        'su testuser -s /bin/sh -c "'
        'cat /tmp/.two-slot-secret | '
        '/run/wrappers/bin/underwrap-local test-libsecret store '
        '--label=UserSlot service gh:github.com username rschaffar"'
    )
    user_out = machine.succeed(
        'su testuser -s /bin/sh -c '
        '"/run/wrappers/bin/underwrap-local test-libsecret lookup '
        'service gh:github.com username rschaffar"'
    ).strip()
    user_last = user_out.split("\n")[-1].strip()
    assert user_last == "libsecret_two_slot_value", \
        f"user slot mismatch: got '{user_last}'"
    machine.log("PASS: user slot store/lookup")

    # Active slot: (service=gh:github.com, username="").
    machine.succeed(
        'su testuser -s /bin/sh -c "'
        'cat /tmp/.two-slot-secret | '
        "/run/wrappers/bin/underwrap-local test-libsecret store "
        "--label=ActiveSlot service gh:github.com username ''\""
    )
    active_out = machine.succeed(
        'su testuser -s /bin/sh -c "'
        "/run/wrappers/bin/underwrap-local test-libsecret lookup "
        "service gh:github.com username ''\""
    ).strip()
    active_last = active_out.split("\n")[-1].strip()
    assert active_last == "libsecret_two_slot_value", \
        f"active slot mismatch: got '{active_last}'"
    machine.log("PASS: active slot store/lookup")

    machine.succeed("rm -f /tmp/.two-slot-secret")
    machine.log("PASS: libsecret two-slot compatibility (gh-style)")


def local_directory_credentials():
    # ============================================================
    # LOCAL INSTANCE: directory credentials
    # ============================================================

    # New grant model: one Unlocked grant per `(client_instance,
    # wrapper, resource)` tuple. Read access is then unconditional;
    # wrapper-side writes are gated by the per-grant `writable: bool`
    # flag (which `grant_local_unlocked` sets true).
    for wrapper in [
        "test-dir-write",
        "test-dir-read",
        "test-dir-readonly-write",
        "test-dir-sleep",
        "test-dir-symlink",
    ]:
        grant_local_unlocked(wrapper)

    # Seed a directory credential through a read-write wrapper.
    machine.succeed(su("/run/wrappers/bin/underwrap-local test-dir-write seed"))
    machine.log("PASS: directory credential seed wrapper succeeded")

    # Read the mounted directory through a read-only wrapper.
    dir_output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-dir-read")).strip().split("\n")
    assert dir_output[0].strip() == "token-seed", f"expected token-seed, got {dir_output}"
    machine.log("PASS: directory credential read-only wrapper can read the mounted directory")

    # Update content through the read-write wrapper.
    machine.succeed(su("/run/wrappers/bin/underwrap-local test-dir-write update"))
    dir_output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-dir-read")).strip().split("\n")
    assert dir_output[0].strip() == "token-updated", f"expected token-updated, got {dir_output}"
    machine.log("PASS: directory read-write wrapper updates backend-managed files")

    # Verify the in-blob files section reflects the writeback (single
    # resource per wrapper, so all files land in dir-a's blob).
    dir_a_files = sorted(
        json.loads(
            base64.b64decode(
                admin_get_blob(ADMIN_LOCAL, "local", "test/dir-a")
            )
        )["files"].keys()
    )
    assert dir_a_files == [
        ".config/underwrap-dir-a/sub/nested",
        ".config/underwrap-dir-a/token",
    ], (
        f"unexpected dir-a blob files: {dir_a_files}"
    )
    machine.log("PASS: resource blob carries the writeback")

    # While a wrapped target is running, a separate same-caller host process must not
    # be able to access the mounted secret files from the host namespace view.
    machine.succeed(
        "su testuser -s /bin/sh -c '"
        "setsid /run/wrappers/bin/underwrap-local test-dir-sleep 300 </dev/null >/dev/null 2>&1 &'"
    )
    time.sleep(1)

    host_view = machine.execute(
        "su testuser -s /bin/sh -c 'cat $HOME/.config/underwrap-dir-a/token 2>/dev/null'"
    )[0]
    assert host_view != 0, "FAIL: same-caller host process can read directory credential file"

    host_ls = machine.execute(
        "su testuser -s /bin/sh -c 'ls $HOME/.config/underwrap-dir-a >/dev/null 2>&1'"
    )[0]
    assert host_ls != 0, "FAIL: same-caller host process can list mounted directory"

    host_find = machine.succeed(
        "su testuser -s /bin/sh -c 'find $HOME/.config/underwrap-dir-a -maxdepth 2 -type f 2>/dev/null || true'"
    ).strip()
    assert host_find == "", f"FAIL: host view exposes mounted directory files: {host_find}"
    machine.log("PASS: same-caller host processes cannot list mounted directory or access files in it")

    machine.execute("pkill -KILL -f '[s]leep 300' || true")
    time.sleep(0.5)

    # The mounted directory must disappear from normal host view after the wrapper exits.
    post_exit = machine.execute(
        "su testuser -s /bin/sh -c 'test -e $HOME/.config/underwrap-dir-a/token'"
    )[0]
    assert post_exit != 0, "FAIL: directory credential file remained visible after wrapper exit"
    machine.log("PASS: directory credentials leave no host-visible files after wrapper exit")

    # Symlinks in the managed staging tree must never reach the
    # backend. Under the atomic-blob model the staging tmpfs is
    # always RW (so the target's `ln -s` succeeds in-namespace), but
    # the end-of-run walk refuses to follow symlinks and drops the
    # writeback. The backend blob stays unchanged.
    before = admin_get_blob(ADMIN_LOCAL, "local", "test/dir-a")
    machine.succeed(su("/run/wrappers/bin/underwrap-local test-dir-symlink"))
    after = admin_get_blob(ADMIN_LOCAL, "local", "test/dir-a")
    assert after == before, (
        "symlink-creating wrapper unexpectedly mutated the backend blob"
    )
    machine.log("PASS: directory writeback drops runs that introduce symlinks")

    # Seed the per-grant `writable=true` for the wrappers used by
    # the atomic-writeback scenarios. They share the dir-a resource.
    for wrapper in [
        "test-dir-write-then-sleep",
        "test-dir-touch",
        "test-dir-env-probe",
        "test-dir-env-override-probe",
    ]:
        grant_local_unlocked(wrapper)

    # While a libsecret wrapper is running, the private bus must also remain
    # inaccessible to a same-caller host process.
    machine.succeed(
        'su testuser -s /bin/sh -c "'
        'setsid /run/wrappers/bin/underwrap-local test-libsecret lookup service gh account github.com '
        '</dev/null >/dev/null 2>&1 &"'
    )
    time.sleep(1)
    libsecret_host_view = machine.succeed(
        "su testuser -s /bin/sh -c 'find /tmp/.underwrap-ns-bus -type s 2>/dev/null || true'"
    ).strip()
    assert libsecret_host_view == "", f"FAIL: same-caller host process sees private bus socket: {libsecret_host_view}"
    libsecret_ls = machine.execute(
        "su testuser -s /bin/sh -c 'ls /tmp/.underwrap-ns-bus >/dev/null 2>&1'"
    )[0]
    assert libsecret_ls != 0, "FAIL: same-caller host process can list private libsecret bus mountpoint"
    machine.log("PASS: same-caller host process cannot list private libsecret runtime directories or see the bus socket")
    machine.execute("pkill -u testuser -x underwrap-ns || true")
    time.sleep(0.5)


def local_atomic_writeback():
    # ============================================================
    # LOCAL INSTANCE: atomic resource-blob writeback decision tree
    #
    # End-to-end coverage of the wrapper-resource Checkout / Checkin /
    # UsageSignal / drop+warn paths. Each scenario starts from a
    # known-good blob seeded via `test-dir-write seed` and drives a
    # wrapper that exercises one decision-tree arm.
    # ============================================================

    DIR_A = "test/dir-a"

    def reset_blob_state():
        """Re-seed the dir-a blob to a known state. The
        `test-dir-write seed` script writes a deterministic set of
        files via the writable grant; we then re-fetch the blob bytes
        so subsequent scenarios can byte-compare against this baseline."""
        grant_local_unlocked("test-dir-write")
        machine.succeed(su("/run/wrappers/bin/underwrap-local test-dir-write seed"))
        return admin_get_blob(ADMIN_LOCAL, "local", DIR_A)

    def journal_lines_since(boot_offset, *, units=None, grep=None):
        """Capture journal output emitted since `boot_offset` (a
        cursor string from journalctl -o export --show-cursor=)."""
        units_args = ""
        if units:
            units_args = " ".join(f"-u {u}" for u in units) + " "
        cmd = f"journalctl --no-pager --after-cursor={shlex.quote(boot_offset)} {units_args}--output=cat"
        if grep:
            cmd += f" | grep -F {shlex.quote(grep)} || true"
        return machine.succeed(cmd).strip().split("\n") if machine.succeed(cmd).strip() else []

    def journal_cursor():
        """Snapshot the current journal cursor so a follow-up call to
        journal_lines_since() can scope its grep to events after this
        point. journalctl prints the cursor on stderr with -o export
        --show-cursor; the simplest reliable form is to read the very
        last entry's cursor via -n 1 -o export."""
        out = machine.succeed("journalctl -n 1 -o export --no-pager")
        for line in out.splitlines():
            if line.startswith("__CURSOR="):
                return line[len("__CURSOR=") :]
        raise AssertionError(f"journalctl produced no cursor: {out!r}")

    # ----------------------------------------------------------
    # Scenario 1: clean run with no changes
    # Wrapper reads the bound files but never writes. Backend blob
    # bytes are byte-identical after the run. Journal contains no
    # writeback_rejected event. `last_used_at` is bumped.
    # ----------------------------------------------------------
    baseline_a = reset_blob_state()
    grant_local_unlocked("test-dir-read")

    before_pairs = admin_list_pairs(ADMIN_LOCAL, "local")
    before_pair = find_pair(before_pairs, "test-dir-read", DIR_A)
    before_last_used = before_pair["last_used_at"] if before_pair else None

    cursor = journal_cursor()
    machine.succeed(su("/run/wrappers/bin/underwrap-local test-dir-read"))
    time.sleep(0.5)

    after_a = admin_get_blob(ADMIN_LOCAL, "local", DIR_A)
    assert after_a == baseline_a, "scenario 1: dir-a blob changed after clean run"

    rejected = machine.succeed(
        f"journalctl --no-pager --after-cursor={shlex.quote(cursor)} "
        "| grep -F 'writeback_rejected' || true"
    ).strip()
    assert rejected == "", f"scenario 1: unexpected writeback_rejected event: {rejected!r}"

    after_pairs = admin_list_pairs(ADMIN_LOCAL, "local")
    after_pair = find_pair(after_pairs, "test-dir-read", DIR_A)
    assert after_pair is not None and after_pair["last_used_at"] is not None, (
        f"scenario 1: last_used_at not bumped for (test-dir-read, dir-a): {after_pair}"
    )
    if before_last_used is not None:
        assert after_pair["last_used_at"] >= before_last_used, (
            "scenario 1: last_used_at regressed"
        )
    machine.log("PASS: scenario 1 — clean run leaves backend blob unchanged, last_used bumped")

    # ----------------------------------------------------------
    # Scenario 2: changed run with writable grant — Checkin path
    # The `test-dir-write update` wrapper rewrites the seeded files;
    # the backend blob's bytes change accordingly, and a second run
    # via `test-dir-read` sees the new content.
    # ----------------------------------------------------------
    machine.succeed(su("/run/wrappers/bin/underwrap-local test-dir-write update"))
    time.sleep(0.5)
    updated_a = admin_get_blob(ADMIN_LOCAL, "local", DIR_A)
    assert updated_a != baseline_a, (
        "scenario 2: dir-a blob did not change after writable update run"
    )
    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-dir-read")).strip().split("\n")
    assert output[0].strip() == "token-updated", (
        f"scenario 2: second run did not observe new dir-a content: {output}"
    )
    machine.log("PASS: scenario 2 — writable Checkin persists, second run observes new state")

    # ----------------------------------------------------------
    # Scenario 3: changed run with read-only grant — quiet drop
    # The `test-dir-readonly-write` wrapper writes mutations through a
    # grant with `writable=false`. The daemon emits no Checkin (ns
    # has writable=false from Checkout), the backend blob is
    # unchanged, the journal carries the structured
    # `AUDIT=writeback_rejected REASON=grant_readonly` event (routed
    # via journald's native socket so it does not clutter the
    # wrapper's stderr), and stderr contains only the human-readable
    # line — which names both the resource and its id.
    # ----------------------------------------------------------
    baseline_a = reset_blob_state()
    admin_upsert_grant(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        "test-dir-readonly-write",
        DIR_A,
        {"kind": "unlocked"},
        writable=False,
    )

    cursor = journal_cursor()
    stderr_capture = machine.succeed(
        su(
            "/run/wrappers/bin/underwrap-local test-dir-readonly-write update 2>/tmp/ro-stderr.log "
            "&& cat /tmp/ro-stderr.log"
        )
    )
    time.sleep(0.5)

    after_a = admin_get_blob(ADMIN_LOCAL, "local", DIR_A)
    assert after_a == baseline_a, "scenario 3: dir-a blob changed despite read-only grant"

    # The structured audit event is delivered via journald's native
    # socket; the human-readable warning goes to wrapper stderr. The
    # warning identifies the resource by name and id.
    assert '"audit":"writeback_rejected"' not in stderr_capture, (
        f"scenario 3: audit JSON leaked onto stderr instead of journald: {stderr_capture!r}"
    )
    assert "grant is read-only" in stderr_capture, (
        f"scenario 3: missing human-readable stderr warning: {stderr_capture!r}"
    )
    assert f"resource '{DIR_A}'" in stderr_capture, (
        f"scenario 3: stderr warning missing resource name: {stderr_capture!r}"
    )

    journal_after = machine.succeed(
        f"journalctl --no-pager --after-cursor={shlex.quote(cursor)} "
        "SYSLOG_IDENTIFIER=underwrap-ns --output=export || true"
    )
    assert "AUDIT=writeback_rejected" in journal_after, (
        f"scenario 3: missing writeback_rejected event in journal: {journal_after!r}"
    )
    assert "REASON=grant_readonly" in journal_after, (
        f"scenario 3: missing grant_readonly reason in journal: {journal_after!r}"
    )
    assert f"RESOURCE_NAME={DIR_A}" in journal_after, (
        f"scenario 3: journal event missing resource name: {journal_after!r}"
    )
    machine.log(
        "PASS: scenario 3 — read-only grant drops mutations; journal records audit event, stderr shows named warning"
    )

    # ----------------------------------------------------------
    # Scenario 4: suppression env var
    # Same setup as scenario 3 but with UNDERWRAP_SUPPRESS_READONLY_WARNING=1
    # in the wrapper's environment. Journal still records the event
    # (suppression only gates the stderr warning); the human-readable
    # line is silenced; the target process does NOT see the env var
    # (env_clear whitelist).
    # ----------------------------------------------------------
    baseline_a = reset_blob_state()
    admin_upsert_grant(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        "test-dir-readonly-write",
        DIR_A,
        {"kind": "unlocked"},
        writable=False,
    )

    cursor = journal_cursor()
    stderr_capture = machine.succeed(
        "su testuser -s /bin/sh -c '"
        "UNDERWRAP_SUPPRESS_READONLY_WARNING=1 "
        "/run/wrappers/bin/underwrap-local test-dir-readonly-write update 2>/tmp/ro-stderr.log; "
        "cat /tmp/ro-stderr.log'"
    )
    time.sleep(0.5)

    # The audit event still fires (journal-side); stderr should now
    # be completely silent under suppression — neither the audit JSON
    # nor the human-readable line should appear.
    assert '"audit":"writeback_rejected"' not in stderr_capture, (
        f"scenario 4: audit JSON leaked to stderr instead of journald: {stderr_capture!r}"
    )
    assert "grant is read-only" not in stderr_capture, (
        f"scenario 4: human-readable stderr line leaked despite suppression env: {stderr_capture!r}"
    )

    journal_after = machine.succeed(
        f"journalctl --no-pager --after-cursor={shlex.quote(cursor)} "
        "SYSLOG_IDENTIFIER=underwrap-ns --output=export || true"
    )
    assert "AUDIT=writeback_rejected" in journal_after, (
        f"scenario 4: journal missing audit event under suppression: {journal_after!r}"
    )
    assert "REASON=grant_readonly" in journal_after, (
        f"scenario 4: journal missing grant_readonly reason: {journal_after!r}"
    )

    # Env clear + forced HOME/XDG: the target must not see
    # UNDERWRAP_SUPPRESS_READONLY_WARNING (or any other UNDERWRAP_*)
    # propagated from the wrapper invocation environment, and caller-
    # supplied HOME/XDG values must be replaced with the trusted
    # underwrap defaults.
    grant_local_unlocked("test-dir-env-probe")
    target_env = machine.succeed(
        "su testuser -s /bin/sh -c '"
        "UNDERWRAP_SUPPRESS_READONLY_WARNING=1 "
        "HOME=/tmp/evil-home "
        "XDG_RUNTIME_DIR=/tmp/evil-run "
        "XDG_CONFIG_HOME=/tmp/evil-config "
        "XDG_DATA_HOME=/tmp/evil-data "
        "XDG_CACHE_HOME=/tmp/evil-cache "
        "XDG_STATE_HOME=/tmp/evil-state "
        "/run/wrappers/bin/underwrap-local test-dir-env-probe'"
    )
    leaked = [
        line for line in target_env.splitlines() if line.startswith("UNDERWRAP_")
    ]
    assert leaked == [], (
        f"scenario 4: UNDERWRAP_* vars leaked into target environment: {leaked}"
    )
    assert "HOME=/home/testuser" in target_env, target_env
    assert "XDG_RUNTIME_DIR=/run/underwrap-test-runtime/1099" in target_env, target_env
    assert "XDG_CONFIG_HOME=/home/testuser/.config" in target_env, target_env
    assert "XDG_DATA_HOME=/home/testuser/.local/share" in target_env, target_env
    assert "XDG_CACHE_HOME=/home/testuser/.cache" in target_env, target_env
    assert "XDG_STATE_HOME=/home/testuser/.local/state" in target_env, target_env

    grant_local_unlocked("test-dir-env-override-probe")
    override_env = machine.succeed(
        "su testuser -s /bin/sh -c '"
        "HOME=/tmp/evil-home "
        "XDG_RUNTIME_DIR=/tmp/evil-run "
        "XDG_CONFIG_HOME=/tmp/evil-config "
        "/run/wrappers/bin/underwrap-local test-dir-env-override-probe'"
    )
    assert "HOME=/tmp/underwrap-home-override" in override_env, override_env
    assert "XDG_RUNTIME_DIR=/run/underwrap-test-runtime/1099" in override_env, override_env
    assert "XDG_CONFIG_HOME=/tmp/underwrap-config-override" in override_env, override_env
    assert "XDG_DATA_HOME=/tmp/underwrap-home-override/.local/share" in override_env, override_env
    assert "XDG_CACHE_HOME=/tmp/underwrap-home-override/.cache" in override_env, override_env
    assert "XDG_STATE_HOME=/tmp/underwrap-home-override/.local/state" in override_env, override_env
    machine.log(
        "PASS: scenario 4 — suppression stays private, caller HOME/XDG spoofing is ignored, and envSet overrides apply cleanly"
    )

    # ----------------------------------------------------------
    # Scenario 5: always-writable namespace mount
    # Even with a read-only grant, the in-namespace bind mount is RW
    # to the target. A `touch` inside the bound directory must exit 0
    # (the kernel allowed the write); the file does not persist to
    # the backend (covered by scenario 3's blob check, repeated here).
    # ----------------------------------------------------------
    baseline_a = reset_blob_state()
    admin_upsert_grant(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        "test-dir-touch",
        DIR_A,
        {"kind": "unlocked"},
        writable=False,
    )
    # touch on the bound directory under a read-only grant — must
    # succeed (target sees a writable mount).
    machine.succeed(su("/run/wrappers/bin/underwrap-local test-dir-touch"))
    after_a = admin_get_blob(ADMIN_LOCAL, "local", DIR_A)
    assert after_a == baseline_a, (
        "scenario 5: read-only-grant touch persisted to backend"
    )
    machine.log("PASS: scenario 5 — read-only grant accepts target touch, drops it at writeback")

    # ----------------------------------------------------------
    # Scenario 6: server-side reject on mid-run grant flip
    # Wrapper Checkouts a writable grant, then the test driver flips
    # the grant to writable=false while the target sleeps. On exit,
    # ns dispatches Checkin against the now-stale grant; the daemon
    # re-evaluates the policy and returns Unauthorized. The journal
    # carries `writeback_rejected reason=server_rejected` and the
    # backend blob is unchanged.
    # ----------------------------------------------------------
    baseline_a = reset_blob_state()
    admin_upsert_grant(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        "test-dir-write-then-sleep",
        DIR_A,
        {"kind": "unlocked"},
        writable=True,
    )

    machine.execute("rm -f /tmp/dir-write-then-sleep.ready")
    cursor = journal_cursor()
    # The script sleeps for ~4s after writing; we flip the grant
    # during the sleep, then wait for the natural exit.
    machine.succeed(
        "su testuser -s /bin/sh -c '"
        "setsid /run/wrappers/bin/underwrap-local test-dir-write-then-sleep "
        ">/tmp/wts-stdout.log 2>/tmp/wts-stderr.log </dev/null &'"
    )

    # Wait for the target to finish writing.
    deadline = time.time() + 10
    flipped = False
    while time.time() < deadline:
        rc = machine.execute("test -e /tmp/dir-write-then-sleep.ready")[0]
        if rc == 0:
            flipped = True
            break
        time.sleep(0.2)
    if not flipped:
        raise AssertionError("scenario 6: target never signaled write completion")

    # Flip the grant to read-only mid-run.
    admin_upsert_grant(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        "test-dir-write-then-sleep",
        DIR_A,
        {"kind": "unlocked"},
        writable=False,
    )

    # Wait for the natural sleep + ns writeback to complete. The
    # daemon-end of the channel exits when ns sends Checkin → wait
    # for the audit event to appear in the journal (which is where
    # the structured trail lives now).
    deadline = time.time() + 30
    journal_after = ""
    while time.time() < deadline:
        journal_after = machine.succeed(
            f"journalctl --no-pager --after-cursor={shlex.quote(cursor)} "
            "SYSLOG_IDENTIFIER=underwrap-ns --output=export || true"
        )
        if "AUDIT=writeback_rejected" in journal_after:
            break
        time.sleep(0.5)

    after_a = admin_get_blob(ADMIN_LOCAL, "local", DIR_A)
    assert after_a == baseline_a, "scenario 6: backend blob changed despite mid-run flip"

    assert "AUDIT=writeback_rejected" in journal_after, (
        f"scenario 6: missing audit event in journal after grant flip: {journal_after!r}"
    )
    assert "REASON=server_rejected" in journal_after, (
        f"scenario 6: missing server_rejected reason in journal: {journal_after!r}"
    )

    stderr_capture = machine.succeed("cat /tmp/wts-stderr.log || true")
    assert '"audit":"writeback_rejected"' not in stderr_capture, (
        f"scenario 6: audit JSON leaked to stderr instead of journald: {stderr_capture!r}"
    )
    assert "grant is read-only" in stderr_capture, (
        f"scenario 6: missing human-readable stderr warning: {stderr_capture!r}"
    )
    assert f"resource '{DIR_A}'" in stderr_capture, (
        f"scenario 6: stderr warning missing resource name: {stderr_capture!r}"
    )
    machine.log(
        "PASS: scenario 6 — server rejects Checkin after mid-run grant flip; journal records server_rejected"
    )


def encrypted_store_in_memory_keys():
    """Verifies the in-memory-master-key flow end-to-end against the
    local backend's admin API.

    Phases:
      1. Create a fresh encrypted store via the admin API and capture
         the one-shot hex key from the create response. The store
         must not leave a `.key` file anywhere under the backend's
         state directory.
      2. Seed a known plaintext through the admin secret-write
         endpoint and verify it round-trips through admin secret-read.
      3. Restart `underwrap-backend-local`. After the restart the
         store boots locked — admin secret-read must return
         `store_locked`.
      4. Unlock with the wrong hex — refused with `key_mismatch`,
         store stays locked.
      5. Unlock with the right hex — succeeds. Secret round-trips
         again under the original key.
      6. Rotate the key — every resource is re-encrypted under a
         fresh master key. The original plaintext still reads back.
      7. Restart again — store locks. Unlocking with the *old* key
         is refused; unlocking with the *new* key succeeds.
      8. Explicit lock — admin lock endpoint drops the in-memory key
         and subsequent reads fail with `store_locked` until unlock.
    """
    machine.log("encrypted_store_in_memory_keys: starting")

    # ---- 1. create the store --------------------------------------
    create_resp = admin_api(
        ADMIN_LOCAL,
        "/v1/admin/stores",
        {"name": "vault", "backend": "encrypted"},
    )
    store_id = create_resp["store"]["id"]
    key_hex_original = create_resp["key_hex"]
    assert isinstance(key_hex_original, str) and len(key_hex_original) == 64
    assert create_resp["store"]["locked"] is False
    assert create_resp["store"]["backend"] == "encrypted"

    # No master-key file lands on disk anywhere under the backend's
    # state directories. Use `find` to surface any accidental .key
    # files anywhere reachable.
    stray = machine.succeed(
        "find /var/lib/underwrap-backend-local /run/underwrap-backend-local "
        "-name '*.key' 2>/dev/null || true"
    ).strip()
    assert stray == "", f"unexpected on-disk .key files: {stray}"

    # ---- 2. seed a known plaintext and verify round-trip ----------
    # Admin secret writes/reads route through the resource blob shim
    # without requiring a pre-existing resource record, so we can
    # exercise the store's encryption without first wiring up
    # resources / pairs / grants.
    resource_id = "vault/greeting"
    slot_payload = base64.b64encode(b"hello-locked").decode()
    secret_path = (
        f"/v1/admin/stores/{store_id}/secrets/{_seg(resource_id)}/env/GREETING"
    )
    admin_api(
        ADMIN_LOCAL,
        secret_path,
        {"material_b64": slot_payload},
        method="PUT",
    )

    def read_secret():
        return admin_api(ADMIN_LOCAL, secret_path, method="GET")

    assert read_secret()["material_b64"] == slot_payload, (
        "unencrypted round-trip failed"
    )

    # ---- 3. restart → boots locked --------------------------------
    machine.succeed("systemctl restart underwrap-backend-local")
    machine.wait_for_unit("underwrap-backend-local.service")
    # The admin listener takes a moment to come back up.
    machine.wait_until_succeeds(
        f"curl -fsS --cacert {CA_CERT} "
        f"{ADMIN_LOCAL}/v1/admin/auth/bootstrap/status >/dev/null"
    )
    # We have to re-login our session against the listener because
    # in-memory session state was reset. The on-disk bootstrap
    # sentinel persists across restarts, so the public bootstrap
    # endpoint now refuses; use the login path instead.
    login_admin_session(ADMIN_LOCAL)

    stores_after_restart = admin_api(
        ADMIN_LOCAL, "/v1/admin/stores", None, method="GET"
    )["items"]
    locked_item = next(s for s in stores_after_restart if s["id"] == store_id)
    assert locked_item["locked"] is True, (
        f"store must boot locked: {locked_item}"
    )
    assert locked_item["actions"]["can_unlock"] is True
    assert locked_item["actions"]["can_lock"] is False

    locked_read = admin_api_expect_status(
        ADMIN_LOCAL,
        secret_path,
        None,
        status=503,
        method="GET",
    )
    assert locked_read["code"] == "store_locked", locked_read

    # ---- 4. unlock with wrong hex → key_mismatch ------------------
    wrong_unlock = admin_api_expect_status(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/unlock",
        {"key_hex": "0" * 64},
        status=403,
    )
    assert wrong_unlock["code"] == "key_mismatch", wrong_unlock
    stores_still_locked = admin_api(
        ADMIN_LOCAL, "/v1/admin/stores", None, method="GET"
    )["items"]
    assert next(s for s in stores_still_locked if s["id"] == store_id)[
        "locked"
    ] is True, "wrong-hex unlock must not transition the registry"

    # ---- 5. unlock with right hex → succeeds ----------------------
    admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/unlock",
        {"key_hex": key_hex_original},
    )
    assert read_secret()["material_b64"] == slot_payload, (
        "round-trip after unlock failed"
    )

    # ---- 6. rotate key → existing plaintext still readable --------
    rotate_resp = admin_api(
        ADMIN_LOCAL, f"/v1/admin/stores/{store_id}/rotate-key", {}
    )
    key_hex_rotated = rotate_resp["key_hex"]
    assert key_hex_rotated != key_hex_original
    assert len(key_hex_rotated) == 64
    assert read_secret()["material_b64"] == slot_payload, (
        "round-trip after rotate-key failed"
    )

    # ---- 7. restart → locked, old key refused, new key unlocks ----
    machine.succeed("systemctl restart underwrap-backend-local")
    machine.wait_for_unit("underwrap-backend-local.service")
    machine.wait_until_succeeds(
        f"curl -fsS --cacert {CA_CERT} "
        f"{ADMIN_LOCAL}/v1/admin/auth/bootstrap/status >/dev/null"
    )
    login_admin_session(ADMIN_LOCAL)
    old_key_attempt = admin_api_expect_status(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/unlock",
        {"key_hex": key_hex_original},
        status=403,
    )
    assert old_key_attempt["code"] == "key_mismatch", old_key_attempt
    admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/unlock",
        {"key_hex": key_hex_rotated},
    )
    assert read_secret()["material_b64"] == slot_payload

    # ---- 8. explicit lock --------------------------------------------
    admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/lock",
        {},
    )
    locked_after_lock = admin_api_expect_status(
        ADMIN_LOCAL,
        secret_path,
        None,
        status=503,
        method="GET",
    )
    assert locked_after_lock["code"] == "store_locked"

    # Restore for any later tests / leave the store unlocked at the end.
    admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/unlock",
        {"key_hex": key_hex_rotated},
    )

    machine.log("PASS: encrypted store keeps master keys in memory only")


def admin_api_expect_status(url, path, payload, *, status, method="POST"):
    """Like `admin_api` but expects a specific non-2xx status and
    returns the decoded JSON error body. Used by the
    encrypted-store-locked tests where the interesting case is the
    `code` field on a 4xx / 5xx response.
    """
    is_state_changing = method not in ("GET", "HEAD", "OPTIONS")
    cookie_jar = _COOKIE_JAR.get(url)
    csrf = _CSRF_TOKEN.get(url)
    cmd = [
        "curl", "-sS",
        "-o", "/tmp/admin-err-body",
        "-w", "%{http_code}",
        "--cacert", CA_CERT,
        "-b", cookie_jar,
        "-X", method,
        "-H", f"Origin: {url}",
    ]
    if is_state_changing:
        cmd += ["-H", f"X-CSRF-Token: {csrf}"]
    if payload is not None:
        cmd += ["-H", "Content-Type: application/json", "-d", json.dumps(payload)]
    cmd += [f"{url}{path}"]
    quoted = " ".join(shlex.quote(part) for part in cmd)
    code = machine.succeed(quoted).strip()
    assert code == str(status), (
        f"expected HTTP {status} from {path}, got {code}"
    )
    body = machine.succeed("cat /tmp/admin-err-body")
    return json.loads(body) if body.strip() else {}


def remote_service_lifecycle():
    # ============================================================
    # REMOTE INSTANCE: backend service lifecycle
    # ============================================================

    # Seed an Unlocked grant for the remote daemon's
    # `(client_instance, wrapper, resource)` tuple. The remote daemon
    # has a single wrapper (`remote-echo`) declaring the
    # `remote/token` resource.
    admin_upsert_grant(
        ADMIN_REMOTE,
        "remote",
        REMOTE_CLIENT_INSTANCE,
        "remote-echo",
        "remote/token",
        {"kind": "unlocked"},
        writable=True,
    )

    # Store secret in backend through the admin API.
    admin_write_secret(ADMIN_REMOTE, "remote", "remote-echo", "remote/token/env/REMOTE_TOKEN", "secret_via_service")

    # With backend service up: succeed.
    output = machine.succeed(su("XDG_RUNTIME_DIR=/tmp/evil-run /run/wrappers/bin/underwrap-tunnel remote-echo"))
    assert "REMOTE_TOKEN=secret_via_service" in output, f"expected REMOTE_TOKEN: {output}"
    assert "XDG_RUNTIME_DIR=/run/user/1099" in output, f"expected forced default XDG_RUNTIME_DIR: {output}"
    machine.log("PASS: remote secret via backend service")

    # No secret on local frontend store disk.
    machine.fail("test -f /var/lib/underwrap-backend-local/secrets/remote/token")
    machine.log("PASS: no remote secret on local frontend disk")

    # Stop backend service: fail.
    machine.succeed("systemctl stop underwrap-backend.service")
    time.sleep(0.5)
    machine.fail(su("/run/wrappers/bin/underwrap-tunnel remote-echo"))
    machine.log("PASS: wrapper fails when backend service is down")

    # Restart backend service: succeed again.
    machine.succeed("systemctl start underwrap-backend.service")
    machine.wait_for_unit("underwrap-backend.service")
    time.sleep(1)
    output = machine.succeed(su("XDG_RUNTIME_DIR=/tmp/evil-run /run/wrappers/bin/underwrap-tunnel remote-echo"))
    assert "REMOTE_TOKEN=secret_via_service" in output
    assert "XDG_RUNTIME_DIR=/run/user/1099" in output
    machine.log("PASS: wrapper works after backend service restart")


def local_backend_managed_grants():
    # ============================================================
    # LOCAL INSTANCE: backend-managed grants
    # ============================================================
    #
    # Grants are now keyed on `(client_instance, wrapper, resource)`.
    # This section exercises the full lifecycle (no grant → denied,
    # Unlocked → allowed, deleted → denied, UnlockedUntil with a past
    # deadline → denied).

    # Remove the seed grant from the earlier section so we can
    # exercise the "no grant" path.
    admin_delete_grant(ADMIN_LOCAL, "local", LOCAL_CLIENT_INSTANCE, "test-echo2", "test/other")
    machine.fail(su("/run/wrappers/bin/underwrap-local test-echo2 hello"))
    machine.log("PASS: wrapper with no agent grant is denied")

    # Unlocked grant → wrapper can read.
    admin_upsert_grant(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        "test-echo2",
        "test/other",
        {"kind": "unlocked"},
        reason="vm test allow",
    )
    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo2 hello"))
    assert "hello" in output
    machine.log("PASS: active agent grant allows wrapper action")

    # Deleting the grant revokes future reads.
    admin_delete_grant(ADMIN_LOCAL, "local", LOCAL_CLIENT_INSTANCE, "test-echo2", "test/other")
    machine.fail(su("/run/wrappers/bin/underwrap-local test-echo2 hello"))
    machine.log("PASS: deleting agent grant revokes future wrapper action")

    # UnlockedUntil with a deadline guaranteed in the past
    # (1970-01-02) is denied lazily on the next request — no flaky
    # sleep needed and no risk of host/guest clock skew.
    admin_upsert_grant(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        "test-echo2",
        "test/other",
        {"kind": "unlocked-until", "until_epoch": 86400},
        reason="expired allow",
    )
    machine.fail(su("/run/wrappers/bin/underwrap-local test-echo2 hello"))
    machine.log("PASS: expired UnlockedUntil grant is not honored")

    # Clean up so the next test starts fresh.
    admin_delete_grant(ADMIN_LOCAL, "local", LOCAL_CLIENT_INSTANCE, "test-echo2", "test/other")



def local_backend_policy_control_center():
    # ============================================================
    # LOCAL INSTANCE: backend-managed grant mode control
    # ============================================================

    # Remove the seed grant so we can prove "no grant → denied".
    admin_delete_grant(ADMIN_LOCAL, "local", LOCAL_CLIENT_INSTANCE, "test-echo", "test/token")
    machine.fail(su("/run/wrappers/bin/underwrap-local test-echo"))
    machine.log("PASS: wrapper with no grant is denied")

    # Unlocked grant → wrapper can read.
    grant_local_unlocked("test-echo")
    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo"))
    assert "MY_TOKEN=overwrite" in output, f"expected MY_TOKEN after grant: {output}"
    machine.log("PASS: Unlocked grant allows wrapper read")

    # Locked grant → wrapper denied.
    admin_upsert_grant(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        "test-echo",
        "test/token",
        {"kind": "locked"},
        reason="maintenance",
    )
    machine.fail(su("/run/wrappers/bin/underwrap-local test-echo"))
    machine.log("PASS: Locked grant denies wrapper read")

    # Re-Unlock → wrapper allowed again.
    grant_local_unlocked("test-echo")
    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo"))
    assert "MY_TOKEN=overwrite" in output, f"expected MY_TOKEN after re-grant: {output}"
    machine.log("PASS: re-Unlocking the grant restores wrapper access")

    # `local-test-locked` is wired up by the test harness as a wrapper
    # that historically required a local unlock; under the grant
    # model it simply needs an Unlocked grant.
    grant_local_unlocked("test-locked")
    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-locked"))
    assert "LOCKED_TOKEN=locked_secret" in output, (
        f"expected formerly-locked wrapper to work via grant model: {output}"
    )
    machine.log("PASS: grant model replaces local unlock when active")


def instance_isolation():
    # ============================================================
    # INSTANCE ISOLATION: cross-instance access blocked
    # ============================================================

    # The local instance's `underwrap-local` entrypoint cannot reach tunnel
    # instance secrets: it connects only to the local instance socket
    # (gated by the local instance group egid) and the local daemon talks
    # only to the local backend.
    grant_local_unlocked("test-echo")
    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo"))
    assert "REMOTE_TOKEN" not in output
    machine.log("PASS: local wrapper does not leak remote secrets")


def local_debug_subcommand():
    # ============================================================
    # DEBUG SUBCOMMAND: always-present variant, policy-gated
    # ============================================================
    # The debug variant rides on the SAME (wrapper, resource) pair as the
    # real wrapper. It is selected by the `debug` argv token
    # (`underwrap-<inst> debug <wrapper>`), and the daemon — never the caller —
    # chooses the target binary (the configured target for Real, the
    # debug tool for Debug) and only after the backend authorised the
    # variant. The single instance group egid is the only kernel gate.

    DEBUG_HEADER = "underwrap-debug-tool (UNSAFE)"

    def daemon_log():
        return machine.succeed(
            "journalctl -u underwrapd-local.service --no-pager"
            " | sed 's/\\x1b\\[[0-9;]*m//g'"
        )

    # ---- 1. Debug DENIED under Unlocked --------------------------------
    grant_local_unlocked("test-echo")  # mode = Unlocked
    rc, out = machine.execute(su("/run/wrappers/bin/underwrap-local debug test-echo 2>&1"))
    assert rc != 0, f"debug under Unlocked must FAIL, but rc={rc}; out={out!r}"
    assert "not-unsafe-debug" in out, \
        f"expected not-unsafe-debug deny under Unlocked, got: {out}"
    assert DEBUG_HEADER not in out, f"debug tool must not have run: {out}"
    machine.log("PASS: debug variant denied while permission is Unlocked")

    # The bare convenience shim must behave identically.
    out = machine.fail("su -l testuser -c 'debug-test-echo 2>&1'")
    assert "not-unsafe-debug" in out, f"debug shim deny mismatch: {out}"
    machine.log("PASS: debug-<wrapper> shim mirrors underwrap-local debug <wrapper>")

    # ---- 2. Debug ALLOWED under UnsafeDebugUntil (future) --------------
    until = int(time.time()) + 600
    admin_upsert_grant(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        "test-echo",
        "test/token",
        {"kind": "unsafe-debug-until", "until_epoch": until},
        writable=True,
    )
    out = machine.succeed(su("/run/wrappers/bin/underwrap-local debug test-echo"))
    assert DEBUG_HEADER in out, f"expected debug summary header, got: {out}"
    assert "MY_TOKEN=overwrite" in out, \
        f"debug summary must include the projected env, got: {out}"
    machine.log("PASS: debug variant allowed under UnsafeDebugUntil; projects env")

    # ---- 3. Real STAYS Real under UnsafeDebugUntil ---------------------
    # The same permission allows Debug, but a Real invocation must still
    # run the CONFIGURED target (env), not the debug tool.
    out = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo"))
    assert "MY_TOKEN=overwrite" in out, f"real wrapper must still project env: {out}"
    assert DEBUG_HEADER not in out, \
        f"real invocation must NOT land in the debug tool: {out}"
    machine.log("PASS: Real invocation runs the configured target even under UnsafeDebugUntil")

    # ---- 4. Target authority (daemon chooses target per variant) -------
    log = daemon_log()
    assert "variant=Debug" in log, f"daemon must record the Debug variant: …{log[-400:]}"
    assert "variant=Real" in log, f"daemon must record the Real variant: …{log[-400:]}"
    machine.log("PASS: daemon records variant per invocation (target authority is server-side)")

    # ---- 5. Expiry -----------------------------------------------------
    # The margin must comfortably exceed both whole-second truncation and any
    # host/VM clock skew. `policy::decide` compares `now.as_secs() > until_epoch`
    # and `until_epoch` is itself a whole second, so a deadline only ~1s in the
    # past collapses to zero effective margin: if the debug request lands in the
    # same integer second as `int(time.time()) - 1`, `decide` reads the
    # permission as not-yet-expired and (wrongly, for the test's intent) allows
    # the Debug variant. That is the non-deterministic "debug allowed when it
    # should be denied" flake investigated in ticket 1610 — a too-tight fixture
    # boundary, not a policy bug. Two minutes is far outside any plausible skew.
    past = int(time.time()) - 120
    admin_upsert_grant(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        "test-echo",
        "test/token",
        {"kind": "unsafe-debug-until", "until_epoch": past},
        writable=True,
    )
    out = machine.fail(su("/run/wrappers/bin/underwrap-local debug test-echo 2>&1"))
    assert "expired" in out, f"expected expired deny, got: {out}"
    machine.log("PASS: expired UnsafeDebugUntil denies the debug variant")

    # ---- 6. Unknown / reserved names rejected --------------------------
    out = machine.fail(su("/run/wrappers/bin/underwrap-local nonesuch 2>&1"))
    machine.log("PASS: unknown wrapper name rejected")
    # `debug` with no wrapper name following is a usage error.
    machine.fail(su("/run/wrappers/bin/underwrap-local debug 2>&1"))
    machine.log("PASS: `underwrap-local debug` with no wrapper name rejected")

    # Restore the fixture grant so later sections see test-echo Unlocked.
    grant_local_unlocked("test-echo")


def runtime_api_invariants():
    # ============================================================
    # RUNTIME API INVARIANTS
    # ============================================================
    #
    # Pins the trust-boundary properties of the new three-endpoint
    # runtime API end-to-end: lazy self-heal after a backend restart
    # (regression for the original auth-bypass risk that motivated
    # the redesign), no legacy storage-key route on the wire, no
    # PerCallApproval mode anywhere in the admin surface, and the
    # env-immutability rule applied by the backend on every finalize.
    # ============================================================

    # ----------------------------------------------------------
    # Backend restart self-heal — the daemon's BackendClient is
    # lazy and re-opens on the next wrapper call, so the wrapper
    # must keep working without restarting underwrapd-local.
    # ----------------------------------------------------------
    grant_local_unlocked("test-echo")
    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo"))
    assert "MY_TOKEN=" in output

    machine.succeed("systemctl restart underwrap-backend-local.service")
    machine.wait_for_unit("underwrap-backend-local.service")
    # Give the new listener a moment to bind before the next wrapper call.
    time.sleep(1)

    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo"))
    assert "MY_TOKEN=" in output, (
        f"wrapper must self-heal after backend restart, got: {output}"
    )
    machine.log("PASS: daemon self-heals after backend restart")

    # The in-memory `tower-sessions` store does not survive the
    # restart above, so the admin session captured at boot is gone.
    # Re-login before any admin API call after a backend restart.
    login_admin_session(ADMIN_LOCAL)

    # ----------------------------------------------------------
    # PerCallApproval variant is gone — admin wrapper-permissions/
    # upsert rejects any payload that tries to carry it.
    # ----------------------------------------------------------
    rid = _resource_id_for(ADMIN_LOCAL, "test/token")
    err = admin_api_expect_status(
        ADMIN_LOCAL,
        f"/v1/admin/stores/local/wrapper-permissions/{_seg(LOCAL_CLIENT_INSTANCE)}/test-echo/{_seg(rid)}",
        {
            "mode": {"kind": "per-call-approval"},
            "writable": False,
        },
        status=400,
        method="PUT",
    )
    err_blob = json.dumps(err).lower()
    assert (
        "unknown variant" in err_blob
        or "per-call-approval" in err_blob
        or "kind" in err_blob
    ), f"unexpected wrapper-permissions upsert error shape: {err}"
    machine.log(
        "PASS: admin wrapper-permissions upsert rejects per-call-approval payloads"
    )

    # ----------------------------------------------------------
    # The legacy per-key /v1/write route is gone — even an
    # authenticated runtime mTLS client cannot reach it. Same for
    # /v1/status, /v1/read, /v1/delete, /v1/list.
    # ----------------------------------------------------------
    runtime_cert = "/var/lib/underwrap-local/client-local.pem"
    runtime_key = "/var/lib/underwrap-local/client-local.key"

    def _runtime_curl(method, path, body=None, expect_status=None):
        """Issue an mTLS request against the local backend's runtime
        surface using underwrapd-local's own client credential, then
        return either the parsed JSON body (when expect_status is
        None or 2xx) or just the http_code (for failure cases)."""
        cmd = [
            "curl", "-sS",
            "-o", "/tmp/runtime-body",
            "-w", "%{http_code}",
            "--cacert", CA_CERT,
            "--cert", runtime_cert,
            "--key", runtime_key,
            "-X", method,
            f"https://127.0.0.1:18443{path}",
        ]
        if body is not None:
            cmd += ["-H", "Content-Type: application/json", "-d", json.dumps(body)]
        code = machine.succeed(" ".join(shlex.quote(p) for p in cmd)).strip()
        if expect_status is not None:
            assert code == str(expect_status), (
                f"runtime {method} {path} expected HTTP {expect_status}, got {code}"
            )
            return code
        if not code.startswith("2"):
            body_text = machine.succeed("cat /tmp/runtime-body")
            raise AssertionError(f"runtime {method} {path} returned {code}: {body_text}")
        body_text = machine.succeed("cat /tmp/runtime-body")
        return json.loads(body_text) if body_text.strip() else {}

    for legacy in ("/v1/status", "/v1/read", "/v1/write", "/v1/list", "/v1/delete",
                   "/v1/read-prefixes", "/v1/runtime/wrappers/register"):
        _runtime_curl("POST", legacy, body={}, expect_status=404)
    machine.log("PASS: every legacy per-key runtime route returns 404 on mTLS")

    # ----------------------------------------------------------
    # Env immutability through the full mTLS path: open returns
    # the canonical blob, finalize rejects a writeback that
    # mutates the env section even when the permission is
    # writable. This pins the redesign's "env is admin-only on
    # writeback" guarantee at the wire level inside a real VM.
    # ----------------------------------------------------------
    admin_upsert_grant(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        "test-echo",
        "test/token",
        {"kind": "unlocked"},
        writable=True,
    )

    open_resp = _runtime_curl(
        "POST",
        "/v1/wrapper-invocations",
        body={"wrapper_name": "test-echo", "resource_name": "test/token"},
    )
    assert open_resp["outcome"] == "allow", f"open should Allow, got {open_resp}"
    assert open_resp["writable"] is True, "grant flipped to writable above"
    blob_bytes = base64.b64decode(open_resp["blob_b64"])
    blob = json.loads(blob_bytes.decode())
    # Mutate the env section. JCS-canonical JSON for our blob shape
    # (ASCII keys, base64 string values, sorted u64 schema_version) is
    # byte-identical to Python's sorted-keys minified JSON, so the
    # backend's re-canonicalize check will accept the encoding and
    # then reject specifically on env-section-immutable.
    blob.setdefault("env", {})["INJECTED"] = base64.b64encode(b"x").decode()
    mutated = json.dumps(blob, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    finalize_resp = _runtime_curl(
        "POST",
        "/v1/wrapper-invocations/finalize",
        body={
            "wrapper_name": "test-echo",
            "resource_name": "test/token",
            "blob_b64": base64.b64encode(mutated).decode(),
        },
    )
    assert finalize_resp == {"outcome": "invalid-blob", "reason": "env-section-immutable"}, (
        f"env mutation must be rejected, got {finalize_resp}"
    )
    machine.log("PASS: backend rejects env mutation on direct mTLS finalize")

    # Sanity: a finalize that touches files (and leaves env alone)
    # still succeeds against the same writable permission, so the
    # rejection above is specifically about env and not about the
    # mTLS path being broken.
    blob = json.loads(blob_bytes.decode())
    blob.setdefault("files", {})["touched"] = base64.b64encode(b"y").decode()
    ok_blob = json.dumps(blob, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    finalize_resp = _runtime_curl(
        "POST",
        "/v1/wrapper-invocations/finalize",
        body={
            "wrapper_name": "test-echo",
            "resource_name": "test/token",
            "blob_b64": base64.b64encode(ok_blob).decode(),
        },
    )
    assert finalize_resp == {"outcome": "ok"}, (
        f"files-only writeback must succeed, got {finalize_resp}"
    )
    machine.log("PASS: files-only writeback through the same path succeeds")

    # Clean up the writable grant so later sections don't trip over
    # it; reset the env-only baseline by re-writing through the
    # admin API.
    admin_upsert_grant(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        "test-echo",
        "test/token",
        {"kind": "unlocked"},
        writable=False,
    )

    # ----------------------------------------------------------
    # Static check: the shipped daemon and backend binaries no
    # longer carry any of the SSH-agent approval symbols. Guards
    # against accidental reintroduction by tooling that doesn't
    # run the integration tests.
    # ----------------------------------------------------------
    forbidden = ("approval_key", "ApprovalProof", "ApprovalRequired", "PerCallApproval")
    for unit in ("underwrapd-local.service", "underwrap-backend-local.service",
                 "underwrap-backend.service"):
        pid = machine.succeed(
            f"systemctl show {unit} -p MainPID --value"
        ).strip()
        if not pid or pid == "0":
            continue
        exe = machine.succeed(f"readlink /proc/{pid}/exe").strip()
        for needle in forbidden:
            hit = machine.succeed(
                f"grep -aoF {shlex.quote(needle)} {shlex.quote(exe)} | head -n 1 || true"
            ).strip()
            assert not hit, (
                f"binary for {unit} ({exe}) still carries forbidden symbol {needle!r}"
            )
    machine.log("PASS: no SSH-approval symbols remain in shipped binaries")


def audit_and_no_secret_leaks():
    # ============================================================
    # AUDIT + NO SECRET LEAKS
    # ============================================================

    logs = machine.succeed(
        "journalctl -u underwrapd-local.service -u underwrapd-tunnel.service --no-pager"
        " | sed 's/\\x1b\\[[0-9;]*m//g'"
    )
    assert 'audit="read_granted"' in logs, "missing read_granted"

    assert "ghp_testvmtoken" not in logs, "secret leaked!"
    assert "locked_secret" not in logs, "secret leaked!"
    assert "secret_via_service" not in logs, "secret leaked!"
    assert "my_gh_token_value" not in logs, "libsecret secret leaked!"
    assert "libsecret_two_slot_value" not in logs, "two-slot secret leaked!"
    assert "iso_files_secret_val" not in logs, "isolated files secret leaked!"
    assert "iso_libsecret_val" not in logs, "isolated libsecret secret leaked!"
    machine.log("PASS: audit events present, no secrets in logs")

    machine.log("ALL TESTS PASSED")

boot_and_service_checks()
local_socket_permissions()
local_store_and_basic_ops()
local_unprivileged_wrapper_invocation()
per_user_bare_name_path()
local_verify_capabilities_dropped()
local_isolation()
local_spoofing_resistance()
local_daemon_restart_persistence()
local_pid_namespace_proc_isolation()
local_target_uid_authority()
local_target_uid_isolation()
local_target_uid_idmapped_home()
local_target_uid_runtime_dir()
local_target_uid_files_libsecret()
local_target_uid_readonly_writeback()
local_proc_isolation_env_mode()
local_ancestor_attack()
local_core_dump_protection()
local_symlink_confused_deputy_attack()
local_proc_isolation_libsecret_mode()
local_libsecret_mode()
local_libsecret_two_slot_compatibility()
local_directory_credentials()
local_atomic_writeback()
remote_service_lifecycle()
instance_isolation()
local_debug_subcommand()
local_backend_policy_control_center()
local_backend_managed_grants()
encrypted_store_in_memory_keys()
runtime_api_invariants()
audit_and_no_secret_leaks()

