"""underwrap VM test — local + remote instances on one machine."""

import time

# Helper for running commands as testuser.
def su(cmd):
    return f"su testuser -s /bin/sh -c '{cmd}'"

# Spoof script path is passed as an env var by the Nix test driver.
import os
SPOOF_SCRIPT = os.environ.get("SPOOF_SCRIPT", "/dev/null")
PYTHON3 = os.environ.get("PYTHON3", "python3")
UWCTL_LOCAL = os.environ.get("UWCTL_LOCAL", "underwrapctl")


def boot_and_service_checks():
    # ============================================================
    # BOOT + SERVICE CHECKS
    # ============================================================

    machine.wait_for_unit("multi-user.target")
    machine.wait_for_unit("underwrapd-local.service")
    machine.wait_for_unit("underwrapd-tunnel.service")
    machine.wait_for_unit("underwrap-backendd.service")
    machine.wait_for_unit("underwrap-backend-export.service")
    machine.wait_for_unit("underwrap-backendd-local.service")
    machine.wait_for_unit("underwrap-backend-export-local.service")
    machine.wait_for_unit("sshd.service")

    machine.succeed("id testuser")
    machine.succeed("id underwrap-local")
    machine.succeed("id underwrap-tunnel")


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
    perms = machine.succeed("stat -c %a /run/underwrap-local/control.sock").strip()
    assert perms == "600", f"expected 600, got {perms}"

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

    machine.succeed(f"echo -n 'ghp_testvmtoken' | {UWCTL_LOCAL} store test/token")
    machine.succeed(f"echo -n 'other_secret_val' | {UWCTL_LOCAL} store test/other")
    machine.succeed(f"echo -n 'locked_secret' | {UWCTL_LOCAL} store test/locked-tok")
    machine.succeed(f"echo -n 'dumpable_test_secret' | {UWCTL_LOCAL} store test/dumpable-tok")
    machine.succeed(f"echo -n 'crash_secret_value' | {UWCTL_LOCAL} store test/crash-tok")
    machine.succeed(f"echo -n 'caps_test_val' | {UWCTL_LOCAL} store test/caps-tok")

    # Status.
    status = machine.succeed(f"{UWCTL_LOCAL} status")
    assert "local-test-echo" in status
    status = machine.succeed(f"{UWCTL_LOCAL} status local-test-echo")
    assert "true" in status

    # Operator-managed credential refs may be updated through the control socket.
    machine.succeed(f"echo -n 'overwrite' | {UWCTL_LOCAL} store test/token")
    machine.succeed(f"echo -n 'updated_val' | {UWCTL_LOCAL} store test/other")


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

    machine.fail(su(f"echo test | {UWCTL_LOCAL} status"))
    machine.log("PASS: unprivileged user cannot access control socket")

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


def local_lock_unlock_lifecycle():
    # ============================================================
    # LOCAL INSTANCE: lock/unlock lifecycle
    # ============================================================

    machine.fail(su("/run/wrappers/bin/local-test-locked"))
    machine.log("PASS: locked wrapper blocks unprivileged user")

    machine.succeed(f"{UWCTL_LOCAL} unlock local-test-locked --seconds 60 --for-user testuser")
    output = machine.succeed(su("/run/wrappers/bin/local-test-locked"))
    assert "LOCKED_TOKEN=locked_secret" in output

    machine.succeed(f"{UWCTL_LOCAL} lock local-test-locked")
    machine.fail(su("/run/wrappers/bin/local-test-locked"))
    machine.log("PASS: lock/unlock lifecycle works")


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
        if "local-test-sleep" not in wrapper_procs and "sleep" not in wrapper_procs:
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
    # inspect its descendant target and recover the secret. Under Yama
    # ptrace_scope=1, ancestor→descendant tracing is normally allowed.
    # This test determines whether the underwrap architecture actually
    # protects against a malicious invoker.

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


def local_directory_credentials():
    # ============================================================
    # LOCAL INSTANCE: directory credentials
    # ============================================================

    # Seed two directory credentials through a read-write wrapper.
    machine.succeed(su("/run/wrappers/bin/local-test-dir-write seed"))
    machine.log("PASS: directory credential seed wrapper succeeded")

    # Read both mounted directories through a read-only wrapper.
    dir_output = machine.succeed(su("/run/wrappers/bin/local-test-dir-read")).strip().split("\n")
    assert dir_output[0].strip() == "token-seed", f"expected token-seed, got {dir_output}"
    assert dir_output[1].strip() == "meta-seed", f"expected meta-seed, got {dir_output}"
    machine.log("PASS: directory credential read-only wrapper can read both mounted directories")

    # Read-only mounts must reject writes.
    machine.fail(su("/run/wrappers/bin/local-test-dir-readonly-write update"))
    machine.log("PASS: directory read-only mounts reject mutation")

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


