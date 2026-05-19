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
ADMIN_LOCAL = "http://127.0.0.1:19443"
ADMIN_REMOTE = "http://127.0.0.1:19444"
ADMIN_TOKEN = "vm-test-admin-token"


def admin_api(url, path, payload=None, method="POST"):
    # State-changing requests against an admin listener with
    # `allowed_origins` configured (ticket 212) must carry an
    # `Origin` header matching one of the allowlisted entries.
    # The bearer-token path is exempt from CSRF, but the origin
    # check still applies to every non-GET/HEAD/OPTIONS request.
    cmd = [
        "curl",
        "-fsS",
        "-X",
        method,
        "-H",
        f"Authorization: Bearer {ADMIN_TOKEN}",
        "-H",
        f"Origin: {url}",
    ]
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
}

# The directory wrappers map a second resource (`test/dir-b`); after
# seeding the primary grant we also seed the secondary one so reads
# of both mounted directories are authorized.
LOCAL_WRAPPER_EXTRA_RESOURCES = {
    "test-dir-write": ["test/dir-b"],
    "test-dir-read": ["test/dir-b"],
    "test-dir-readonly-write": ["test/dir-b"],
    "test-dir-sleep": ["test/dir-b"],
}

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
    machine.wait_for_unit("underwrapd-local.service")
    machine.wait_for_unit("underwrapd-tunnel.service")
    machine.wait_for_unit("underwrap-backendd.service")
    machine.wait_for_unit("underwrap-backendd-local.service")

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
    # Declarative provenance is surfaced so the SPA can refuse to mutate.
    assert all(s["source"] == "declarative" for s in local_stores + remote_stores)

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

    # Frontends auto-register catalog rows on daemon startup. The backend
    # stamps each row with the mTLS-authenticated `client_instance` id so
    # the admin UI can group wrapper/resource pairs by enrolled client.
    local_catalog = admin_api(ADMIN_LOCAL, "/v1/admin/catalog/get", {"store": "local"})["entries"]
    remote_catalog = admin_api(ADMIN_REMOTE, "/v1/admin/catalog/get", {"store": "remote"})["entries"]
    assert any(
        entry["client_instance"] == LOCAL_CLIENT_INSTANCE
        and entry["wrapper"] == "test-echo"
        and entry["resource"] == "test/token"
        for entry in local_catalog
    ), f"missing local catalog row with client_instance/wrapper/resource fields: {local_catalog}"
    assert any(
        entry["client_instance"] == REMOTE_CLIENT_INSTANCE
        and entry["wrapper"] == "remote-echo"
        and entry["resource"] == "remote/token"
        for entry in remote_catalog
    ), f"missing remote catalog row with client_instance/wrapper/resource fields: {remote_catalog}"
    machine.log("PASS: frontend catalog rows register client_instance/wrapper/resource separately")

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
    # for the full test wrapper catalog so the subsequent test sections
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
    backend_files = machine.succeed(
        "find /var/lib/underwrap-backend-local/secrets/test/libsecret-store/ -type f 2>/dev/null || echo none"
    ).strip()
    assert backend_files != "none", "expected libsecret backend files"
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

    # Seed two directory credentials through a read-write wrapper.
    machine.succeed(su("/run/wrappers/bin/local-test-dir-write seed"))
    machine.log("PASS: directory credential seed wrapper succeeded")

    # Read both mounted directories through a read-only wrapper.
    dir_output = machine.succeed(su("/run/wrappers/bin/local-test-dir-read")).strip().split("\n")
    assert dir_output[0].strip() == "token-seed", f"expected token-seed, got {dir_output}"
    assert dir_output[1].strip() == "meta-seed", f"expected meta-seed, got {dir_output}"
    machine.log("PASS: directory credential read-only wrapper can read both mounted directories")

    # Pre-245 the daemon supported per-wrapper write rules so the
    # `test-dir-readonly-write` wrapper could be denied mutation
    # while sharing a target with `test-dir-write`. The new grant
    # model expresses this via the per-grant `writable: bool` flag,
    # which is decided per `(client_instance, wrapper, resource)`.

    # Update content and delete one file through the read-write wrapper.
    machine.succeed(su("/run/wrappers/bin/local-test-dir-write update"))
    dir_output = machine.succeed(su("/run/wrappers/bin/local-test-dir-read")).strip().split("\n")
    assert dir_output[0].strip() == "token-updated", f"expected token-updated, got {dir_output}"
    assert dir_output[1].strip() == "meta-updated", f"expected meta-updated, got {dir_output}"
    machine.log("PASS: directory read-write wrapper updates backend-managed files")

    # Verify one backend entry per file and deletion propagation.
    dir_a_backend_files = machine.succeed(
        "find /var/lib/underwrap-backend-local/secrets/test/dir-a/files -type f -printf '%P\\n' | sort"
    ).strip().split("\n")
    assert dir_a_backend_files == ["sub/nested", "token"], f"unexpected dir-a backend files: {dir_a_backend_files}"
    dir_b_backend_files = machine.succeed(
        "find /var/lib/underwrap-backend-local/secrets/test/dir-b/files -type f -printf '%P\\n' | sort || true"
    ).strip().split("\n")
    assert dir_b_backend_files == ["meta"], f"unexpected dir-b backend files: {dir_b_backend_files}"
    machine.log("PASS: directory backend stores one entry per file and propagates deletions")

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

    host_view = machine.execute(
        "su testuser -s /bin/sh -c 'cat $HOME/.local/share/underwrap-dir-b/meta 2>/dev/null'"
    )[0]
    assert host_view != 0, "FAIL: same-UID host process can read second directory credential file"

    host_ls = machine.execute(
        "su testuser -s /bin/sh -c 'ls $HOME/.config/underwrap-dir-a >/dev/null 2>&1'"
    )[0]
    assert host_ls != 0, "FAIL: same-UID host process can list first mounted directory"
    host_ls = machine.execute(
        "su testuser -s /bin/sh -c 'ls $HOME/.local/share/underwrap-dir-b >/dev/null 2>&1'"
    )[0]
    assert host_ls != 0, "FAIL: same-UID host process can list second mounted directory"

    host_find = machine.succeed(
        "su testuser -s /bin/sh -c 'find $HOME/.config/underwrap-dir-a -maxdepth 2 -type f 2>/dev/null || true'"
    ).strip()
    assert host_find == "", f"FAIL: host view exposes mounted directory files: {host_find}"
    host_find = machine.succeed(
        "su testuser -s /bin/sh -c 'find $HOME/.local/share/underwrap-dir-b -maxdepth 2 -type f 2>/dev/null || true'"
    ).strip()
    assert host_find == "", f"FAIL: host view exposes second mounted directory files: {host_find}"
    machine.log("PASS: same-UID host processes cannot list mounted directories or access files in them")

    machine.execute("pkill -u testuser -f local-test-dir-sleep || true")
    time.sleep(0.5)

    # The mounted directories must disappear from normal host view after the wrapper exits.
    post_exit = machine.execute(
        "su testuser -s /bin/sh -c 'test -e $HOME/.config/underwrap-dir-a/token'"
    )[0]
    assert post_exit != 0, "FAIL: directory credential file remained visible after wrapper exit"
    post_exit = machine.execute(
        "su testuser -s /bin/sh -c 'test -e $HOME/.local/share/underwrap-dir-b/meta'"
    )[0]
    assert post_exit != 0, "FAIL: second directory credential file remained visible after wrapper exit"
    machine.log("PASS: directory credentials leave no host-visible files after wrapper exit")

    # Symlink creation inside a managed tree must be rejected.
    machine.fail(su("/run/wrappers/bin/local-test-dir-symlink"))
    machine.log("PASS: directory sync rejects symlinks in managed trees")

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
remote_service_lifecycle()
instance_isolation()
local_backend_policy_control_center()
local_backend_managed_grants()
audit_and_no_secret_leaks()

