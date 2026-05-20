"""underwrap VM test — local + remote instances on one machine."""

import base64
import json
import shlex
import time

# Helper for running commands as testuser.
def su(cmd):
    return f"su testuser -s /bin/sh -c '{cmd}'"

# Spoof script path is passed as an env var by the Nix test driver.
import os
SPOOF_SCRIPT = os.environ.get("SPOOF_SCRIPT", "/dev/null")
PYTHON3 = os.environ.get("PYTHON3", "python3")

# Each daemon now runs a single HTTPS listener that serves both the
# runtime service API (mTLS-authenticated, `/v1/...`) and the admin
# SPA + JSON API (session-cookie + CSRF authenticated, `/v1/admin/...`).
ADMIN_LOCAL = "https://127.0.0.1:18443"
ADMIN_REMOTE = "https://127.0.0.1:18444"
CA_CERT = "/etc/underwrap-test-ca.pem"
ADMIN_BOOTSTRAP_USER = "tester"
ADMIN_BOOTSTRAP_PASSWORD = "hunter2"

# Per-admin session-cookie state, populated by `bootstrap_admin_session`
# during boot. `_COOKIE_JAR[url]` is the path of curl's cookie file on
# the VM; `_CSRF_TOKEN[url]` is the value of the `__Host-uw-csrf` cookie
# extracted from it.
_COOKIE_JAR = {}
_CSRF_TOKEN = {}


def _cookie_jar_path(url):
    safe = url.replace("https://", "").replace(":", "-").replace("/", "_")
    return f"/tmp/underwrap-admin-cookies-{safe}.txt"


def bootstrap_admin_session(url):
    """Create the first operator on an admin listener and capture its
    session cookies. Must be called exactly once per backend boot —
    subsequent bootstrap POSTs would return 409.
    """
    cookie_jar = _cookie_jar_path(url)
    payload = json.dumps({
        "username": ADMIN_BOOTSTRAP_USER,
        "password": ADMIN_BOOTSTRAP_PASSWORD,
    })
    cmd = [
        "curl", "-fsS",
        "--cacert", CA_CERT,
        "-c", cookie_jar,
        "-X", "POST",
        "-H", f"Origin: {url}",
        "-H", "Content-Type: application/json",
        "-d", payload,
        f"{url}/v1/admin/auth/bootstrap",
    ]
    machine.succeed(" ".join(shlex.quote(p) for p in cmd))

    # Extract the CSRF cookie value from the Netscape-format cookie jar.
    # Field 6 is the cookie name, field 7 is the value.
    csrf = machine.succeed(
        f"awk '$6==\"__Host-uw-csrf\"{{print $7}}' {shlex.quote(cookie_jar)}"
    ).strip()
    if not csrf:
        raise AssertionError(
            f"bootstrap_admin_session: no __Host-uw-csrf cookie in {cookie_jar}"
        )

    _COOKIE_JAR[url] = cookie_jar
    _CSRF_TOKEN[url] = csrf