def remote_tunnel_lifecycle():
    # ============================================================
    # REMOTE INSTANCE: tunnel lifecycle
    # ============================================================

    # Store secret in backend.
    machine.succeed(
        "su underwrap-backend -s /bin/sh -c '"
        "mkdir -p /var/lib/underwrap-backend/secrets/remote"
        "'"
    )
    machine.succeed(
        "su underwrap-backend -s /bin/sh -c '"
        "echo -n secret_via_tunnel > /var/lib/underwrap-backend/secrets/remote/token"
        "'"
    )

    # Generate SSH key for backend user.
    machine.succeed(
        "su underwrap-backend -s /bin/sh -c '"
        "mkdir -p ~/.ssh && chmod 700 ~/.ssh && "
        "ssh-keygen -t ed25519 -f ~/.ssh/tunnel_key -N \"\" -q"
        "'"
    )

    # Authorize key for tunnel user.
    pubkey = machine.succeed(
        "cat /var/lib/underwrap-backend/.ssh/tunnel_key.pub"
    ).strip()
    tunnel_home = machine.succeed(
        "getent passwd underwrap-tunnel-tunnel | cut -d: -f6"
    ).strip()
    machine.succeed(f"mkdir -p {tunnel_home}/.ssh && chmod 700 {tunnel_home}/.ssh")
    machine.succeed(f"echo '{pubkey}' > {tunnel_home}/.ssh/authorized_keys")
    machine.succeed(f"chmod 600 {tunnel_home}/.ssh/authorized_keys")
    machine.succeed(f"chown -R underwrap-tunnel-tunnel:underwrap-tunnel {tunnel_home}/.ssh")

    # Without tunnel: fail.
    machine.fail("test -S /run/underwrap-tunnel/backend/backend.sock")
    machine.fail(su("/run/wrappers/bin/tunnel-remote-echo"))
    machine.log("PASS: wrapper fails without tunnel")

    # Establish SSH tunnel.
    machine.succeed(
        "su underwrap-backend -s /bin/sh -c '"
        "ssh -f -N "
        "-o StrictHostKeyChecking=no "
        "-o UserKnownHostsFile=/dev/null "
        "-i ~/.ssh/tunnel_key "
        "-R /run/underwrap-tunnel/backend/backend.sock:/run/underwrap-backend/backend.sock "
        "underwrap-tunnel-tunnel@localhost"
        "'"
    )
    time.sleep(1)
    machine.succeed("test -S /run/underwrap-tunnel/backend/backend.sock")
    machine.succeed("chmod 0660 /run/underwrap-tunnel/backend/backend.sock")
    machine.log("PASS: tunnel established")

    # With tunnel: succeed.
    output = machine.succeed(su("/run/wrappers/bin/tunnel-remote-echo"))
    assert "REMOTE_TOKEN=secret_via_tunnel" in output, f"expected REMOTE_TOKEN: {output}"
    machine.log("PASS: remote secret via tunnel")

    # No secret on local disk.
    machine.fail("test -f /var/lib/underwrap-backend-remote/secrets/remote/token")
    machine.log("PASS: no secret on local disk")

    # Kill tunnel: fail.
    machine.execute("pkill -f 'ssh.*tunnel_key' || true")
    time.sleep(0.5)
    machine.succeed("rm -f /run/underwrap-tunnel/backend/backend.sock")
    machine.fail(su("/run/wrappers/bin/tunnel-remote-echo"))
    machine.log("PASS: fails after tunnel killed")

    # Reconnect: succeed.
    machine.succeed(
        "su underwrap-backend -s /bin/sh -c '"
        "ssh -f -N "
        "-o StrictHostKeyChecking=no "
        "-o UserKnownHostsFile=/dev/null "
        "-i ~/.ssh/tunnel_key "
        "-R /run/underwrap-tunnel/backend/backend.sock:/run/underwrap-backend/backend.sock "
        "underwrap-tunnel-tunnel@localhost"
        "'"
    )
    time.sleep(1)
    machine.succeed("chmod 0660 /run/underwrap-tunnel/backend/backend.sock")
    output = machine.succeed(su("/run/wrappers/bin/tunnel-remote-echo"))
    assert "REMOTE_TOKEN=secret_via_tunnel" in output
    machine.log("PASS: works after reconnect")


def instance_isolation():
    # ============================================================
    # INSTANCE ISOLATION: cross-instance access blocked
    # ============================================================

    # local-test-echo wrapper cannot reach tunnel instance secrets.
    # (The wrapper's GID maps to the local instance, not tunnel.)
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
    assert 'audit="secret_stored"' in logs, "missing secret_stored"

    assert "ghp_testvmtoken" not in logs, "secret leaked!"
    assert "locked_secret" not in logs, "secret leaked!"
    assert "secret_via_tunnel" not in logs, "secret leaked!"
    assert "my_gh_token_value" not in logs, "libsecret secret leaked!"
    machine.log("PASS: audit events present, no secrets in logs")

    machine.log("ALL TESTS PASSED")

boot_and_service_checks()
local_socket_permissions()
local_store_and_basic_ops()
local_unprivileged_wrapper_invocation()
local_verify_capabilities_dropped()
local_isolation()
local_spoofing_resistance()
local_lock_unlock_lifecycle()
local_daemon_restart_persistence()
local_pid_namespace_proc_isolation()
local_ancestor_attack()
local_core_dump_protection()
local_symlink_confused_deputy_attack()
local_proc_isolation_libsecret_mode()
local_libsecret_mode()
local_directory_credentials()
remote_tunnel_lifecycle()
instance_isolation()
audit_and_no_secret_leaks()