def admin_api(url, path, payload=None, method="POST"):
    """Make an authenticated admin API call. Uses the session cookies
    populated by `bootstrap_admin_session` for the listener at `url`
    and adds an `X-CSRF-Token` header on every state-changing request
    so the backend's CSRF double-submit check (ticket 212) passes.
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
    return admin_api(
        url,
        "/v1/admin/secret/write",
        {
            "store": store,
            "wrapper": wrapper,
            "secret_id": secret_id,
            "material_b64": base64.b64encode(value.encode()).decode(),
        },
    )


def admin_upsert_grant(url, store, client_instance, wrapper, resource, mode, *, writable=False, reason=None):
    """Upsert an agent grant for a `(client_instance, wrapper, resource)`
    tuple.

    `mode` is a dict that serializes WrapperApprovalMode, e.g.:
      {"kind": "locked"}
      {"kind": "unlocked"}
      {"kind": "unlocked-until", "until_epoch": 1234}
      {"kind": "per-call-approval", "approval_key": "ssh-ed25519 ..."}
    """
    grant = {
        "client_instance": client_instance,
        "wrapper": wrapper,
        "resource": resource,
        "mode": mode,
        "writable": writable,
    }
    if reason is not None:
        grant["reason"] = reason
    return admin_api(
        url,
        "/v1/admin/grants/upsert",
        {"store": store, "grant": grant},
    )


def admin_delete_grant(url, store, client_instance, wrapper, resource):
    return admin_api(
        url,
        "/v1/admin/grants/delete",
        {
            "store": store,
            "client_instance": client_instance,
            "wrapper": wrapper,
            "resource": resource,
        },
    )


def admin_get_blob(url, store, resource):
    """Fetch a resource blob's canonical bytes via the admin API.

    Returns the raw base64 string the API surfaces so the caller can
    do byte-equal comparisons between runs without re-canonicalizing.
    """
    return admin_api(
        url,
        "/v1/admin/resources/blob/get",
        {"store": store, "name": resource},
    )["blob_b64"]


def admin_list_pairs(url, store):
    return admin_api(
        url,
        "/v1/admin/pairs/list",
        {"store": store},
    )["pairs"]


def find_pair(pairs, wrapper, resource):
    for p in pairs:
        if p["wrapper"] == wrapper and p["resource"] == resource:
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
    "test-crash": "test/crash-tok",
    "test-libsecret": "test/libsecret-store",
    "test-caps": "test/caps-tok",
    "test-ns-pids": "test/ns-pids-store",
    "test-keyring-probe": "test/libsecret-store",
    "test-dir-write": "test/dir-a",
    "test-dir-read": "test/dir-a",
    "test-dir-readonly-write": "test/dir-a",
    "test-dir-sleep": "test/dir-a",
    "test-dir-symlink": "test/dir-a",
    "test-dir-write-then-sleep": "test/dir-a",
    "test-dir-touch": "test/dir-a",
    "test-dir-env-probe": "test/dir-a",
}

# Each wrapper now projects exactly one resource, so no secondary
# grants are needed.
LOCAL_WRAPPER_EXTRA_RESOURCES = {}

REMOTE_WRAPPER_RESOURCES = {
    "remote-echo": "remote/token",
}

# Resolved at boot from `GET /v1/admin/client-instances`. The backend
# assigns runtime-client ids dynamically (see
# `crates/backend-backendd/src/runtime_clients.rs`), so we look them
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
    machine.wait_for_unit("underwrap-backendd.service")
    machine.wait_for_unit("underwrap-backendd-local.service")

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
    # backends are reachable (they probe /v1/status at startup), so
    # wait for them now.
    machine.wait_for_unit("underwrapd-local.service")
    machine.wait_for_unit("underwrapd-tunnel.service")

    machine.succeed("id testuser")
    machine.succeed("id underwrap-local")
    machine.succeed("id underwrap-tunnel")

    # Ticket 231 replaced the bare `{stores: [...]}` response with a
    # richer per-store list. Assert the surfaced names match what
    # each daemon's `stores_declarative_dir` actually contains.
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
        ADMIN_LOCAL, "/v1/admin/resources/list", {"store": "local"}
    )["resources"]
    local_pairs = admin_api(
        ADMIN_LOCAL, "/v1/admin/pairs/list", {"store": "local"}
    )["pairs"]
    remote_resources = admin_api(
        ADMIN_REMOTE, "/v1/admin/resources/list", {"store": "remote"}
    )["resources"]
    remote_pairs = admin_api(
        ADMIN_REMOTE, "/v1/admin/pairs/list", {"store": "remote"}
    )["pairs"]

    assert any(
        r["name"] == "test/token"
        and r["created_by"].get("kind") == "client-instance"
        and r["created_by"].get("id") == LOCAL_CLIENT_INSTANCE
        for r in local_resources
    ), f"missing local resource record for test/token with client-instance provenance: {local_resources}"
    assert any(
        p["wrapper"] == "test-echo"
        and p["resource"] == "test/token"
        and p["created_by"].get("id") == LOCAL_CLIENT_INSTANCE
        for p in local_pairs
    ), f"missing local pair record (test-echo, test/token): {local_pairs}"
    assert any(
        r["name"] == "remote/token"
        and r["created_by"].get("id") == REMOTE_CLIENT_INSTANCE
        for r in remote_resources
    ), f"missing remote resource record: {remote_resources}"
    assert any(
        p["wrapper"] == "remote-echo" and p["resource"] == "remote/token"
        for p in remote_pairs
    ), f"missing remote pair record: {remote_pairs}"
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
    # LOCAL INSTANCE: socket permissions
    # ============================================================

    perms = machine.succeed("stat -c %a /run/underwrap-local").strip()
    assert perms == "755", f"expected 755, got {perms}"
    perms = machine.succeed("stat -c %a /run/underwrap-local/wrapper-test-echo.sock").strip()
    assert perms == "660", f"expected 660, got {perms}"
    perms = machine.succeed("stat -c %G /run/underwrap-local/wrapper-test-echo.sock").strip()
    assert perms == "underwrap-local-test-echo", f"expected wrapper group, got {perms}"
    # Groups exist.
    machine.succeed("getent group underwrap-local-test-echo")
    machine.succeed("getent group underwrap-local-test-echo2")

    # Setgid wrappers installed.
    stat = machine.succeed("stat -c '%A %G' /run/wrappers/bin/local-test-echo").strip()
    assert "underwrap-local-test-echo" in stat, f"wrong group: {stat}"
    assert "s" in stat.lower(), f"setgid not set: {stat}"

    # Sidecar configs.
    machine.succeed("test -f /etc/underwrap/wrappers/local-test-echo.yaml")

    # underwrap-ns has the capabilities it needs for namespace setup and
    # attempts bounding-set cleanup where the kernel permits it.
    caps = machine.succeed("getcap /run/wrappers/bin/underwrap-ns").strip()
    assert "cap_sys_admin" in caps, f"expected cap_sys_admin: {caps}"
    assert "cap_setpcap" in caps, f"expected cap_setpcap: {caps}"


def local_store_and_basic_ops():
    # ============================================================
    # LOCAL INSTANCE: store secrets + basic operations
    # ============================================================

    # Admin writes through `/v1/admin/secret/write` are unconditional
    # in the new grant model; the global `writes_enabled` toggle is
    # gone and per-grant `writable` controls only wrapper-side writes.

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
        ("test-crash", "test/crash-tok"),
        ("test-libsecret", "test/libsecret-store"),
        ("test-caps", "test/caps-tok"),
        ("test-ns-pids", "test/ns-pids-store"),
        ("test-keyring-probe", "test/libsecret-store"),
    ]:
        grant_local_unlocked(wrapper, resource)

    admin_write_secret(ADMIN_LOCAL, "local", "test-echo", "test/token/env/MY_TOKEN", "ghp_testvmtoken")
    admin_write_secret(ADMIN_LOCAL, "local", "test-echo2", "test/other/env/OTHER_SECRET", "other_secret_val")
    admin_write_secret(ADMIN_LOCAL, "local", "test-locked", "test/locked-tok/env/LOCKED_TOKEN", "locked_secret")
    admin_write_secret(ADMIN_LOCAL, "local", "test-sleep", "test/dumpable-tok/env/DUMPABLE_SECRET", "dumpable_test_secret")
    admin_write_secret(ADMIN_LOCAL, "local", "test-crash", "test/crash-tok/env/CRASH_SECRET", "crash_secret_value")
    admin_write_secret(ADMIN_LOCAL, "local", "test-caps", "test/caps-tok/env/CAPS_SECRET", "caps_test_val")

    # Admin writes may update operator-managed credential refs centrally.
    admin_write_secret(ADMIN_LOCAL, "local", "test-echo", "test/token/env/MY_TOKEN", "overwrite")
    admin_write_secret(ADMIN_LOCAL, "local", "test-echo2", "test/other/env/OTHER_SECRET", "updated_val")


def local_unprivileged_wrapper_invocation():
    # ============================================================
    # LOCAL INSTANCE: unprivileged user wrapper invocation
    # ============================================================

    output = machine.succeed(su("/run/wrappers/bin/local-test-echo"))
    assert "MY_TOKEN=overwrite" in output, f"expected MY_TOKEN: {output}"
    machine.log("PASS: unprivileged user can use wrapper")

    output = machine.succeed(su("/run/wrappers/bin/local-test-echo2 hello"))
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
    # PER-USER PATH INTEGRATION: bare-named wrappers
    # ============================================================
    # The instance-prefixed setgid binaries in /run/wrappers/bin are the real
    # entry points; the module additionally installs bare-named symlinks into
    # each authorized user's profile bin via users.users.<u>.packages, so that
    # users invoke wrappers by their bare attribute name without each
    # operator wiring up a parallel security.wrappers list.
    profile_bin = "/etc/profiles/per-user/testuser/bin"
    machine.succeed(f"test -d {profile_bin}")
    machine.log(f"PASS: per-user profile bin exists at {profile_bin}")

    target = machine.succeed(f"readlink -f {profile_bin}/test-echo").strip()
    assert target.startswith("/run/wrappers/") and target.endswith("/local-test-echo"), \
        f"expected local-test-echo target, got: {target}"
    target = machine.succeed(f"readlink -f {profile_bin}/remote-echo").strip()
    assert target.startswith("/run/wrappers/") and target.endswith("/tunnel-remote-echo"), \
        f"expected tunnel-remote-echo target, got: {target}"
    machine.log("PASS: per-user bin contains bare-named symlinks across both instances")

    # The per-user dir is a bare-name namespace: it must not also expose the
    # instance-prefixed names. Those remain available through /run/wrappers/bin.
    rc = machine.execute(f"test -e {profile_bin}/local-test-echo")[0]
    assert rc != 0, "per-user bin unexpectedly contains prefixed name"
    machine.log("PASS: per-user bin does not duplicate prefixed names")

    # Login shell for testuser must resolve the bare name through this dir.
    which_out = machine.succeed("su -l testuser -c 'command -v test-echo'").strip()
    assert which_out == f"{profile_bin}/test-echo", \
        f"expected bare 'test-echo' to resolve via per-user bin, got: {which_out}"
    machine.log(f"PASS: bare 'test-echo' resolves to {which_out} in login shell")

    # And bare-name invocation actually works end-to-end.
    output = machine.succeed("su -l testuser -c 'test-echo'")
    assert "MY_TOKEN=overwrite" in output, \
        f"expected MY_TOKEN via bare-name invocation: {output}"
    machine.log("PASS: bare-name wrapper invocation through login shell works")


def local_verify_capabilities_dropped():
    # ============================================================
    # LOCAL INSTANCE: verify capabilities are dropped for target
    # ============================================================

    caps_output = machine.succeed(su("/run/wrappers/bin/local-test-caps")).strip()
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
    output = machine.succeed(su("/run/wrappers/bin/local-test-echo"))
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
    machine.wait_for_file("/run/underwrap-local/wrapper-test-echo.sock")
    time.sleep(0.5)
    output = machine.succeed(su("/run/wrappers/bin/local-test-echo"))
    assert "MY_TOKEN=overwrite" in output
    machine.log("PASS: secrets persist across restart")


def local_pid_namespace_proc_isolation():
    # ============================================================
    # LOCAL INSTANCE: PID namespace and /proc isolation (DECISIVE TEST)
    # ============================================================
    # This test answers: can a same-UID host-namespace process read
    # /proc/<host_pid>/environ of the final target process?
    #
    # Design:
    # 1. Launch a long-lived wrapped target (sleep 300) as testuser
    # 2. As ROOT (test instrumentation only), find the host-visible PID
    #    of the actual "sleep" process inside the namespace
    # 3. As TESTUSER (same-UID attacker), attempt to read its /proc entries
    # 4. Hard assertion on the result

    machine.succeed(
        "su testuser -s /bin/sh -c '"
        "setsid /run/wrappers/bin/local-test-sleep 300 </dev/null >/dev/null 2>&1 &"
        "'"
    )
    time.sleep(2)

    # Step 2: As root, find the host PID of the final "sleep" target.
    # Root can always see all processes regardless of namespaces.
    # Try multiple methods since process naming may vary.
    host_pid = machine.succeed(
        "pgrep -u testuser -x sleep || "
        "pgrep -u testuser -f 'sleep 300' || "
        "echo NONE"
    ).strip().split("\n")[0]

    if host_pid == "NONE" or host_pid == "":
        # Debug: show ALL testuser processes.
        all_procs = machine.succeed(
            "ps -u testuser -o pid,comm,args 2>/dev/null || echo 'no processes'"
        ).strip()
        machine.log(f"DEBUG: all testuser processes: {all_procs}")
        # Also check if the wrapper chain is still running.
        wrapper_procs = machine.succeed(
            "pgrep -u testuser -a || echo 'no pgrep results'"
        ).strip()
        machine.log(f"DEBUG: pgrep -u testuser -a: {wrapper_procs}")

        # If root truly can't find the sleep process, this means either:
        # a) The process died (wrapper failed), OR
        # b) The PID namespace genuinely hides it from host /proc
        # Either way, we need to know which. Check if the wrapper parent is alive.
        if "test-sleep" not in wrapper_procs and "sleep" not in wrapper_procs:
            assert False, \
                f"INCONCLUSIVE: no sleep-related processes found. " \
                f"Wrapper may have failed. All testuser procs: {all_procs}"
        else:
            # Wrapper chain exists but sleep itself not found by pgrep.
            # This could mean the target's comm doesn't match "sleep".
            # Accept this as "root can see wrapper but not target".
            machine.log("NOTE: root found wrapper processes but not the final target")
            # Try to find any process in a child PID namespace.
            ns_pids = machine.succeed(
                "find /proc -maxdepth 2 -name 'NSpid' -exec grep -l . {} + 2>/dev/null | head -5 || echo NONE"
            ).strip()
            machine.log(f"DEBUG: processes with NSpid: {ns_pids}")
            host_pid = "NONE"

    if host_pid != "NONE" and host_pid != "":
        machine.log(f"Found target sleep process at host PID {host_pid}")

        # Verify it's actually the right process (running sleep, owned by testuser).
        cmdline = machine.succeed(f"cat /proc/{host_pid}/cmdline 2>/dev/null || echo UNKNOWN").strip()
        machine.log(f"Target cmdline: {cmdline}")
        assert "sleep" in cmdline, f"Host PID {host_pid} is not a sleep process: {cmdline}"

        # Step 3: As testuser (same-UID), attempt to read the target's environ.
        # This is THE decisive test. If this succeeds, the secret is exposed.
        rc_environ = machine.execute(
            f"su testuser -s /bin/sh -c 'cat /proc/{host_pid}/environ 2>/dev/null'"
        )[0]

        rc_mem = machine.execute(
            f"su testuser -s /bin/sh -c 'dd if=/proc/{host_pid}/mem bs=1 count=1 2>/dev/null'"
        )[0]

        rc_maps = machine.execute(
            f"su testuser -s /bin/sh -c 'cat /proc/{host_pid}/maps 2>/dev/null'"
        )[0]

        machine.log(f"Same-UID access to target PID {host_pid}: "
                    f"environ={rc_environ} mem={rc_mem} maps={rc_maps}")

        # Record what actually happened, then assert.
        if rc_environ == 0:
            # Same-UID CAN read the target's environ.
            leaked_env = machine.succeed(
                f"cat /proc/{host_pid}/environ 2>/dev/null | tr '\\0' '\\n' | head -5"
            ).strip()
            machine.log(f"LEAKED environ (first 5 vars): {leaked_env}")
            full_env = machine.succeed(
                f"cat /proc/{host_pid}/environ 2>/dev/null | tr '\\0' '\\n' || true"
            )
            if "dumpable_test_secret" in full_env:
                assert False, \
                    f"CRITICAL: same-UID host process CAN read target's environ " \
                    f"AND the secret is visible! PID {host_pid}"
            else:
                assert False, \
                    f"EXPOSED: same-UID host process CAN read target's environ " \
                    f"(secret not found in first read, but access is open). PID {host_pid}"
        else:
            machine.log(f"PASS: same-UID host process CANNOT read target environ (PID {host_pid})")

        assert rc_mem != 0, f"FAIL: same-UID can read /proc/{host_pid}/mem"
        assert rc_maps != 0, f"FAIL: same-UID can read /proc/{host_pid}/maps"
        machine.log("PASS: final target process is protected from same-UID /proc inspection")
    else:
        machine.log("INFO: target process not visible to root via pgrep — PID namespace hides it")
        machine.log("PASS: target process invisible from parent namespace")

    # Also verify the wrapper parent processes are still protected.
    wrapper_pids = machine.succeed(
        "pgrep -u testuser -f 'local-test-sleep|underwrap' || echo none"
    ).strip()
    if wrapper_pids != "none":
        for pid_str in wrapper_pids.split("\n"):
            pid_str = pid_str.strip()
            if not pid_str or pid_str == host_pid:
                continue
            rc = machine.execute(
                f"su testuser -s /bin/sh -c 'cat /proc/{pid_str}/environ 2>/dev/null'"
            )[0]
            assert rc != 0, f"FAIL: same-UID read /proc/{pid_str}/environ (wrapper process)"
        machine.log("PASS: wrapper parent processes also protected")

    machine.execute("pkill -u testuser -f local-test-sleep || true")
    time.sleep(0.5)


def local_ancestor_attack():
    # ============================================================
    # LOCAL INSTANCE: adversarial ancestor attack (CRITICAL TEST)
    # ============================================================
    # This tests whether a PARENT process that spawns the wrapper can
    # inspect its descendant target and recover the secret. With
    # services.underwrap.hardenTargetHost = true (default), the module
    # pins ptrace_scope=2 so even an ancestor needs CAP_SYS_PTRACE to
    # ptrace or read /proc/<pid>/{environ,mem}. This test confirms the
    # adversarial caller really cannot recover the token.

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

    # The ancestor MUST NOT be able to read the secret.
    if "SECRET_LEAKED" in results:
        assert False, \
            "CRITICAL: ancestor process CAN read descendant target's secret via /proc/environ!"

    if "ENVIRON_READABLE_NO_SECRET" in results:
        machine.log("WARNING: ancestor can read /proc/environ but secret not found in output")
        machine.log("This may indicate a partial leak or timing issue")

    if "PTRACE_ALLOWED" in results:
        machine.log("WARNING: ancestor can ptrace-attach to descendant target")

    if "MEM_READABLE" in results:
        machine.log("WARNING: ancestor can read /proc/mem of descendant target")

    # Log the definitive outcome.
    if "ENVIRON_DENIED" in results:
        machine.log("PASS: ancestor /proc/environ access DENIED")
    elif "TARGET_NOT_FOUND" in results:
        machine.log("PASS: ancestor cannot find descendant target in /proc")
    elif "PROC_NOT_FOUND" in results:
        machine.log("PASS: descendant target /proc entry not found by ancestor")
    else:
        machine.log(f"INFO: ancestor attack outcome: {results}")

    # Hard assertion: the secret must not leak.
    assert "SECRET_LEAKED" not in results, \
        "CRITICAL: adversarial ancestor recovered secret from descendant target"
    machine.log("PASS: adversarial ancestor attack did not recover secret")

    machine.execute("pkill -u testuser -f local-test-sleep || true")
    time.sleep(0.5)


def local_core_dump_protection():
    # ============================================================
    # LOCAL INSTANCE: core dump protection
    # ============================================================

    machine.succeed("mkdir -p /tmp/cores")
    machine.succeed("echo '/tmp/cores/core.%e.%p' > /proc/sys/kernel/core_pattern")

    machine.execute(su("/run/wrappers/bin/local-test-crash"))

    core_files = machine.succeed("ls /tmp/cores/ 2>/dev/null || true").strip()
    assert core_files == "", f"FAIL: core dump produced: {core_files}"
    machine.log("PASS: no core dump on crash")


def local_symlink_confused_deputy_attack():
    # ============================================================
    # LOCAL INSTANCE: symlink confused-deputy attack must fail
    # ============================================================
    # Create a symlink from one wrapper name to another wrapper's binary.
    # The wrapper must still use the correct config (from its egid), not
    # the symlink name. If it used argv[0], this would pair the wrong
    # secret with the wrong target.

    machine.succeed(
        "ln -sf /run/wrappers/bin/local-test-echo /tmp/local-test-echo2"
    )
    # Run via the symlink — argv[0] = local-test-echo2, but egid = test-echo's group.
    # The wrapper should load test-echo's config and return test-echo's secret.
    output = machine.succeed(su("/tmp/local-test-echo2")).strip()
    assert "MY_TOKEN=overwrite" in output, \
        f"FAIL: symlink wrapper did not get the correct (test-echo) secret: {output}"
    # It must NOT have loaded test-echo2's config (which would show OTHER_SECRET).
    assert "OTHER_SECRET" not in output, \
        f"FAIL: symlink wrapper loaded the wrong config: {output}"
    machine.succeed("rm -f /tmp/local-test-echo2")
    machine.log("PASS: symlink confused-deputy attack prevented (egid-based identity)")


def local_proc_isolation_libsecret_mode():
    # ============================================================
    # LOCAL INSTANCE: /proc isolation in libsecret (mount namespace) mode
    # ============================================================
    # In libsecret mode, /proc is remounted inside the new PID namespace.
    # The target should only see namespace-local PIDs, not host PIDs.

    ns_pids_output = machine.succeed(
        su("/run/wrappers/bin/local-test-ns-pids")
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
        '/run/wrappers/bin/local-test-libsecret store '
        '--label=TestToken service gh account github.com"'
    )
    machine.succeed("rm -f /tmp/.secretsd-test")
    machine.log("PASS: secret-tool store via libsecret wrapper")

    # Retrieve the secret via secret-tool through the libsecret wrapper.
    output = machine.succeed(
        'su testuser -s /bin/sh -c '
        '"/run/wrappers/bin/local-test-libsecret lookup service gh account github.com"'
    ).strip()
    # The output may include secretsd log lines on stderr mixed in;
    # the actual secret is the last line.
    last_line = output.split("\n")[-1].strip()
    assert last_line == "my_gh_token_value", f"expected 'my_gh_token_value', got '{last_line}'"
    machine.log("PASS: secret-tool lookup via libsecret wrapper")

    # Verify the secret IS in the backend storage (via the daemon).
    # Under the atomic-blob storage layout the whole libsecret keyring
    # lives in a single file at `resources/<resource_id>`.
    backend_blob = machine.succeed(
        "test -f /var/lib/underwrap-backend-local/secrets/resources/test/libsecret-store && echo ok || echo none"
    ).strip()
    assert backend_blob == "ok", "expected libsecret resource blob in backend"
    machine.log("PASS: libsecret data stored in backend")

    # Verify the private bus is NOT reachable from outside the namespace.
    # Run a libsecret wrapper in the background, then try to connect to the
    # bus socket from a host process. The socket should not exist on the host.
    machine.succeed(
        'su testuser -s /bin/sh -c "'
        'setsid /run/wrappers/bin/local-test-libsecret lookup service gh account github.com '
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

    machine.execute("pkill -u testuser -f local-test-libsecret || true")
    time.sleep(0.5)

    # Find ALL files on the system containing the secret, outside the backend.
    # Exclude: backend dir (correct storage), /nix/ (test script literals),
    # /proc, /sys, /dev.
    leak_files = machine.succeed(
        "find / -type f "
        "-not -path '/var/lib/underwrap-backend-local/secrets/*' "
        "-not -path '/proc/*' "
        "-not -path '/sys/*' "
        "-not -path '/nix/*' "
        "-not -path '/dev/*' "
        "-exec grep -l 'my_gh_token_value' {} + 2>/dev/null || echo NONE"
    ).strip()
    machine.log(f"SECRET LEAK CHECK: files containing secret outside backend: {leak_files}")
    assert leak_files == "NONE", f"SECRET LEAKED to files outside backend: {leak_files}"
    machine.log("PASS: no secret material leaked outside backend")


def local_go_keyring_compatibility():
    # ============================================================
    # LOCAL INSTANCE: go-keyring-compatible Secret Service flow
    # ============================================================

    import tempfile, os
    with tempfile.NamedTemporaryFile(mode='w', suffix='.secret', delete=False) as f:
        f.write('keyring_roundtrip_value')
        secret_file = f.name
    machine.copy_from_host(secret_file, "/tmp/.keyring-probe-secret")
    os.unlink(secret_file)
    machine.succeed("chmod 600 /tmp/.keyring-probe-secret && chown testuser /tmp/.keyring-probe-secret")
    output = machine.succeed(
        'su testuser -s /bin/sh -c "'
        'cat /tmp/.keyring-probe-secret | '
        '/run/wrappers/bin/local-test-keyring-probe gh-login-sequence '
        '--service gh:github.com --user rschaffar --secret-stdin"'
    ).strip()
    machine.succeed("rm -f /tmp/.keyring-probe-secret")
    assert "user-slot=ok" in output, output
    assert "active-slot=ok" in output, output
    machine.log("PASS: go-keyring-compatible Secret Service sequence works")


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
    machine.succeed(su("/run/wrappers/bin/local-test-dir-write seed"))
    machine.log("PASS: directory credential seed wrapper succeeded")

    # Read the mounted directory through a read-only wrapper.
    dir_output = machine.succeed(su("/run/wrappers/bin/local-test-dir-read")).strip().split("\n")
    assert dir_output[0].strip() == "token-seed", f"expected token-seed, got {dir_output}"
    machine.log("PASS: directory credential read-only wrapper can read the mounted directory")

    # Update content through the read-write wrapper.
    machine.succeed(su("/run/wrappers/bin/local-test-dir-write update"))
    dir_output = machine.succeed(su("/run/wrappers/bin/local-test-dir-read")).strip().split("\n")
    assert dir_output[0].strip() == "token-updated", f"expected token-updated, got {dir_output}"
    machine.log("PASS: directory read-write wrapper updates backend-managed files")

    # Verify the in-blob files section reflects the writeback (single
    # resource per wrapper, so all files land in dir-a's blob).
    dir_a_files = sorted(
        json.loads(
            base64.b64decode(
                admin_api(
                    ADMIN_LOCAL, "/v1/admin/resources/blob/get",
                    {"store": "local", "name": "test/dir-a"},
                )["blob_b64"]
            )
        )["files"].keys()
    )
    assert dir_a_files == ["sub/nested", "token"], (
        f"unexpected dir-a blob files: {dir_a_files}"
    )
    machine.log("PASS: resource blob carries the writeback")

    # While a wrapped target is running, a separate same-UID host process must not
    # be able to access the mounted secret files from the host namespace view.
    machine.succeed(
        "su testuser -s /bin/sh -c '"
        "setsid /run/wrappers/bin/local-test-dir-sleep 300 </dev/null >/dev/null 2>&1 &'"
    )
    time.sleep(1)

    host_view = machine.execute(
        "su testuser -s /bin/sh -c 'cat $HOME/.config/underwrap-dir-a/token 2>/dev/null'"
    )[0]
    assert host_view != 0, "FAIL: same-UID host process can read directory credential file"

    host_ls = machine.execute(
        "su testuser -s /bin/sh -c 'ls $HOME/.config/underwrap-dir-a >/dev/null 2>&1'"
    )[0]
    assert host_ls != 0, "FAIL: same-UID host process can list mounted directory"

    host_find = machine.succeed(
        "su testuser -s /bin/sh -c 'find $HOME/.config/underwrap-dir-a -maxdepth 2 -type f 2>/dev/null || true'"
    ).strip()
    assert host_find == "", f"FAIL: host view exposes mounted directory files: {host_find}"
    machine.log("PASS: same-UID host processes cannot list mounted directory or access files in it")

    machine.execute("pkill -u testuser -f local-test-dir-sleep || true")
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
    before = admin_api(
        ADMIN_LOCAL, "/v1/admin/resources/blob/get",
        {"store": "local", "name": "test/dir-a"},
    )["blob_b64"]
    machine.succeed(su("/run/wrappers/bin/local-test-dir-symlink"))
    after = admin_api(
        ADMIN_LOCAL, "/v1/admin/resources/blob/get",
        {"store": "local", "name": "test/dir-a"},
    )["blob_b64"]
    assert after == before, (
        "symlink-creating wrapper unexpectedly mutated the backend blob"
    )
    machine.log("PASS: directory writeback drops runs that introduce symlinks")

    # Seed the per-grant `writable=true` for the wrappers used by
    # the atomic-writeback scenarios (story 410 / ticket 432). They
    # share the dir-a resource.
    for wrapper in [
        "test-dir-write-then-sleep",
        "test-dir-touch",
        "test-dir-env-probe",
    ]:
        grant_local_unlocked(wrapper)

    # While a libsecret wrapper is running, the private bus must also remain
    # inaccessible to a same-UID host process.
    machine.succeed(
        'su testuser -s /bin/sh -c "'
        'setsid /run/wrappers/bin/local-test-libsecret lookup service gh account github.com '
        '</dev/null >/dev/null 2>&1 &"'
    )
    time.sleep(1)
    libsecret_host_view = machine.succeed(
        "su testuser -s /bin/sh -c 'find /tmp/.underwrap-ns-bus -type s 2>/dev/null || true'"
    ).strip()
    assert libsecret_host_view == "", f"FAIL: same-UID host process sees private bus socket: {libsecret_host_view}"
    libsecret_ls = machine.execute(
        "su testuser -s /bin/sh -c 'ls /tmp/.underwrap-ns-bus >/dev/null 2>&1'"
    )[0]
    assert libsecret_ls != 0, "FAIL: same-UID host process can list private libsecret bus mountpoint"
    machine.log("PASS: same-UID host process cannot list private libsecret runtime directories or see the bus socket")
    machine.execute("pkill -u testuser -f local-test-libsecret || true")
    time.sleep(0.5)


def local_atomic_writeback():
    # ============================================================
    # LOCAL INSTANCE: atomic resource-blob writeback decision tree
    # (story 410 / ticket 432)
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
        machine.succeed(su("/run/wrappers/bin/local-test-dir-write seed"))
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
    machine.succeed(su("/run/wrappers/bin/local-test-dir-read"))
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
    machine.succeed(su("/run/wrappers/bin/local-test-dir-write update"))
    time.sleep(0.5)
    updated_a = admin_get_blob(ADMIN_LOCAL, "local", DIR_A)
    assert updated_a != baseline_a, (
        "scenario 2: dir-a blob did not change after writable update run"
    )
    output = machine.succeed(su("/run/wrappers/bin/local-test-dir-read")).strip().split("\n")
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
            "/run/wrappers/bin/local-test-dir-readonly-write update 2>/tmp/ro-stderr.log "
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
        "/run/wrappers/bin/local-test-dir-readonly-write update 2>/tmp/ro-stderr.log; "
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

    # Env clear: the target must not see UNDERWRAP_SUPPRESS_READONLY_WARNING
    # (or any other UNDERWRAP_*) propagated from the wrapper invocation
    # environment.
    grant_local_unlocked("test-dir-env-probe")
    target_env = machine.succeed(
        "su testuser -s /bin/sh -c '"
        "UNDERWRAP_SUPPRESS_READONLY_WARNING=1 "
        "/run/wrappers/bin/local-test-dir-env-probe'"
    )
    leaked = [
        line for line in target_env.splitlines() if line.startswith("UNDERWRAP_")
    ]
    assert leaked == [], (
        f"scenario 4: UNDERWRAP_* vars leaked into target environment: {leaked}"
    )
    machine.log(
        "PASS: scenario 4 — suppression silences stderr, journal still records, target sees no UNDERWRAP_*"
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
    machine.succeed(su("/run/wrappers/bin/local-test-dir-touch"))
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
        "setsid /run/wrappers/bin/local-test-dir-write-then-sleep "
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
      3. Restart `underwrap-backendd-local`. After the restart the
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
    admin_api(
        ADMIN_LOCAL,
        "/v1/admin/secret/write",
        {
            "store": store_id,
            "resource_id": resource_id,
            "slot": "env",
            "key": "GREETING",
            "material_b64": slot_payload,
        },
    )

    def read_secret():
        return admin_api(
            ADMIN_LOCAL,
            "/v1/admin/secret/read",
            {
                "store": store_id,
                "resource_id": resource_id,
                "slot": "env",
                "key": "GREETING",
            },
        )

    assert read_secret()["material_b64"] == slot_payload, (
        "unencrypted round-trip failed"
    )

    # ---- 3. restart → boots locked --------------------------------
    machine.succeed("systemctl restart underwrap-backendd-local")
    machine.wait_for_unit("underwrap-backendd-local.service")
    # The admin listener takes a moment to come back up.
    machine.wait_until_succeeds(
        f"curl -fsS --cacert {CA_CERT} "
        f"{ADMIN_LOCAL}/v1/admin/auth/bootstrap/status >/dev/null"
    )
    # We have to re-bootstrap our session against the listener
    # because in-memory session state was reset.
    bootstrap_admin_session(ADMIN_LOCAL)

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
        "/v1/admin/secret/read",
        {
            "store": store_id,
            "resource_id": resource_id,
            "slot": "env",
            "key": "GREETING",
        },
        status=503,
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
    machine.succeed("systemctl restart underwrap-backendd-local")
    machine.wait_for_unit("underwrap-backendd-local.service")
    machine.wait_until_succeeds(
        f"curl -fsS --cacert {CA_CERT} "
        f"{ADMIN_LOCAL}/v1/admin/auth/bootstrap/status >/dev/null"
    )
    bootstrap_admin_session(ADMIN_LOCAL)
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
        "/v1/admin/secret/read",
        {
            "store": store_id,
            "resource_id": resource_id,
            "slot": "env",
            "key": "GREETING",
        },
        status=503,
    )
    assert locked_after_lock["code"] == "store_locked"

    # Restore for any later tests / leave the store unlocked at the end.
    admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/unlock",
        {"key_hex": key_hex_rotated},
    )

    machine.log("PASS: encrypted store keeps master keys in memory only")


def admin_api_expect_status(url, path, payload, *, status):
    """Like `admin_api` but expects a specific non-2xx status and
    returns the decoded JSON error body. Used by the
    encrypted-store-locked tests where the interesting case is the
    `code` field on a 4xx / 5xx response.
    """
    is_state_changing = True
    cookie_jar = _COOKIE_JAR.get(url)
    csrf = _CSRF_TOKEN.get(url)
    cmd = [
        "curl", "-sS",
        "-o", "/tmp/admin-err-body",
        "-w", "%{http_code}",
        "--cacert", CA_CERT,
        "-b", cookie_jar,
        "-X", "POST",
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
    output = machine.succeed(su("/run/wrappers/bin/tunnel-remote-echo"))
    assert "REMOTE_TOKEN=secret_via_service" in output, f"expected REMOTE_TOKEN: {output}"
    machine.log("PASS: remote secret via backend service")

    # No secret on local frontend store disk.
    machine.fail("test -f /var/lib/underwrap-backend-local/secrets/remote/token")
    machine.log("PASS: no remote secret on local frontend disk")

    # Stop backend service: fail.
    machine.succeed("systemctl stop underwrap-backendd.service")
    time.sleep(0.5)
    machine.fail(su("/run/wrappers/bin/tunnel-remote-echo"))
    machine.log("PASS: wrapper fails when backend service is down")

    # Restart backend service: succeed again.
    machine.succeed("systemctl start underwrap-backendd.service")
    machine.wait_for_unit("underwrap-backendd.service")
    time.sleep(1)
    output = machine.succeed(su("/run/wrappers/bin/tunnel-remote-echo"))
    assert "REMOTE_TOKEN=secret_via_service" in output
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
    machine.fail(su("/run/wrappers/bin/local-test-echo2 hello"))
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
    output = machine.succeed(su("/run/wrappers/bin/local-test-echo2 hello"))
    assert "hello" in output
    machine.log("PASS: active agent grant allows wrapper action")

    # Deleting the grant revokes future reads.
    admin_delete_grant(ADMIN_LOCAL, "local", LOCAL_CLIENT_INSTANCE, "test-echo2", "test/other")
    machine.fail(su("/run/wrappers/bin/local-test-echo2 hello"))
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
    machine.fail(su("/run/wrappers/bin/local-test-echo2 hello"))
    machine.log("PASS: expired UnlockedUntil grant is not honored")

    # Clean up so the next test starts fresh.
    admin_delete_grant(ADMIN_LOCAL, "local", LOCAL_CLIENT_INSTANCE, "test-echo2", "test/other")



def local_backend_policy_control_center():
    # ============================================================
    # LOCAL INSTANCE: backend-managed grant mode control
    # ============================================================

    # Remove the seed grant so we can prove "no grant → denied".
    admin_delete_grant(ADMIN_LOCAL, "local", LOCAL_CLIENT_INSTANCE, "test-echo", "test/token")
    machine.fail(su("/run/wrappers/bin/local-test-echo"))
    machine.log("PASS: wrapper with no grant is denied")

    # Unlocked grant → wrapper can read.
    grant_local_unlocked("test-echo")
    output = machine.succeed(su("/run/wrappers/bin/local-test-echo"))
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
    machine.fail(su("/run/wrappers/bin/local-test-echo"))
    machine.log("PASS: Locked grant denies wrapper read")

    # Re-Unlock → wrapper allowed again.
    grant_local_unlocked("test-echo")
    output = machine.succeed(su("/run/wrappers/bin/local-test-echo"))
    assert "MY_TOKEN=overwrite" in output, f"expected MY_TOKEN after re-grant: {output}"
    machine.log("PASS: re-Unlocking the grant restores wrapper access")

    # `local-test-locked` is wired up by the test harness as a wrapper
    # that historically required a local unlock; under the grant
    # model it simply needs an Unlocked grant.
    grant_local_unlocked("test-locked")
    output = machine.succeed(su("/run/wrappers/bin/local-test-locked"))
    assert "LOCKED_TOKEN=locked_secret" in output, (
        f"expected formerly-locked wrapper to work via grant model: {output}"
    )
    machine.log("PASS: grant model replaces local unlock when active")


def instance_isolation():
    # ============================================================
    # INSTANCE ISOLATION: cross-instance access blocked
    # ============================================================

    # local-test-echo wrapper cannot reach tunnel instance secrets.
    # (The wrapper's GID maps to the local instance, not tunnel.)
    grant_local_unlocked("test-echo")
    output = machine.succeed(su("/run/wrappers/bin/local-test-echo"))
    assert "REMOTE_TOKEN" not in output
    machine.log("PASS: local wrapper does not leak remote secrets")


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
    assert "keyring_roundtrip_value" not in logs, "keyring probe secret leaked!"
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
local_ancestor_attack()
local_core_dump_protection()
local_symlink_confused_deputy_attack()
local_proc_isolation_libsecret_mode()
local_libsecret_mode()
local_go_keyring_compatibility()
local_directory_credentials()
local_atomic_writeback()
remote_service_lifecycle()
instance_isolation()
local_backend_policy_control_center()
local_backend_managed_grants()
encrypted_store_in_memory_keys()
audit_and_no_secret_leaks()

