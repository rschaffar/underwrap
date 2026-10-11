"""underwrap VM test — local and remote-style instances use direct loopback on one machine."""

import base64
import binascii
import contextlib
import errno
import hashlib
import json
import os
import re
import shlex
import signal
import tempfile
import time
import urllib.parse
import uuid


def _seg(value):
    """Percent-encode a single URL path segment. Slashes inside the
    value (e.g. resource names like `github/token`) become `%2F` so
    they don't collide with the path delimiter."""
    return urllib.parse.quote(value, safe="")

# Helper for running commands as testuser.
def su(cmd):
    return f"su testuser -s /bin/sh -c '{cmd}'"


def stage_text_secret_for_testuser(value, guest_path):
    """Stage a textual secret in a mode-0600 guest file owned by testuser."""
    with tempfile.NamedTemporaryFile(mode="w", suffix=".secret", delete=False) as secret:
        secret.write(value)
        host_path = secret.name
    try:
        machine.copy_from_host(host_path, guest_path)
        quoted_guest_path = shlex.quote(guest_path)
        machine.succeed(
            f"chmod 600 {quoted_guest_path} && chown testuser {quoted_guest_path}"
        )
    finally:
        os.unlink(host_path)


# Spoof script path is passed as an env var by the Nix test driver.
SPOOF_SCRIPT = os.environ.get("SPOOF_SCRIPT", "/dev/null")
PEER_PROBE_SCRIPT = os.environ.get("PEER_PROBE_SCRIPT", "/dev/null")
DIRECT_NS_ATTACK_SCRIPT = os.environ.get("DIRECT_NS_ATTACK_SCRIPT", "/dev/null")
PYTHON3 = os.environ.get("PYTHON3", "python3")

# Each daemon now runs a single HTTPS listener that serves both the
# runtime service API (mTLS-authenticated, `/v1/...`) and the admin
# SPA + JSON API (session-cookie + CSRF authenticated, `/v1/admin/...`).
ADMIN_LOCAL = "https://127.0.0.1:18443"
ADMIN_REMOTE = "https://127.0.0.1:18444"
CA_CERT = "/etc/underwrap-test-ca.pem"
ADMIN_BOOTSTRAP_USER = "tester"
# Public fixture material, not a credential. Guest-side generators derive the
# synthetic password and write it directly into a pipe; the derived value never
# enters a Nix test command string, argv, environment, or configuration.
ADMIN_PASSWORD_FIXTURE_MATERIAL = "underwrap-vm-admin-password-v1"
MAX_CANONICAL_RESOURCE_BYTES = 2 * 1024 * 1024
SIZE_ENV_VALUE = b"size-env-value-" + b"e" * (96 * 1024)
SIZE_FILES_INITIAL = b"size-files-seed"
SIZE_FILES_REPLACEMENT = b"r" * len(SIZE_FILES_INITIAL)
SIZE_FILES_OVERSIZE = SIZE_FILES_INITIAL + b"x"
SIZE_ADMIN_OVERSIZE = b"size-admin-oversize"

# Per-admin session-cookie state, populated by `login_admin_session`
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


def backend_config_path(unit):
    """Read the immutable config path from the packaged unit definition."""
    unit_text = machine.succeed(f"systemctl cat {shlex.quote(unit)}")
    match = re.search(r"--config[ =]([^ \n]+)", unit_text)
    if not match:
        raise AssertionError(f"could not find --config in {unit} unit")
    return match.group(1)


def _admin_password_stream(*, as_login_json=False):
    """Return a guest command that writes the derived fixture only to stdout."""
    digest = (
        f"hashlib.sha256({ADMIN_PASSWORD_FIXTURE_MATERIAL.encode()!r}).hexdigest()"
    )
    if as_login_json:
        source = (
            "import hashlib,json,sys;"
            f"password={digest};"
            "sys.stdout.write(json.dumps({"
            f"'username':{ADMIN_BOOTSTRAP_USER!r},'password':password"
            "},separators=(',',':')))"
        )
    else:
        source = (
            "import hashlib,sys;"
            f"sys.stdout.write({digest}+'\\n')"
        )
    return f"{shlex.quote(PYTHON3)} -c {shlex.quote(source)}"


def provision_backend_operator(unit, service_user, state_dir):
    """Provision before listener startup through the real stdin-only CLI."""
    config = backend_config_path(unit)
    backend_command = (
        f"underwrap-backend --config {shlex.quote(config)} admin bootstrap-user "
        f"--username {shlex.quote(ADMIN_BOOTSTRAP_USER)}"
    )

    # The ordinary user must fail specifically at the service-owned DAC
    # boundary, without creating either authoritative artifact.
    ordinary_command = (
        f"{_admin_password_stream()} | runuser -u testuser -- {backend_command} 2>&1"
    )
    status, output = machine.execute(ordinary_command)
    assert status != 0, "ordinary user unexpectedly provisioned an operator"
    assert "Permission denied" in output, output
    machine.succeed(
        f"test ! -e {shlex.quote(state_dir + '/bootstrap.json')} && "
        f"{{ test ! -d {shlex.quote(state_dir + '/users')} || "
        f"! find {shlex.quote(state_dir + '/users')} -name '*.json' -print -quit "
        "| grep -q .; }"
    )

    service_command = (
        f"{_admin_password_stream()} | runuser -u {shlex.quote(service_user)} -- "
        f"{backend_command}"
    )
    machine.succeed(service_command)
    machine.succeed(
        f"test -f {shlex.quote(state_dir + '/bootstrap.json')} && "
        f"test $(find {shlex.quote(state_dir + '/users')} -name '*.json' | wc -l) -eq 1"
    )


def login_admin_session(url):
    """Re-authenticate against an already-bootstrapped backend, e.g.
    after a daemon restart that wiped the in-memory session state.
    Captures a fresh session cookie + CSRF authenticity token from
    `/v1/admin/auth/login`."""
    cookie_jar = _cookie_jar_path(url)
    headers_file = f"{cookie_jar}.headers"
    cmd = [
        "curl", "-fsS",
        "--cacert", CA_CERT,
        "-c", cookie_jar,
        "-D", headers_file,
        "-X", "POST",
        "-H", f"Origin: {url}",
        "-H", "Content-Type: application/json",
        "--data-binary", "@-",
        f"{url}/v1/admin/auth/login",
    ]
    quoted_curl = " ".join(shlex.quote(p) for p in cmd)
    machine.succeed(f"{_admin_password_stream(as_login_json=True)} | {quoted_curl}")
    headers = machine.succeed(f"cat {shlex.quote(headers_file)}")
    csrf = _extract_csrf_response_header(headers)
    if not csrf:
        raise AssertionError(
            f"login_admin_session: no X-CSRF-Token response header in {headers_file}"
        )
    _COOKIE_JAR[url] = cookie_jar
    _CSRF_TOKEN[url] = csrf


@contextlib.contextmanager
def _admin_json_body(payload):
    """Copy one JSON body through a mode-0600 file so secret-bearing
    request data never appears in the guest process argv or shell command."""
    guest_path = f"/run/underwrap-admin-body-{uuid.uuid4().hex}"
    with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as body:
        json.dump(payload, body, separators=(",", ":"))
        host_path = body.name
    try:
        machine.copy_from_host(host_path, guest_path)
        machine.succeed(f"chmod 0600 {shlex.quote(guest_path)}")
        yield guest_path
    finally:
        machine.succeed(f"rm -f {shlex.quote(guest_path)}")
        os.unlink(host_path)


def admin_api(url, path, payload=None, method="POST"):
    """Make an authenticated admin API call. Uses the session cookies
    populated by `login_admin_session` for the listener at `url`
    and adds an `X-CSRF-Token` header on every state-changing request
    so the backend's CSRF double-submit check passes.
    """
    is_state_changing = method not in ("GET", "HEAD", "OPTIONS")
    cookie_jar = _COOKIE_JAR.get(url)
    csrf = _CSRF_TOKEN.get(url)
    if cookie_jar is None or csrf is None:
        raise AssertionError(
            f"admin_api({url!r}, …): login_admin_session was never "
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
    if payload is None:
        cmd += [f"{url}{path}"]
        out = machine.succeed(" ".join(shlex.quote(part) for part in cmd))
    else:
        with _admin_json_body(payload) as body_path:
            cmd += [
                "-H", "Content-Type: application/json",
                "--data-binary", f"@{body_path}",
                f"{url}{path}",
            ]
            out = machine.succeed(" ".join(shlex.quote(part) for part in cmd))
    return json.loads(out) if out.strip() else None


def admin_api_expect_status(url, path, payload, *, status, method="POST"):
    """Submit an authenticated request and decode an expected non-2xx JSON
    response without placing request material in argv.

    The response capture is unique and removed even when the assertion fails;
    `_admin_json_body` keeps a supplied request body in a mode-0600 file.
    """
    is_state_changing = method not in ("GET", "HEAD", "OPTIONS")
    cookie_jar = _COOKIE_JAR.get(url)
    csrf = _CSRF_TOKEN.get(url)
    if cookie_jar is None or csrf is None:
        raise AssertionError(f"admin session is unset for {url!r}")
    response_path = f"/run/underwrap-admin-response-{uuid.uuid4().hex}.json"
    cmd = [
        "curl", "-sS", "-o", response_path, "-w", "%{http_code}",
        "--cacert", CA_CERT, "-b", cookie_jar, "-X", method,
        "-H", f"Origin: {url}",
    ]
    if is_state_changing:
        cmd += ["-H", f"X-CSRF-Token: {csrf}"]
    try:
        if payload is None:
            cmd += [f"{url}{path}"]
            code = machine.succeed(" ".join(shlex.quote(part) for part in cmd)).strip()
        else:
            with _admin_json_body(payload) as body_path:
                cmd += [
                    "-H", "Content-Type: application/json",
                    "--data-binary", f"@{body_path}", f"{url}{path}",
                ]
                code = machine.succeed(
                    " ".join(shlex.quote(part) for part in cmd)
                ).strip()
        assert code == str(status), f"expected HTTP {status} from {path}, got {code}"
        response = machine.succeed(f"cat {shlex.quote(response_path)}")
        return json.loads(response) if response.strip() else {}
    finally:
        machine.succeed(f"rm -f {shlex.quote(response_path)}")


def admin_write_secret(url, store, wrapper, secret_id, value):
    """Admin-side secret write. `secret_id` is the
    `<resource_name>/<slot>/<key>` path used in test fixtures; we
    split it from the right (slot + key are single segments) and
    translate the resource name to its backend id. `wrapper` is kept
    in the signature for call-site readability but is not on the
    new wire shape."""
    del wrapper  # kept for call-site readability only
    resource_name, slot, key = secret_id.rsplit("/", 2)
    return admin_write_resource_value(url, store, resource_name, slot, key, value.encode())


def admin_write_resource_value(url, store, resource_name, slot, key, value):
    """Write arbitrary bytes to an explicit resource slot/key.

    Unlike `admin_write_secret`, the key is a separate argument and may contain
    slashes, which is required for deployed files-projection fixtures.
    """
    rid = _resource_id_for(url, resource_name)
    return admin_api(
        url,
        f"/v1/admin/stores/{store}/secrets/{_seg(rid)}/{_seg(slot)}/{_seg(key)}",
        {"material_b64": base64.b64encode(value).decode()},
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


def admin_upsert_permission(url, store, client_instance, wrapper, resource, mode, *, writable=False, reason=None):
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


def admin_delete_permission(url, store, client_instance, wrapper, resource):
    rid = _resource_id_for(url, resource)
    return admin_api(
        url,
        f"/v1/admin/stores/{store}/wrapper-permissions/{_seg(client_instance)}/{_seg(wrapper)}/{_seg(rid)}",
        method="DELETE",
    )


def install_local_plain_resource_blob(resource, canonical):
    """Install trusted test-generated canonical bytes by same-directory rename.

    This is root-only VM harness setup for the local PlainBackend. The transfer
    and final resource file are service-owned mode 0600; no secret bytes enter a
    shell command line, and every temporary guest artifact is removed.
    """
    assert isinstance(canonical, bytes)
    resource_id = _resource_id_for(ADMIN_LOCAL, resource)
    resource_dir = f"{_storage_dir_for(ADMIN_LOCAL, 'local')}/resources"
    destination = f"{resource_dir}/{resource_id}"
    transfer = f"{resource_dir}/.{resource_id}.size-transfer-{uuid.uuid4().hex}"
    with tempfile.NamedTemporaryFile(mode="wb", delete=False) as source:
        source.write(canonical)
        source.flush()
        os.fchmod(source.fileno(), 0o600)
        host_path = source.name
    try:
        machine.copy_from_host(host_path, transfer)
        machine.succeed(
            f"chmod 0600 {shlex.quote(transfer)} && "
            f"chown underwrap-local:underwrap-local {shlex.quote(transfer)} && "
            f"mv -f -- {shlex.quote(transfer)} {shlex.quote(destination)} && "
            f"test $(stat -c %a {shlex.quote(destination)}) = 600 && "
            f"test $(stat -c %U {shlex.quote(destination)}) = underwrap-local && "
            f"test $(stat -c %G {shlex.quote(destination)}) = underwrap-local"
        )
    finally:
        machine.succeed(f"rm -f -- {shlex.quote(transfer)}")
        os.unlink(host_path)


_BLOB_OBSERVATION_CHUNK_BYTES = 64 * 1024


def plain_resource_blob_fingerprint(url, store, resource):
    """Return only the on-disk blob length and SHA-256 digest."""
    resource_id = _resource_id_for(url, resource)
    storage_dir = _storage_dir_for(url, store)
    blob_path = f"{storage_dir}/resources/{resource_id}"
    summary = machine.succeed(
        f"wc -c < {shlex.quote(blob_path)}; "
        f"sha256sum {shlex.quote(blob_path)} | cut -d' ' -f1"
    ).splitlines()
    assert len(summary) == 2, f"unexpected blob fingerprint summary: {summary!r}"
    length, digest = summary
    assert length.isdigit(), f"invalid blob length summary: {length!r}"
    assert re.fullmatch(r"[0-9a-f]{64}", digest), f"invalid blob digest summary: {digest!r}"
    return int(length), digest


def admin_get_blob(url, store, resource):
    """Return exact canonical bytes as base64 via bounded driver transfers.

    One multi-megabyte shell response can be truncated by the NixOS test driver.
    Bounded transfers retain byte-for-byte assertions without retries or fixture logs.
    """
    resource_id = _resource_id_for(url, resource)
    storage_dir = _storage_dir_for(url, store)
    blob_path = f"{storage_dir}/resources/{resource_id}"
    before = plain_resource_blob_fingerprint(url, store, resource)
    chunk_count = (
        before[0] + _BLOB_OBSERVATION_CHUNK_BYTES - 1
    ) // _BLOB_OBSERVATION_CHUNK_BYTES
    chunks = []
    for chunk_index in range(chunk_count):
        command = (
            f"dd if={shlex.quote(blob_path)} "
            f"bs={_BLOB_OBSERVATION_CHUNK_BYTES} skip={chunk_index} "
            "count=1 status=none | base64 -w0"
        )
        status, encoded = machine.execute(command)
        assert status == 0, (
            f"resource blob chunk read failed: chunk={chunk_index} status={status}"
        )
        encoded = encoded.strip()
        try:
            chunk = base64.b64decode(encoded, validate=True)
        except binascii.Error as error:
            raise AssertionError(
                f"resource blob chunk transport was not valid base64: "
                f"chunk={chunk_index} characters={len(encoded)}"
            ) from error
        expected_length = min(
            _BLOB_OBSERVATION_CHUNK_BYTES,
            before[0] - chunk_index * _BLOB_OBSERVATION_CHUNK_BYTES,
        )
        assert len(chunk) == expected_length, (
            f"resource blob chunk transport changed length: chunk={chunk_index} "
            f"expected={expected_length} observed={len(chunk)}"
        )
        chunks.append(chunk)

    blob = b"".join(chunks)
    after = plain_resource_blob_fingerprint(url, store, resource)
    observed = (len(blob), hashlib.sha256(blob).hexdigest())
    assert after == before, (
        "resource blob changed during exact observation; "
        f"before length={before[0]} sha256={before[1]}; "
        f"after length={after[0]} sha256={after[1]}"
    )
    assert observed == before, (
        "resource blob chunk transport changed content; "
        f"guest length={before[0]} sha256={before[1]}; "
        f"driver length={observed[0]} sha256={observed[1]}"
    )
    return base64.b64encode(blob).decode()


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


# Wrapper → operator-facing resource name mapping for the local instance.
# `set_local_unlocked_permission` resolves the name through `LOCAL_RESOURCE_IDS` before constructing the stable resource id permission tuple.
LOCAL_WRAPPER_RESOURCES = {
    "test-echo": "test/token",
    "test-echo2": "test/other",
    "test-locked": "test/locked-tok",
    "test-sleep": "test/dumpable-tok",
    "test-descendant": "test/descendant-tok",
    "test-iso": "test/iso-tok",
    "test-iso-home": "test/iso-home-tok",
    "test-runtime-dir": "test/runtime-tok",
    "test-iso-dir": "test/iso-dir-tok",
    "test-iso-libsecret": "test/iso-libsecret-store",
    "test-crash": "test/crash-tok",
    "test-libsecret": "test/libsecret-store",
    "test-libsecret-bus-probe": "test/libsecret-store",
    "test-caps": "test/caps-tok",
    "test-ns-pids": "test/ns-pids-store",
    "test-ns-pids-env": "test/ns-pids-env-store",
    "test-staging-provenance": "test/staging-provenance",
    "test-dir-write": "test/dir-a",
    "test-dir-read": "test/dir-a",
    "test-dir-readonly-write": "test/dir-a",
    "test-dir-sleep": "test/dir-a",
    "test-dir-symlink": "test/dir-a",
    "test-dir-write-then-sleep": "test/dir-a",
    "test-dir-touch": "test/dir-a",
    "test-dir-env-probe": "test/dir-a",
    "test-dir-env-override-probe": "test/dir-a",
    "test-gate-unroutable": "test/gate-unroutable",
    "test-size-env": "test/size-env",
    "test-size-files": "test/size-files",
    "test-gate-exec-missing": "test/gate-exec-missing",
    "test-gate-exit-37": "test/gate-exit-37",
    "test-gate-sigkill": "test/gate-sigkill",
    "test-life-lifecycle": "test/life-lifecycle",
    "test-life-filecap": "test/life-filecap",
}

# Each wrapper now projects exactly one resource, so no secondary
# permissions are needed.

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


def set_local_unlocked_permission(wrapper, resource=None):
    """Set the `(local-client, wrapper, resource)` permission mode to
    Unlocked. Most VM-test scenarios just want the wrapper to work
    end-to-end without ceremony."""
    if resource is None:
        resource = LOCAL_WRAPPER_RESOURCES[wrapper]
    admin_upsert_permission(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        wrapper,
        resource,
        {"kind": "unlocked"},
        writable=True,
    )


def discovery_without_backend_access():
    """Public metadata works before backend startup and for a user with no aliases."""
    for unit in ("underwrap-backend.service", "underwrap-backend-local.service"):
        machine.fail(f"systemctl is-active --quiet {unit}")
    info = "/run/current-system/sw/bin/underwrap-info"
    invocation = f"su nobody -s /bin/sh -c '{info} --json'"
    inventory = json.loads(machine.succeed(invocation))
    assert inventory["schema_version"] == 1
    assert inventory["scope"] == "local-configuration"
    assert inventory["authorization"] == inventory["filesystem_access"] == "not-checked"
    assert inventory["instances"] == ["local", "tunnel"]
    keys = {
        "instance", "name", "description", "command", "alias", "access_paths",
        "projections", "fixed_home", "fixed_tmpdir",
    }
    for wrapper in inventory["wrappers"]:
        assert set(wrapper) == keys
        assert wrapper["command"] == [
            f"/run/wrappers/bin/underwrap-{wrapper['instance']}", wrapper["name"]
        ]
        assert wrapper["alias"] == {"name": wrapper["name"], "users": ["testuser"]}
        assert set(wrapper["projections"]) == {"env", "files", "libsecret"}
    echo = next(w for w in inventory["wrappers"] if w["instance"] == "local" and w["name"] == "test-echo")
    assert echo["description"] == "Synthetic environment-projection test target."
    selected = json.loads(machine.succeed(
        f"su nobody -s /bin/sh -c '{info} test-echo --instance local --json'"
    ))
    assert selected["wrappers"] == [echo]
    assert any(w["description"] == "" for w in inventory["wrappers"])
    text = machine.succeed(f"su nobody -s /bin/sh -c '{info} test-echo --instance local'")
    assert echo["description"] in text and "authorization and filesystem access are not checked" in text
    machine.fail(f"su nobody -s /bin/sh -c '{info} absent-wrapper --json'")
    machine.succeed("test $(stat -Lc '%a:%U:%G' /etc/underwrap/discovery.json) = 644:root:root")
    # No fallback to daemon configuration, credentials, live policy or target execution.
    machine.succeed("mv /etc/underwrap/discovery.json /etc/underwrap/discovery.json.saved")
    try:
        status, output = machine.execute(f"{invocation} 2>&1")
        assert status == 1, "missing catalog must return the CLI failure exit code"
        assert output.strip() == (
            "underwrap-info: cannot read /etc/underwrap/discovery.json; "
            "ask the host operator to install the NixOS discovery catalog"
        ), "missing catalog must return the static stderr diagnostic"
    finally:
        machine.succeed("mv /etc/underwrap/discovery.json.saved /etc/underwrap/discovery.json")
    assert json.loads(machine.succeed(invocation)) == inventory
    machine.log("PASS: public discovery works without aliases or running backends; missing catalog fails explicitly")


def boot_and_service_checks():
    # ============================================================
    # BOOT + SERVICE CHECKS
    # ============================================================

    machine.wait_for_unit("multi-user.target")

    machine.fail("systemctl is-enabled --quiet sshd.service")
    machine.fail("systemctl is-active --quiet sshd.service")
    machine.log("PASS: sshd.service is neither enabled nor active in the loopback-only VM")

    discovery_without_backend_access()

    # Neither backend listener is started until deployment-side filesystem
    # authority has created its first operator through the real offline CLI.
    provision_backend_operator(
        "underwrap-backend.service",
        "underwrap-backend",
        "/var/lib/underwrap-backend",
    )
    provision_backend_operator(
        "underwrap-backend-local.service",
        "underwrap-local",
        "/var/lib/underwrap-backend-local",
    )
    machine.succeed("systemctl start underwrap-backend.service")
    machine.succeed("systemctl start underwrap-backend-local.service")
    machine.wait_for_unit("underwrap-backend.service")
    machine.wait_for_unit("underwrap-backend-local.service")

    for url in (ADMIN_LOCAL, ADMIN_REMOTE):
        machine.wait_until_succeeds(
            f"curl -fsS --cacert {CA_CERT} {url}/v1/admin/auth/bootstrap/status >/dev/null"
        )
        hostile_status = machine.succeed(
            f"{_admin_password_stream(as_login_json=True)} | "
            "curl -sS -o /dev/null -w '%{http_code}' "
            f"--cacert {CA_CERT} -X POST -H {shlex.quote('Origin: ' + url)} "
            "-H 'Content-Type: application/json' --data-binary @- "
            f"{url}/v1/admin/auth/bootstrap"
        ).strip()
        assert hostile_status in ("404", "405"), hostile_status

    # Ordinary login, not bootstrap, supplies session + CSRF state.
    login_admin_session(ADMIN_LOCAL)
    login_admin_session(ADMIN_REMOTE)

    # Both frontend daemons connect directly to their same-machine loopback HTTPS listeners.
    # They start after the backends are reachable because startup sends an empty `POST /v1/wrappers` registration.
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

    # services.underwrap.hardenTargetHost defaults true and, absent an
    # operator override, sets kernel.yama.ptrace_scope=2. Confirm the VM uses
    # the expected value.
    scope = machine.succeed("cat /proc/sys/kernel/yama/ptrace_scope").strip()
    assert scope == "2", f"expected ptrace_scope=2 from hardenTargetHost, got {scope}"
    machine.log("PASS: kernel.yama.ptrace_scope=2 enforced by hardenTargetHost")



def local_socket_permissions():
    # ============================================================
    # LOCAL INSTANCE: one socket / one group / one setgid binary
    # ============================================================
    # Each instance has exactly one group, socket, and setgid entry binary. The
    # wrapper name is a client-supplied selector validated by the daemon; the
    # kernel gate is the single instance-group egid.

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

    # Only the instance group exists; wrapper-specific groups must not.
    machine.succeed("getent group underwrap-local")
    machine.fail("getent group underwrap-local-test-echo")
    machine.fail("getent group underwrap-local-test-echo2")

    # One setgid entrypoint per instance, owned by the instance group.
    stat = machine.succeed("stat -c '%A %G' /run/wrappers/bin/underwrap-local").strip()
    assert "underwrap-local" in stat, f"wrong group: {stat}"
    assert "s" in stat.lower(), f"setgid not set: {stat}"
    # Wrapper-specific setgid binaries must not exist.
    machine.fail("test -e /run/wrappers/bin/local-test-echo")

    # Instance configs: full DaemonConfig + minimal underwrap-exec sidecar.
    machine.succeed("test -f /etc/underwrap/daemon/local.yaml")
    machine.succeed("test -f /etc/underwrap/instances/local.yaml")
    daemon_config = json.loads(
        machine.succeed("cat /etc/underwrap/daemon/local.yaml")
    )
    sidecar_config = json.loads(
        machine.succeed("cat /etc/underwrap/instances/local.yaml")
    )
    assert "ns_binary" not in daemon_config, daemon_config
    assert set(sidecar_config) == {
        "socket",
        "ns_binary",
        "dbus_daemon_binary",
        "secretsd_binary",
    }, sidecar_config
    assert sidecar_config["ns_binary"] == "/run/wrappers/bin/underwrap-ns", sidecar_config
    # Wrapper-specific sidecar configs must not exist.
    machine.fail("test -e /etc/underwrap/wrappers/local-test-echo.yaml")

    # underwrap-ns has the capabilities it needs for namespace setup and
    # attempts bounding-set cleanup where the kernel permits it.
    caps = machine.succeed("getcap /run/wrappers/bin/underwrap-ns").strip()
    cap_names = set(re.findall(r"cap_[a-z0-9_]+", caps))
    assert cap_names == {
        "cap_setgid",
        "cap_setpcap",
        "cap_setuid",
        "cap_sys_admin",
    }, f"unexpected underwrap-ns capability set: {caps}"
    assert "cap_kill" not in cap_names


def local_store_and_basic_ops():
    # ============================================================
    # LOCAL INSTANCE: store secrets + basic operations
    # ============================================================

    # Admin writes through the per-secret PUT endpoint are unconditional;
    # per-permission `writable` controls only wrapper-side persistence.

    # Every wrapper invocation needs a permission for
    # `(client_instance, wrapper, resource)`. Pre-seed Unlocked permissions
    # for the full wrapper/resource fixture so the subsequent test sections
    # can drive the wrappers without per-test boilerplate. Tests that
    # exercise the *absence* of a permission (or `Locked` / `UnlockedUntil`)
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
        ("test-crash", "test/crash-tok"),
        ("test-libsecret", "test/libsecret-store"),
        ("test-caps", "test/caps-tok"),
        ("test-ns-pids", "test/ns-pids-store"),
        ("test-ns-pids-env", "test/ns-pids-env-store"),
    ]:
        set_local_unlocked_permission(wrapper, resource)

    admin_write_secret(ADMIN_LOCAL, "local", "test-echo", "test/token/env/MY_TOKEN", "ghp_testvmtoken")
    admin_write_secret(ADMIN_LOCAL, "local", "test-echo2", "test/other/env/OTHER_SECRET", "other_secret_val")
    admin_write_secret(ADMIN_LOCAL, "local", "test-locked", "test/locked-tok/env/LOCKED_TOKEN", "locked_secret")
    admin_write_secret(ADMIN_LOCAL, "local", "test-sleep", "test/dumpable-tok/env/DUMPABLE_SECRET", "dumpable_test_secret")
    admin_write_secret(ADMIN_LOCAL, "local", "test-descendant", "test/descendant-tok/env/DESC_SECRET", "desc_secret_val")
    admin_write_secret(ADMIN_LOCAL, "local", "test-iso", "test/iso-tok/env/ISO_SECRET", "iso_secret_val")
    admin_write_secret(ADMIN_LOCAL, "local", "test-iso-home", "test/iso-home-tok/env/HOME_ISO_SECRET", "home_iso_val")
    admin_write_secret(ADMIN_LOCAL, "local", "test-runtime-dir", "test/runtime-tok/env/RUNTIME_SECRET", "runtime_val")
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

    kernel_release = machine.succeed("uname -r").strip()
    kernel_version = re.match(r"^(\d+)\.(\d+)", kernel_release)
    assert kernel_version is not None, f"could not parse running kernel release: {kernel_release!r}"
    assert tuple(map(int, kernel_version.groups())) >= (6, 5), (
        f"hardened peer-auth witness requires running Linux >= 6.5, got {kernel_release}"
    )

    # Scope the production-path witness to exactly this successful setgid
    # invocation. The cursor is intentionally the final operation before exec.
    cursor = journal_cursor()
    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo"))
    assert "MY_TOKEN=overwrite" in output, f"expected MY_TOKEN: {output}"

    journal_output = machine.succeed(
        "journalctl --no-pager "
        f"--after-cursor={shlex.quote(cursor)} "
        "-u underwrapd-local.service --output=json"
    )
    scoped_messages = [
        _journal_message(json.loads(line))
        for line in journal_output.splitlines()
        if line.strip()
    ]
    matching_reads = [
        message
        for message in scoped_messages
        if "wrapper read request" in message
        and re.search(r"(?:^|\s)wrapper=test-echo(?:\s|$)", message)
    ]
    assert len(matching_reads) == 1, (
        "expected exactly one post-cursor test-echo wrapper-read record, "
        f"got {matching_reads!r} from {scoped_messages!r}"
    )
    assert re.search(
        r"(?:^|[{,]\s*)uid:\s*1099(?=\s*[,}])", matching_reads[0]
    ), matching_reads[0]
    assert re.search(
        r"(?:^|[{,]\s*)auth_path:\s*PeerPidfd(?=\s*[,}])", matching_reads[0]
    ), (
        f"hardened wrapper read did not use SO_PEERPIDFD: {matching_reads[0]!r}"
    )
    fallback_records = [
        message
        for message in scoped_messages
        if "PeerCredFallback" in message
        or "wrapper peer auth fell back" in message
        or "SO_PEERPIDFD unavailable" in message
    ]
    assert not fallback_records, (
        f"hardened invocation emitted fallback path/warning records: {fallback_records!r}"
    )
    machine.log("PASS: unprivileged wrapper uses deployed hardened SO_PEERPIDFD auth path")

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

    # The per-user directory exposes bare names, not instance-prefixed aliases.
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
    # LOCAL INSTANCE: per-wrapper target-UID isolation
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

    # The direct target is namespace PID 2 under the trusted PID-1 relay and
    # runs as uid X; use host-ancestor SIGKILL for deterministic cleanup here.
    machine.execute("pkill -KILL -f '[s]leep 5252' || true")
    machine.execute("pkill -KILL -f iso-target || true")
    time.sleep(0.5)


def local_target_uid_idmapped_home():
    # ============================================================
    # LOCAL INSTANCE: idmapped directory for a dedicated-UID target
    # ============================================================
    # An isolated target runs as the dedicated uid X but, through an idmapped
    # `~/iso-data` mount that maps ONLY caller<->X, it reads a caller-owned file
    # as owner and its writeback lands on disk owned by the CALLER (not X).
    # Back iso-data with a tmpfs (owned by the caller): tmpfs fully supports
    # idmapped mounts and lives in init_user_ns, so the test is deterministic
    # regardless of what filesystem the VM's /home happens to be on. Production
    # homes (ext4/btrfs/xfs) support idmapped mounts too; this pins a
    # deterministic filesystem for the test.
    # mode=0700 (a realistic home perm) is deliberate: it makes the idmap-setup
    # path the same as a real 0700 home, where the caller cannot traverse the
    # mount once it is idmapped — the condition behind the deepthought submount
    # failure (see the idmappable-submount case below).
    machine.succeed("mkdir -p /home/testuser/iso-data")
    machine.execute("umount /home/testuser/iso-data 2>/dev/null || true")
    machine.succeed(
        "mount -t tmpfs -o mode=0700,uid=1099,gid=100 tmpfs /home/testuser/iso-data"
    )
    machine.succeed(
        "su testuser -s /bin/sh -c '"
        "printf %s caller-content-idmap > /home/testuser/iso-data/from-caller && "
        "rm -f /home/testuser/iso-data/from-target'"
    )
    # Runs synchronously: the target reads from-caller, writes from-target, exits.
    # A failure here (e.g. idmap read denied) propagates and fails the test.
    machine.succeed(
        "su testuser -s /bin/sh -c '/run/wrappers/bin/underwrap-local test-iso-home'"
    )
    out = machine.succeed("cat /home/testuser/iso-data/from-target").strip()
    assert out == "caller-content-idmap", (
        f"FAIL: idmapped target did not reproduce the caller file content "
        f"(got {out!r}) — the idmapped read did not work"
    )
    owner = machine.succeed("stat -c %U /home/testuser/iso-data/from-target").strip()
    assert owner == "testuser", (
        f"FAIL: idmapped writeback landed owned by {owner!r}, expected testuser — "
        f"the caller<->X write mapping is wrong"
    )
    machine.log(
        "PASS: dedicated-UID target read the caller's file through an idmapped directory as "
        "owner; its writeback is owned by the caller on disk"
    )

    # A non-idmappable nested mount is skipped without losing the parent idmap.
    # proc lacks FS_ALLOW_IDMAP and provides a portable stand-in for a FUSE
    # submount. The parent directory must remain idmapped.
    machine.succeed("mkdir -p /home/testuser/iso-data/portal")
    machine.succeed("mount -t proc proc /home/testuser/iso-data/portal")
    try:
        machine.succeed(
            "su testuser -s /bin/sh -c '"
            "printf %s caller-content-submount > /home/testuser/iso-data/from-caller && "
            "rm -f /home/testuser/iso-data/from-target'"
        )
        machine.succeed(
            "su testuser -s /bin/sh -c '/run/wrappers/bin/underwrap-local test-iso-home'"
        )
        out_sub = machine.succeed("cat /home/testuser/iso-data/from-target").strip()
        assert out_sub == "caller-content-submount", (
            f"FAIL: idmap of an accessPaths dir carrying a non-idmappable (proc) "
            f"submount did not work (got {out_sub!r}) — the parent dir's idmap "
            f"should still apply, with the submount skipped"
        )
    finally:
        machine.execute("umount /home/testuser/iso-data/portal 2>/dev/null || true")
    machine.log(
        "PASS: non-idmappable (proc) submount under an accessPaths dir was detached; "
        "the parent stayed idmapped and usable"
    )

    # An idmappable submount under a 0700 idmapped root must receive its own
    # idmap. A nested tmpfs (idmap-capable, separate superblock) holds a 0600
    # caller-owned file; the target can read it only when recursive idmapping
    # handles the submount.
    machine.succeed("mkdir -p /home/testuser/iso-data/sub")
    machine.succeed(
        "mount -t tmpfs -o mode=0700,uid=1099,gid=100 tmpfs /home/testuser/iso-data/sub"
    )
    try:
        machine.succeed(
            "su testuser -s /bin/sh -c '"
            "printf %s caller-content-idmap > /home/testuser/iso-data/from-caller && "
            "rm -f /home/testuser/iso-data/from-target && "
            "printf %s sub-secret-content > /home/testuser/iso-data/sub/from-caller-sub && "
            "chmod 0600 /home/testuser/iso-data/sub/from-caller-sub && "
            "rm -f /home/testuser/iso-data/sub/from-target-sub'"
        )
        machine.succeed(
            "su testuser -s /bin/sh -c '/run/wrappers/bin/underwrap-local test-iso-home'"
        )
        out_idsub = machine.succeed(
            "cat /home/testuser/iso-data/sub/from-target-sub"
        ).strip()
        assert out_idsub == "sub-secret-content", (
            f"FAIL: an idmappable submount under a 0700 idmapped root was not "
            f"idmapped (got {out_idsub!r}) — the uid-X target could not read the "
            f"caller's 0600 file through it"
        )
        owner = machine.succeed(
            "stat -c %U /home/testuser/iso-data/sub/from-target-sub"
        ).strip()
        assert owner == "testuser", (
            f"FAIL: submount writeback owned by {owner!r}, expected testuser"
        )
    finally:
        machine.execute("umount /home/testuser/iso-data/sub 2>/dev/null || true")
    machine.log(
        "PASS: recursive submount idmap — an idmappable submount under a 0700 "
        "idmapped root was itself idmapped; the target read the caller's 0600 "
        "file through it"
    )


def local_target_uid_runtime_dir():
    # ============================================================
    # LOCAL INSTANCE: an isolated target gets its OWN provisioned runtime dir
    # ============================================================
    # The dedicated target UID has no logind /run/user, so underwrap provisions
    # its 0700 runtime directory at <targetRuntimeDirBase>/<target-user-name>
    # and exports that as XDG_RUNTIME_DIR. It is X-owned, so the target writes it
    # directly — no idmap, and the caller's /run/user is never involved (the
    # caller here has no session dir, so this also proves the path is
    # session-independent). The dir is created at boot by systemd-tmpfiles.
    runtime_dir = machine.succeed(
        "su testuser -s /bin/sh -c '/run/wrappers/bin/underwrap-local test-runtime-dir'"
    ).strip()
    # The target printed its XDG_RUNTIME_DIR: it must be the provisioned per-
    # target-uid dir, NOT the caller's /run/user and NOT keyed on the caller uid.
    assert runtime_dir.startswith("/run/underwrap-test-runtime/underwrap-target-"), (
        f"FAIL: XDG_RUNTIME_DIR was {runtime_dir!r}, expected the provisioned "
        f"/run/underwrap-test-runtime/<target-user> dir"
    )
    assert "/run/user/" not in runtime_dir, (
        f"FAIL: XDG_RUNTIME_DIR {runtime_dir!r} points into the caller's session tree"
    )
    # The marker is owned by the dedicated target uid X (an underwrap-target-*
    # system user), proving the target wrote its OWN dir — not the caller's.
    owner = machine.succeed(
        f"stat -c %U {runtime_dir}/uw-runtime-probe/marker"
    ).strip()
    assert owner.startswith("underwrap-target-"), (
        f"FAIL: runtime-dir marker owned by {owner!r}, expected the dedicated "
        f"underwrap-target-* uid"
    )
    content = machine.succeed(
        f"cat {runtime_dir}/uw-runtime-probe/marker"
    ).strip()
    assert content == "runtime_dir_ok", f"FAIL: runtime-dir marker content {content!r}"
    machine.succeed(f"rm -rf {runtime_dir}/uw-runtime-probe")
    machine.log(
        "PASS: target used its own provisioned runtime directory "
        "(X-owned), independent of the caller's /run/user"
    )


def local_target_uid_files_libsecret():
    # ============================================================
    # LOCAL INSTANCE: files + libsecret projection under mandatory target UID
    # ============================================================
    # An isolated (dedicated-uid X) target can still project files and libsecret.
    # Files staging is created after the switch to X and collected through the
    # bounded files-only setfsuid(X) window. Libsecret uses host-validated socket
    # publication; the X-owned private bus tmpfs is never a writeback source.

    # --- Files slot under mandatory target UID ---
    set_local_unlocked_permission("test-iso-dir")
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
    # sleep 5353 is the isolated namespace-PID-2 direct target under the trusted
    # relay and runs as uid X; use host-ancestor SIGKILL for cleanup.
    machine.execute("pkill -KILL -f '[s]leep 5353' || true")
    machine.execute("pkill -u testuser -x underwrap-ns || true")
    time.sleep(0.5)
    machine.execute("umount /home/testuser/iso-files 2>/dev/null || true")

    # --- libsecret slot under mandatory target UID ---
    set_local_unlocked_permission("test-iso-libsecret")
    stage_text_secret_for_testuser("iso_libsecret_val", "/tmp/.iso-libsecret-secret")
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

def local_target_uid_authority():
    # ============================================================
    # LOCAL INSTANCE: root-owned target-UID authority
    # ============================================================
    # The dedicated sandbox uid is chosen by underwrap-ns from a ROOT-OWNED
    # (instance, wrapper) -> uid table; the ExecBundle carries no uid field. The
    # separate handoff test proves that these selectors are accepted only with a
    # fixed fd-20 socket created by the selected instance daemon.

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

    # Each row maps an `(instance, wrapper)` pair to a dedicated target
    # user/group ("<instance>\t<wrapper>\t<user>\t<group>"), never to root.
    # Pair-keying separates wrappers even when they share a target binary.
    contents = machine.succeed(f"cat {table}")
    iso_rows = [l for l in contents.splitlines() if l.strip()]
    assert iso_rows, f"FAIL: {table} is empty but wrappers exist"
    pair_to_user = {}
    for row in iso_rows:
        fields = row.split("\t")
        assert len(fields) == 4, f"FAIL: malformed authority row: {row!r}"
        instance, wrapper, user, group = fields
        assert instance and wrapper, f"FAIL: empty instance/wrapper in table: {row!r}"
        assert user.startswith("underwrap-target-"), (
            f"FAIL: authority row maps {instance}/{wrapper} to non-sandbox user {user!r}"
        )
        assert user != "root" and group != "root", (
            f"FAIL: authority row maps {instance}/{wrapper} to root: {row!r}"
        )
        pair_to_user[(instance, wrapper)] = user
    machine.log(
        f"PASS: target-uid table maps {len(iso_rows)} (instance,wrapper) pair(s) to "
        f"dedicated underwrap-target-* users (never root)"
    )

    # test-echo and test-locked in the local instance both target `env`, and
    # remote-echo does the same in another instance. Every pair must nevertheless
    # resolve to its own target user.
    same_binary_same_instance = (
        pair_to_user.get(("local", "test-echo")),
        pair_to_user.get(("local", "test-locked")),
    )
    assert all(same_binary_same_instance) and len(set(same_binary_same_instance)) == 2, (
        f"FAIL: two local wrappers sharing the 'env' binary must get distinct uids, "
        f"got {same_binary_same_instance}"
    )
    cross_instance = (
        pair_to_user.get(("local", "test-echo")),
        pair_to_user.get(("tunnel", "remote-echo")),
    )
    assert all(cross_instance) and len(set(cross_instance)) == 2, (
        f"FAIL: the same 'env' binary across two instances (local, tunnel) must get "
        f"distinct uids, got {cross_instance}"
    )
    # No sandbox user is shared by more than one pair.
    users = list(pair_to_user.values())
    assert len(users) == len(set(users)), (
        f"FAIL: a sandbox uid is shared across (instance,wrapper) pairs: {pair_to_user}"
    )
    machine.log(
        "PASS: distinct sandbox uids per (instance,wrapper) — same binary within an "
        "instance and across instances both resolve to different uids; no uid shared"
    )


def local_ns_handoff_authentication():
    # ============================================================
    # LOCAL INSTANCE: direct underwrap-ns handoff forgery is rejected
    # ============================================================
    # underwrap-ns is necessarily a system-wide capability wrapper. A caller
    # must not be able to pair forged selectors and fd-3 metadata with its own
    # socketpair, then execute arbitrary code as a configured target uid.
    marker = "/tmp/underwrap-direct-ns-attack-succeeded"
    machine.succeed(f"rm -f {marker}")
    command = (
        f"{PYTHON3} {DIRECT_NS_ATTACK_SCRIPT} "
        f"/run/wrappers/bin/underwrap-ns local test-echo {marker} 2>&1"
    )
    status, output = machine.execute(su(command))
    assert status != 0, (
        f"FAIL: direct underwrap-ns invocation accepted a forged handoff: {output}"
    )
    assert "does not match selected instance daemon" in output, (
        f"FAIL: direct underwrap-ns invocation did not fail at fd-20 peer auth: {output}"
    )
    machine.fail(f"test -e {marker}")
    machine.log("PASS: attacker-created fd-20 socketpair rejected before target execution")

    # The legitimate daemon-created socket transferred through underwrap-exec
    # must still authenticate and complete normally.
    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo"))
    assert "MY_TOKEN=" in output, f"legitimate handoff failed after attack: {output}"
    machine.log("PASS: daemon-created fd-20 handoff still succeeds")


def local_target_uid_readonly_writeback():
    # ============================================================
    # LOCAL INSTANCE: read-only permission writeback drop under mandatory target UID
    # ============================================================
    # Proves the read-only drop for a files target in the mandatory uid-X model:
    # the target writes a CHANGED value into its X-owned staging, the supervisor
    # reads the staging back through the single setfsuid(X) window, sees the
    # change, and DROPS it because the permission is read-only — the backend blob is
    # unchanged and the structured audit event fires. So the cross-uid read still
    # happens (exercising the setfsuid window); only the persist is refused.
    DIR = "test/iso-dir-tok"

    # Fresh caller-owned tmpfs for ~/iso-files (idmap-capable; see iso-home test).
    machine.execute("umount /home/testuser/iso-files 2>/dev/null || true")
    machine.succeed("mkdir -p /home/testuser/iso-files")
    machine.succeed(
        "mount -t tmpfs -o mode=0755,uid=1099,gid=100 tmpfs /home/testuser/iso-files"
    )

    # Seed a known baseline via a WRITABLE write, then flip the permission to read-only.
    set_local_unlocked_permission("test-iso-dir")  # writable=True
    machine.succeed(
        "su testuser -s /bin/sh -c '/run/wrappers/bin/underwrap-local test-iso-dir write'"
    )
    baseline = admin_get_blob(ADMIN_LOCAL, "local", DIR)

    admin_upsert_permission(
        ADMIN_LOCAL, "local", LOCAL_CLIENT_INSTANCE, "test-iso-dir", DIR,
        {"kind": "unlocked"}, writable=False,
    )

    # Snapshot the journal cursor so we can scope the audit-event grep.
    cursor = journal_cursor()

    # `rewrite` writes a DISTINCT value, so the post-run blob differs from the
    # baseline — the change the read-only permission must drop.
    stderr_capture = machine.succeed(
        su(
            "/run/wrappers/bin/underwrap-local test-iso-dir rewrite 2>/tmp/iso-ro-stderr.log "
            "&& cat /tmp/iso-ro-stderr.log"
        )
    )
    time.sleep(0.5)

    after = admin_get_blob(ADMIN_LOCAL, "local", DIR)
    assert after == baseline, (
        "FAIL: isolated read-only permission persisted a change — the backend blob changed"
    )

    journal_after = machine.succeed(
        f"journalctl --no-pager --after-cursor={shlex.quote(cursor)} "
        "SYSLOG_IDENTIFIER=underwrap-ns --output=export || true"
    )
    assert "AUDIT=writeback_rejected" in journal_after, (
        f"FAIL: isolated read-only drop missing writeback_rejected event: {journal_after!r}"
    )
    assert "REASON=permission_readonly" in journal_after, (
        f"FAIL: isolated read-only drop missing permission_readonly reason: {journal_after!r}"
    )
    assert "REASON=grant_readonly" not in journal_after, (
        f"FAIL: isolated read-only drop retained legacy reason: {journal_after!r}"
    )
    assert f"RESOURCE_NAME={DIR}" in journal_after, (
        f"FAIL: isolated read-only drop journal event missing resource name: {journal_after!r}"
    )
    assert "permission is read-only" in stderr_capture, (
        f"FAIL: isolated read-only drop missing human-readable stderr warning: {stderr_capture!r}"
    )
    assert "grant is read-only" not in stderr_capture, (
        f"FAIL: isolated read-only drop retained legacy warning: {stderr_capture!r}"
    )
    assert f"resource '{DIR}'" in stderr_capture, (
        f"FAIL: isolated read-only drop stderr warning missing resource name: {stderr_capture!r}"
    )
    machine.log(
        "PASS: read-only permission drops the change after reading the X-owned staging "
        "through the setfsuid(X) window; journal records permission_readonly"
    )

    machine.execute("umount /home/testuser/iso-files 2>/dev/null || true")


def local_ancestor_attack():
    # ============================================================
    # LOCAL INSTANCE: adversarial ancestor attack (CRITICAL TEST)
    # ============================================================
    # A PARENT process spawns a wrapper (test-iso) and tries to inspect the
    # dedicated-UID target subtree it spawned. The UID boundary makes
    # `ptrace_may_access` fail for both the READ-mode files
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

    # Clean up as root: the namespace-PID-2 target subtree runs as uid X, so a
    # testuser pkill cannot reach it.
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

    # A fresh /proc exposes only the trusted PID-1 relay, configured PID-2
    # target, and transient namespace-local children — never host PIDs.
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
    # PIDs (PID 1 = trusted relay, PID 2 = target, plus private helpers).
    # Definitely NOT hundreds of host PIDs.
    assert len(visible_pids) < 20, \
        f"FAIL: too many PIDs visible ({len(visible_pids)}) — /proc may show host PIDs: {visible_pids}"
    # Trusted relay PID 1 must be visible.
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
    stage_text_secret_for_testuser("my_gh_token_value", "/tmp/.secretsd-test")
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

    # Prove the deployed private filesystem bus is absent from the host mount
    # namespace while the target has just completed a real lookup and remains
    # blocked before a second lookup. Trusted root instrumentation checks host
    # absence; a separate same-caller process checks the mode-0000 boundary.
    # A uniquely named transient unit supplies exact cgroup-scoped liveness and
    # cleanup.
    set_local_unlocked_permission("test-libsecret-bus-probe")
    probe_id = uuid.uuid4().hex
    probe_unit = f"underwrap-libsecret-bus-probe-{probe_id}.service"
    release_marker = f"/tmp/underwrap-libsecret-bus-release-{probe_id}"
    quoted_unit = shlex.quote(probe_unit)
    quoted_marker = shlex.quote(release_marker)

    def as_probe_user(command):
        return f"su testuser -s /bin/sh -c {shlex.quote(command)}"

    def probe_unit_properties():
        output = machine.succeed(
            f"systemctl show {quoted_unit} --no-pager "
            "--property=ActiveState --property=SubState --property=Result "
            "--property=ExecMainStatus"
        )
        return dict(line.split("=", 1) for line in output.splitlines() if "=" in line)

    def probe_journal():
        return machine.succeed(
            f"journalctl --quiet --unit={quoted_unit} --output=cat --no-pager"
        )

    def wait_for_probe(predicate, description):
        deadline = time.monotonic() + 30
        journal = ""
        while time.monotonic() < deadline:
            journal = probe_journal()
            if predicate(journal):
                return journal
            properties = probe_unit_properties()
            if properties.get("ActiveState") in {"failed", "inactive"}:
                raise AssertionError(
                    f"libsecret bus probe became {properties} before {description}: "
                    f"{journal!r}"
                )
            time.sleep(0.1)
        raise AssertionError(
            f"timed out waiting for libsecret bus probe {description}: {journal!r}"
        )

    def assert_probe_running(when):
        properties = probe_unit_properties()
        assert properties.get("ActiveState") == "active", (
            f"libsecret bus probe unit not active {when}: {properties}"
        )
        assert properties.get("SubState") == "running", (
            f"libsecret bus probe unit not running {when}: {properties}"
        )

    ready_pattern = re.compile(
        r"^UNDERWRAP_LIBSECRET_BUS_READY DBUS_SESSION_BUS_ADDRESS="
        r"(unix:path=[^,\s]+(?:,guid=[0-9A-Fa-f]+)?)$"
    )
    still_ready = "UNDERWRAP_LIBSECRET_BUS_STILL_READY"
    machine.succeed(f"test ! -e {quoted_marker}")
    try:
        machine.succeed(
            "systemd-run --quiet "
            f"--unit={quoted_unit} --collect --uid=testuser "
            "--property=Type=simple --property=RemainAfterExit=yes "
            "/run/wrappers/bin/underwrap-local test-libsecret-bus-probe "
            f"{quoted_marker}"
        )
        journal = wait_for_probe(
            lambda text: any(
                ready_pattern.fullmatch(line) for line in text.splitlines()
            ),
            "READY marker",
        )
        ready_addresses = [
            match.group(1)
            for line in journal.splitlines()
            if (match := ready_pattern.fullmatch(line)) is not None
        ]
        assert len(ready_addresses) == 1, (
            f"expected exactly one private bus address, got {ready_addresses!r}"
        )
        address = ready_addresses[0]
        address_without_guid = address.split(",guid=", 1)[0]
        bus_path = address_without_guid.removeprefix("unix:path=")
        assert address_without_guid == f"unix:path={bus_path}", (
            f"unexpected private bus address shape: {address!r}"
        )
        assert bus_path == "/tmp/.underwrap-ns-bus/bus", (
            f"unexpected private bus path: {bus_path!r}"
        )

        assert_probe_running("immediately before host isolation probes")

        exact_path_status = machine.execute(
            f"test ! -e {shlex.quote(bus_path)}"
        )[0]
        assert exact_path_status == 0, (
            f"trusted host probe sees private bus path {bus_path!r}"
        )

        host_bus_tree = machine.succeed(
            "find /tmp/.underwrap-ns-bus -type s -print"
        ).strip()
        assert host_bus_tree == "", (
            f"trusted host probe sees a private bus socket: {host_bus_tree}"
        )

        caller_probe_source = (
            "import errno\n"
            "import os\n"
            "import socket\n"
            f"path = {bus_path!r}\n"
            "try:\n"
            "    os.stat(path)\n"
            "except OSError as error:\n"
            "    stat_errno = error.errno\n"
            "else:\n"
            "    stat_errno = 0\n"
            "with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:\n"
            "    connect_errno = client.connect_ex(path)\n"
            "assert stat_errno == errno.EACCES, stat_errno\n"
            "assert connect_errno == errno.EACCES, connect_errno\n"
            "print(stat_errno, connect_errno)\n"
        )
        caller_probe_command = (
            f"{shlex.quote(PYTHON3)} -c {shlex.quote(caller_probe_source)}"
        )
        caller_probe_result = machine.succeed(
            as_probe_user(caller_probe_command)
        ).strip()
        expected_denials = f"{errno.EACCES} {errno.EACCES}"
        assert caller_probe_result == expected_denials, (
            f"private bus stat/connect probe returned {caller_probe_result!r}, "
            f"expected {expected_denials!r}"
        )

        assert_probe_running("immediately after host isolation probes")
        machine.log(
            "PASS: trusted host probe sees no live private bus path or socket; "
            "same-caller stat and connect are denied with EACCES"
        )

        machine.succeed(as_probe_user(f"touch {quoted_marker}"))
        journal = wait_for_probe(
            lambda text: still_ready in text.splitlines(),
            "STILL_READY marker after the second lookup",
        )
        assert journal.splitlines().count(still_ready) == 1, (
            "expected exactly one STILL_READY marker after the second lookup"
        )

        deadline = time.monotonic() + 30
        properties = probe_unit_properties()
        while (
            properties.get("ActiveState") != "active"
            or properties.get("SubState") != "exited"
        ):
            if time.monotonic() >= deadline:
                raise AssertionError(
                    f"libsecret bus probe did not exit cleanly: {properties}"
                )
            if properties.get("ActiveState") == "failed":
                raise AssertionError(f"libsecret bus probe failed: {properties}")
            time.sleep(0.1)
            properties = probe_unit_properties()
        assert properties.get("Result") == "success", properties
        assert properties.get("ExecMainStatus") == "0", properties
        machine.log(
            "PASS: live private-bus probe completed its second lookup and exited cleanly"
        )
    finally:
        machine.execute(
            f"systemctl stop {quoted_unit} >/dev/null 2>&1 || true"
        )
        machine.execute(
            f"systemctl reset-failed {quoted_unit} >/dev/null 2>&1 || true"
        )
        machine.execute(f"rm -f -- {quoted_marker}")

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

    stage_text_secret_for_testuser(
        "libsecret_two_slot_value", "/tmp/.two-slot-secret"
    )

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

    # New permission model: one Unlocked permission per `(client_instance,
    # wrapper, resource)` tuple. Read access is then unconditional;
    # wrapper-side writes are gated by the per-permission `writable: bool`
    # flag (which `set_local_unlocked_permission` sets true).
    for wrapper in [
        "test-dir-write",
        "test-dir-read",
        "test-dir-readonly-write",
        "test-dir-sleep",
        "test-dir-symlink",
    ]:
        set_local_unlocked_permission(wrapper)

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

    # Seed the per-permission `writable=true` for the wrappers used by
    # the atomic-writeback scenarios. They share the dir-a resource.
    for wrapper in [
        "test-dir-write-then-sleep",
        "test-dir-touch",
        "test-dir-env-probe",
        "test-dir-env-override-probe",
    ]:
        set_local_unlocked_permission(wrapper)

def journal_cursor():
    """Snapshot the current journal cursor for exact follow-up queries."""
    out = machine.succeed("journalctl -n 1 -o export --no-pager")
    for line in out.splitlines():
        if line.startswith("__CURSOR="):
            return line[len("__CURSOR=") :]
    raise AssertionError(f"journalctl produced no cursor: {out!r}")


TERMINAL_RESOURCE_AUDITS = ("resource_checkin", "resource_usage_signal")
ANSI_ESCAPE_RE = re.compile(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|[@-Z\\-_])")


def _journal_message(record):
    """Decode journald's JSON string or byte-array MESSAGE representation."""
    value = record.get("MESSAGE", "")
    if isinstance(value, str):
        message = value
    elif isinstance(value, list) and all(
        isinstance(byte, int) and 0 <= byte <= 255 for byte in value
    ):
        message = bytes(value).decode("utf-8", errors="replace")
    else:
        raise AssertionError(f"unexpected journal MESSAGE representation: {value!r}")
    return ANSI_ESCAPE_RE.sub("", message)


def underwrapd_terminal_records_since(cursor, wrapper, resource_id):
    """Return complete records whose parsed terminal audit fields match exactly."""
    output = machine.succeed(
        "journalctl --no-pager "
        f"--after-cursor={shlex.quote(cursor)} "
        "-u underwrapd-local.service --output=json"
    )
    records = []
    for line in output.splitlines():
        record = json.loads(line)
        message = _journal_message(record)
        fields = dict(re.findall(r'(?:^|\s)([a-z_]+)="([^"]*)"', message))
        if (
            fields.get("wrapper") == wrapper
            and fields.get("resource") == resource_id
            and fields.get("audit") in TERMINAL_RESOURCE_AUDITS
        ):
            records.append(
                {
                    "journal_record": record,
                    "message": message,
                    "parsed_fields": fields,
                }
            )
    return records


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
        files via the writable permission; we then re-fetch the blob bytes
        so subsequent scenarios can byte-compare against this baseline."""
        set_local_unlocked_permission("test-dir-write")
        machine.succeed(su("/run/wrappers/bin/underwrap-local test-dir-write seed"))
        return admin_get_blob(ADMIN_LOCAL, "local", DIR_A)

    # ----------------------------------------------------------
    # Scenario 1: clean run with no changes
    # Wrapper reads the bound files but never writes. Backend blob
    # bytes are byte-identical after the run. Journal contains no
    # writeback_rejected event. `last_used_at` is bumped.
    # ----------------------------------------------------------
    baseline_a = reset_blob_state()
    set_local_unlocked_permission("test-dir-read")

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
    # Scenario 2: changed run with writable permission — Checkin path
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
    # Scenario 3: changed run with read-only permission — quiet drop
    # The `test-dir-readonly-write` wrapper writes mutations through a
    # permission with `writable=false`. The daemon emits no Checkin (ns
    # has writable=false from Checkout), the backend blob is
    # unchanged, the journal carries the structured
    # `AUDIT=writeback_rejected REASON=permission_readonly` event (routed
    # via journald's native socket so it does not clutter the
    # wrapper's stderr), and stderr contains only the human-readable
    # line — which names both the resource and its id.
    # ----------------------------------------------------------
    baseline_a = reset_blob_state()
    admin_upsert_permission(
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
    assert after_a == baseline_a, "scenario 3: dir-a blob changed despite read-only permission"

    # The structured audit event is delivered via journald's native
    # socket; the human-readable warning goes to wrapper stderr. The
    # warning identifies the resource by name and id.
    assert '"audit":"writeback_rejected"' not in stderr_capture, (
        f"scenario 3: audit JSON leaked onto stderr instead of journald: {stderr_capture!r}"
    )
    assert "permission is read-only" in stderr_capture, (
        f"scenario 3: missing human-readable stderr warning: {stderr_capture!r}"
    )
    assert "grant is read-only" not in stderr_capture, (
        f"scenario 3: legacy human-readable warning remained: {stderr_capture!r}"
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
    assert "REASON=permission_readonly" in journal_after, (
        f"scenario 3: missing permission_readonly reason in journal: {journal_after!r}"
    )
    assert "REASON=grant_readonly" not in journal_after, (
        f"scenario 3: legacy reason remained in journal: {journal_after!r}"
    )
    assert f"RESOURCE_NAME={DIR_A}" in journal_after, (
        f"scenario 3: journal event missing resource name: {journal_after!r}"
    )
    machine.log(
        "PASS: scenario 3 — read-only permission drops mutations; journal records audit event, stderr shows named warning"
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
    admin_upsert_permission(
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
    assert "permission is read-only" not in stderr_capture, (
        f"scenario 4: human-readable stderr line leaked despite suppression env: {stderr_capture!r}"
    )
    assert "grant is read-only" not in stderr_capture, (
        f"scenario 4: legacy stderr warning appeared under suppression: {stderr_capture!r}"
    )

    journal_after = machine.succeed(
        f"journalctl --no-pager --after-cursor={shlex.quote(cursor)} "
        "SYSLOG_IDENTIFIER=underwrap-ns --output=export || true"
    )
    assert "AUDIT=writeback_rejected" in journal_after, (
        f"scenario 4: journal missing audit event under suppression: {journal_after!r}"
    )
    assert "REASON=permission_readonly" in journal_after, (
        f"scenario 4: journal missing permission_readonly reason: {journal_after!r}"
    )
    assert "REASON=grant_readonly" not in journal_after, (
        f"scenario 4: journal retained legacy reason: {journal_after!r}"
    )

    # Env clear + forced HOME/XDG: the target must not see
    # UNDERWRAP_SUPPRESS_READONLY_WARNING (or any other UNDERWRAP_*)
    # propagated from the wrapper invocation environment, and caller-
    # supplied HOME/XDG values must be replaced with the trusted
    # underwrap defaults.
    set_local_unlocked_permission("test-dir-env-probe")
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
    # XDG_RUNTIME_DIR is the target's OWN provisioned dir
    # (<base>/<target-user-name>), never the caller-supplied value.
    assert (
        "XDG_RUNTIME_DIR=/run/underwrap-test-runtime/underwrap-target-" in target_env
    ), target_env
    assert "XDG_CONFIG_HOME=/home/testuser/.config" in target_env, target_env
    assert "XDG_DATA_HOME=/home/testuser/.local/share" in target_env, target_env
    assert "XDG_CACHE_HOME=/home/testuser/.cache" in target_env, target_env
    assert "XDG_STATE_HOME=/home/testuser/.local/state" in target_env, target_env

    set_local_unlocked_permission("test-dir-env-override-probe")
    override_env = machine.succeed(
        "su testuser -s /bin/sh -c '"
        "HOME=/tmp/evil-home "
        "XDG_RUNTIME_DIR=/tmp/evil-run "
        "XDG_CONFIG_HOME=/tmp/evil-config "
        "/run/wrappers/bin/underwrap-local test-dir-env-override-probe'"
    )
    assert "HOME=/tmp/underwrap-home-override" in override_env, override_env
    assert (
        "XDG_RUNTIME_DIR=/run/underwrap-test-runtime/underwrap-target-" in override_env
    ), override_env
    assert "XDG_CONFIG_HOME=/tmp/underwrap-config-override" in override_env, override_env
    assert "XDG_DATA_HOME=/tmp/underwrap-home-override/.local/share" in override_env, override_env
    assert "XDG_CACHE_HOME=/tmp/underwrap-home-override/.cache" in override_env, override_env
    assert "XDG_STATE_HOME=/tmp/underwrap-home-override/.local/state" in override_env, override_env
    machine.log(
        "PASS: scenario 4 — suppression stays private, caller HOME/XDG spoofing is ignored, and envSet overrides apply cleanly"
    )

    # ----------------------------------------------------------
    # Scenario 5: always-writable namespace mount
    # Even with a read-only permission, the in-namespace bind mount is RW
    # to the target. A `touch` inside the bound directory must exit 0
    # (the kernel allowed the write); the file does not persist to
    # the backend (covered by scenario 3's blob check, repeated here).
    # ----------------------------------------------------------
    baseline_a = reset_blob_state()
    admin_upsert_permission(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        "test-dir-touch",
        DIR_A,
        {"kind": "unlocked"},
        writable=False,
    )
    # touch on the bound directory under a read-only permission — must
    # succeed (target sees a writable mount).
    machine.succeed(su("/run/wrappers/bin/underwrap-local test-dir-touch"))
    after_a = admin_get_blob(ADMIN_LOCAL, "local", DIR_A)
    assert after_a == baseline_a, (
        "scenario 5: read-only permission touch persisted to backend"
    )
    machine.log("PASS: scenario 5 — read-only permission accepts target touch, drops it at writeback")

    # ----------------------------------------------------------
    # Scenario 6: server-side reject on mid-run permission flip
    # Wrapper Checkouts a writable permission, then the test driver flips
    # the permission to writable=false while the target sleeps. On exit,
    # ns dispatches Checkin against the now-stale permission; the daemon
    # re-evaluates the policy and returns Unauthorized. The journal
    # carries `writeback_rejected reason=server_rejected` and the
    # backend blob is unchanged.
    # ----------------------------------------------------------
    baseline_a = reset_blob_state()
    admin_upsert_permission(
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
    # The script sleeps for ~4s after writing; we flip the permission
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

    # Flip the permission to read-only mid-run.
    admin_upsert_permission(
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
        f"scenario 6: missing audit event in journal after permission flip: {journal_after!r}"
    )
    assert "REASON=server_rejected" in journal_after, (
        f"scenario 6: missing server_rejected reason in journal: {journal_after!r}"
    )

    stderr_capture = machine.succeed("cat /tmp/wts-stderr.log || true")
    assert '"audit":"writeback_rejected"' not in stderr_capture, (
        f"scenario 6: audit JSON leaked to stderr instead of journald: {stderr_capture!r}"
    )
    assert "permission is read-only" in stderr_capture, (
        f"scenario 6: missing human-readable stderr warning: {stderr_capture!r}"
    )
    assert "grant is read-only" not in stderr_capture, (
        f"scenario 6: legacy stderr warning remained: {stderr_capture!r}"
    )
    assert f"resource '{DIR_A}'" in stderr_capture, (
        f"scenario 6: stderr warning missing resource name: {stderr_capture!r}"
    )
    machine.log(
        "PASS: scenario 6 — server rejects Checkin after mid-run permission flip; journal records server_rejected"
    )


def files_staging_provenance():
    """Distinct-UID shipped chain ignores preclaimed/replaced legacy staging names."""
    wrapper = "test-staging-provenance"
    resource = LOCAL_WRAPPER_RESOURCES[wrapper]
    key = ".config/staging-provenance/state"
    legacy = "/tmp/.underwrap-ns-directories"
    machine.succeed("install -d -m 0700 -o testuser -g users /home/testuser/.config")
    target_user = machine.succeed(
        "awk -F '\\t' '$1 == \"local\" && $2 == \"test-staging-provenance\" {print $3}' /etc/underwrap/target-uids"
    ).strip()
    target_uid = int(machine.succeed(f"id -u {shlex.quote(target_user)}").strip())
    runtime_attachment = f"/run/underwrap-test-runtime/{target_user}/.underwrap-files"
    assert target_uid not in {0, 1099}, "target identity is not distinct"

    def as_caller(command):
        return f"runuser -u testuser -- sh -c {shlex.quote(command)}"

    def properties(unit):
        output = machine.succeed(
            f"systemctl show {shlex.quote(unit)} --no-pager "
            "--property=ActiveState --property=SubState --property=Result --property=ExecMainStatus"
        )
        return dict(line.split("=", 1) for line in output.splitlines() if "=" in line)

    def journal(unit):
        return machine.succeed(f"journalctl --quiet -u {shlex.quote(unit)} --output=cat --no-pager")

    def wait_ready(unit):
        expected = f"STAGING_PROVENANCE_READY uid={target_uid} fs=tmpfs noswap=1"
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            output = journal(unit)
            if expected in output.splitlines():
                assert output.splitlines().count(expected) == 1
                assert properties(unit).get("SubState") == "running"
                return
            state = properties(unit)
            assert state.get("ActiveState") not in {"failed", "inactive"}, (
                f"staging target failed before READY: {state}; {output}"
            )
            time.sleep(0.05)
        raise AssertionError("staging target did not reach READY")

    # Changed state proves collection/persistence, rather than merely a target
    # success whose diagnostic-only finalization was silently suppressed.
    for case, mode, writable in [
        ("writable", "change", True),
        ("unchanged", "unchanged", True),
        ("readonly", "change", False),
        ("protected-root", "change", True),
    ]:
        protected = case == "protected-root"
        probe = uuid.uuid4().hex
        unit = f"underwrap-staging-provenance-{probe}.service"
        release = f"/tmp/staging-provenance-release-{probe}"
        before_path = f"/tmp/staging-provenance-before-{probe}"
        after_path = f"/tmp/staging-provenance-after-{probe}"
        set_local_unlocked_permission(wrapper)
        admin_upsert_permission(
            ADMIN_LOCAL, "local", LOCAL_CLIENT_INSTANCE, wrapper, resource,
            {"kind": "unlocked"}, writable=writable,
        )
        admin_write_resource_value(ADMIN_LOCAL, "local", resource, "files", key, b"provenance-original")
        baseline = admin_get_blob(ADMIN_LOCAL, "local", resource)
        expected = baseline
        if mode == "change" and writable:
            value = json.loads(base64.b64decode(baseline))
            value["files"][key] = base64.b64encode(b"provenance-target-result").decode()
            expected = base64.b64encode(json.dumps(
                value, sort_keys=True, separators=(",", ":"), ensure_ascii=False,
            ).encode()).decode()
        machine.succeed(f"test ! -e {shlex.quote(legacy)}")
        try:
            if protected:
                machine.succeed(f"install -d -m 0000 -o root -g root {shlex.quote(legacy)}")
                machine.fail(as_caller(f"mv {shlex.quote(legacy)} {shlex.quote(before_path)}"))
            else:
                # All preclaim and replacement effects are performed by U, not root.
                machine.succeed(as_caller(
                    f"mkdir {shlex.quote(legacy)}; mv {shlex.quote(legacy)} {shlex.quote(before_path)}; "
                    f"mkdir -p {shlex.quote(legacy)}/mount-0; "
                    f"printf 'caller-decoy' > {shlex.quote(legacy)}/mount-0/state; "
                    f"chmod 0755 {shlex.quote(legacy)} {shlex.quote(legacy)}/mount-0; "
                    f"chmod 0644 {shlex.quote(legacy)}/mount-0/state"
                ))
                assert machine.succeed(f"stat -c %u {shlex.quote(legacy)}").strip() == "1099"
            cursor = journal_cursor()
            machine.succeed(
                f"systemd-run --quiet --unit={shlex.quote(unit)} --collect --uid=testuser "
                "--property=Type=simple --property=RemainAfterExit=yes "
                f"/run/wrappers/bin/underwrap-local {wrapper} {shlex.quote(release)} {mode}"
            )
            wait_ready(unit)
            # Root instrumentation sees only the bare mode-0000 attachment,
            # while the target just observed its mode-0700 noswap tmpfs there.
            quoted_attachment = shlex.quote(runtime_attachment)
            assert machine.succeed(f"stat -c '%u %a' {quoted_attachment}").strip() == f"{target_uid} 0"
            machine.succeed(f"test -z \"$(find {quoted_attachment} -mindepth 1 -print -quit)\"")
            machine.fail(as_caller(f"mv {quoted_attachment} {quoted_attachment}.replaced"))
            if protected:
                machine.fail(as_caller(f"mv {shlex.quote(legacy)} {shlex.quote(after_path)}"))
            else:
                old_inode = machine.succeed(f"stat -c %i {shlex.quote(legacy)}").strip()
                machine.succeed(as_caller(
                    f"mv {shlex.quote(legacy)} {shlex.quote(after_path)}; "
                    f"mkdir -p {shlex.quote(legacy)}/mount-0; "
                    f"printf 'caller-replacement' > {shlex.quote(legacy)}/mount-0/state; "
                    f"printf 'caller-only' > {shlex.quote(legacy)}/mount-0/injected; "
                    f"chmod 0755 {shlex.quote(legacy)} {shlex.quote(legacy)}/mount-0; "
                    f"chmod 0644 {shlex.quote(legacy)}/mount-0/state {shlex.quote(legacy)}/mount-0/injected"
                ))
                assert machine.succeed(f"stat -c %i {shlex.quote(legacy)}").strip() != old_inode
            assert properties(unit).get("SubState") == "running"
            machine.succeed(as_caller(f"touch {shlex.quote(release)}"))
            deadline = time.monotonic() + 30
            while properties(unit).get("SubState") != "exited":
                state = properties(unit)
                assert state.get("ActiveState") != "failed", f"staging target failed: {state}; {journal(unit)}"
                assert time.monotonic() < deadline, "staging invocation did not finish"
                time.sleep(0.05)
            state = properties(unit)
            assert state.get("Result") == "success" and state.get("ExecMainStatus") == "0", state
            assert journal(unit).splitlines().count("STAGING_PROVENANCE_DONE") == 1
            assert admin_get_blob(ADMIN_LOCAL, "local", resource) == expected, (
                f"{case}: collected state differs from the exact target-authored state"
            )
            expected_audit = "resource_checkin" if writable and mode == "change" else "resource_usage_signal"
            deadline = time.monotonic() + 2
            while True:
                records = underwrapd_terminal_records_since(cursor, wrapper, LOCAL_RESOURCE_IDS[resource])
                if records or time.monotonic() >= deadline:
                    break
                time.sleep(0.05)
            assert [record["parsed_fields"].get("audit") for record in records] == [expected_audit], (
                f"{case}: missing or unexpected terminal dispatch"
            )
            if not protected:
                machine.succeed(f"test $(stat -c %u {shlex.quote(legacy)}/mount-0/state) = 1099")
                machine.succeed(f"test $(stat -c %u {shlex.quote(after_path)}/mount-0/state) = 1099")
            machine.log(f"PASS: pinned staging provenance {case}, distinct UID, tmpfs/noswap and exact terminal state")
        finally:
            machine.execute(f"systemctl stop {shlex.quote(unit)}")
            machine.execute(f"systemctl reset-failed {shlex.quote(unit)}")
            machine.succeed("rm -rf -- " + " ".join(shlex.quote(path) for path in (legacy, release, before_path, after_path)))


def pre_exec_writeback_gate():
    """Exercise pre-exec suppression and retained post-exec finalization."""

    machine.succeed("install -d -m 0700 -o testuser -g users /home/testuser/.config")

    def seed(resource, entries):
        for key, value in entries.items():
            admin_write_resource_value(
                ADMIN_LOCAL, "local", resource, "files", key, value
            )
        return admin_get_blob(ADMIN_LOCAL, "local", resource)

    def decoded(blob_b64):
        return json.loads(base64.b64decode(blob_b64))

    def expected_with_file(blob_b64, key, value):
        value_blob = decoded(blob_b64)
        value_blob["files"][key] = base64.b64encode(value).decode()
        canonical = json.dumps(
            value_blob,
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=False,
        ).encode()
        return base64.b64encode(canonical).decode()

    def assert_no_terminal(cursor, wrapper, resource):
        machine.sleep(0.2)
        records = underwrapd_terminal_records_since(
            cursor, wrapper, LOCAL_RESOURCE_IDS[resource]
        )
        assert records == [], (
            f"{wrapper}: unexpected post-cursor {TERMINAL_RESOURCE_AUDITS!r} "
            f"audit records: {records}"
        )

    def assert_single_terminal(cursor, wrapper, resource, expected_audit):
        assert expected_audit in TERMINAL_RESOURCE_AUDITS, expected_audit
        resource_id = LOCAL_RESOURCE_IDS[resource]
        deadline = time.monotonic() + 2.0
        while True:
            records = underwrapd_terminal_records_since(cursor, wrapper, resource_id)
            if records or time.monotonic() >= deadline:
                break
            time.sleep(0.05)
        parsed = [
            {
                "audit": record["parsed_fields"].get("audit"),
                "wrapper": record["parsed_fields"].get("wrapper"),
                "resource": record["parsed_fields"].get("resource"),
            }
            for record in records
        ]
        assert parsed == [
            {
                "audit": expected_audit,
                "wrapper": wrapper,
                "resource": resource_id,
            }
        ], records

    unchanged_resource = "test/dir-a"
    unchanged_wrapper = "test-dir-read"
    set_local_unlocked_permission(unchanged_wrapper)
    before = admin_get_blob(ADMIN_LOCAL, "local", unchanged_resource)
    cursor = journal_cursor()
    status, output = machine.execute(
        su(f"/run/wrappers/bin/underwrap-local {unchanged_wrapper}") + " 2>&1"
    )
    assert status == 0, f"unchanged target returned {status}: {output}"
    after = admin_get_blob(ADMIN_LOCAL, "local", unchanged_resource)
    assert after == before, "unchanged target changed canonical blob bytes"
    assert_single_terminal(
        cursor,
        unchanged_wrapper,
        unchanged_resource,
        "resource_usage_signal",
    )
    machine.log(
        "PASS: exec-gate unchanged successful target emits one exact UsageSignal audit record"
    )

    unroutable_resource = "test/gate-unroutable"
    unroutable_wrapper = "test-gate-unroutable"
    set_local_unlocked_permission(unroutable_wrapper)
    machine.succeed("rm -f /tmp/gate-unroutable-target-ran")
    before = seed(
        unroutable_resource,
        {
            ".config/gate-unroutable/a-preserved": b"preserved-value",
            ".config/z-gate-unroutable/missing": b"later-unroutable-value",
        },
    )
    before_value = decoded(before)
    assert base64.b64decode(
        before_value["files"][".config/gate-unroutable/a-preserved"]
    ) == b"preserved-value"
    assert base64.b64decode(
        before_value["files"][".config/z-gate-unroutable/missing"]
    ) == b"later-unroutable-value"
    cursor = journal_cursor()
    status, output = machine.execute(
        su(f"/run/wrappers/bin/underwrap-local {unroutable_wrapper}") + " 2>&1"
    )
    assert status != 0, "later-unroutable projection unexpectedly executed"
    assert "no configured mount path is a prefix" in output, output
    machine.fail("test -e /tmp/gate-unroutable-target-ran")
    after = admin_get_blob(ADMIN_LOCAL, "local", unroutable_resource)
    assert after == before, "later-unroutable projection changed canonical blob bytes"
    after_value = decoded(after)
    assert after_value["files"] == before_value["files"]
    assert_no_terminal(cursor, unroutable_wrapper, unroutable_resource)
    machine.log(
        "PASS: exec-gate later-unroutable projection is prevalidated with no target or terminal verb"
    )

    missing_resource = "test/gate-exec-missing"
    missing_wrapper = "test-gate-exec-missing"
    set_local_unlocked_permission(missing_wrapper)
    before = seed(
        missing_resource,
        {".config/gate-exec-missing/token": b"exec-missing-original"},
    )
    cursor = journal_cursor()
    status, output = machine.execute(
        su(f"/run/wrappers/bin/underwrap-local {missing_wrapper}") + " 2>&1"
    )
    assert status == 1, f"missing target returned {status}: {output}"
    assert output.count("underwrap-ns: exec failed:") == 1, output
    assert "No such file or directory" in output, output
    after = admin_get_blob(ADMIN_LOCAL, "local", missing_resource)
    assert after == before, "returned exec error changed canonical blob bytes"
    assert decoded(after)["files"] == decoded(before)["files"]
    assert_no_terminal(cursor, missing_wrapper, missing_resource)
    machine.log(
        "PASS: exec-gate returned exec error reports failure with no collection or terminal verb"
    )

    exit_resource = "test/gate-exit-37"
    exit_wrapper = "test-gate-exit-37"
    set_local_unlocked_permission(exit_wrapper)
    before = seed(
        exit_resource,
        {".config/gate-exit-37/token": b"before-exit-37"},
    )
    expected = expected_with_file(
        before,
        ".config/gate-exit-37/token",
        b"persisted-after-exit-37",
    )
    cursor = journal_cursor()
    program = (
        "import subprocess;"
        f"r=subprocess.run({['/run/wrappers/bin/underwrap-local', exit_wrapper]!r});"
        "print(r.returncode)"
    )
    direct_status = machine.succeed(
        f"runuser -u testuser -- {shlex.quote(PYTHON3)} -c {shlex.quote(program)}"
    ).strip()
    assert direct_status == "37", f"direct parent observed {direct_status!r}"
    after = admin_get_blob(ADMIN_LOCAL, "local", exit_resource)
    assert after == expected, "exit-37 target did not persist the exact expected blob"
    assert base64.b64decode(
        decoded(after)["files"][".config/gate-exit-37/token"]
    ) == b"persisted-after-exit-37"
    assert_single_terminal(
        cursor, exit_wrapper, exit_resource, "resource_checkin"
    )
    machine.log("PASS: exec-gate post-exec writeback persists before exact exit 37")

    signal_resource = "test/gate-sigkill"
    signal_wrapper = "test-gate-sigkill"
    set_local_unlocked_permission(signal_wrapper)
    before = seed(
        signal_resource,
        {".config/gate-sigkill/token": b"before-sigkill"},
    )
    expected = expected_with_file(
        before,
        ".config/gate-sigkill/token",
        b"persisted-before-sigkill",
    )
    machine.succeed("rm -f /tmp/gate-sigkill-ready /tmp/gate-sigkill-status")
    cursor = journal_cursor()
    program = (
        "import subprocess;"
        f"r=subprocess.run({['/run/wrappers/bin/underwrap-local', signal_wrapper]!r});"
        "open('/tmp/gate-sigkill-status','w').write(str(r.returncode))"
    )
    machine.succeed(
        f"runuser -u testuser -- {shlex.quote(PYTHON3)} -c {shlex.quote(program)} "
        ">/tmp/gate-sigkill-parent.log 2>&1 &"
    )
    machine.wait_until_succeeds("test -e /tmp/gate-sigkill-ready")
    target_pid = machine.succeed(
        "pgrep -f '^/nix/store/.*/bin/sleep 6060$' | head -n 1"
    ).strip()
    assert target_pid, "could not find the ready SIGKILL target"
    machine.succeed(f"kill -KILL {shlex.quote(target_pid)}")
    machine.wait_until_succeeds("test -e /tmp/gate-sigkill-status")
    direct_status = machine.succeed("cat /tmp/gate-sigkill-status").strip()
    assert direct_status == str(-signal.SIGKILL), (
        f"direct parent observed {direct_status!r}, expected {-signal.SIGKILL}"
    )
    after = admin_get_blob(ADMIN_LOCAL, "local", signal_resource)
    assert after == expected, "SIGKILL target did not persist the exact expected blob"
    assert base64.b64decode(
        decoded(after)["files"][".config/gate-sigkill/token"]
    ) == b"persisted-before-sigkill"
    assert_single_terminal(
        cursor, signal_wrapper, signal_resource, "resource_checkin"
    )
    machine.log("PASS: exec-gate post-exec writeback persists before exact SIGKILL propagation")


def supervisor_lifecycle():
    """Check relay topology, cancellation teardown, and healthy status/writeback."""
    wrapper = "test-life-lifecycle"
    resource = "test/life-lifecycle"
    set_local_unlocked_permission(wrapper)
    admin_write_resource_value(
        ADMIN_LOCAL,
        "local",
        resource,
        "files",
        ".config/life/token",
        b"life-before",
    )
    resource_id = LOCAL_RESOURCE_IDS[resource]

    def terminal_records(cursor):
        return underwrapd_terminal_records_since(cursor, wrapper, resource_id)

    def assert_no_terminal(cursor, label):
        machine.sleep(0.2)
        records = terminal_records(cursor)
        assert records == [], f"{label}: cancellation emitted terminal records: {records}"

    def assert_one_terminal(cursor, expected):
        deadline = time.monotonic() + 3
        while True:
            records = terminal_records(cursor)
            if records or time.monotonic() >= deadline:
                break
            time.sleep(0.05)
        audits = [r["parsed_fields"].get("audit") for r in records]
        assert audits == [expected], records

    def proc_identity(pid):
        stat = machine.succeed(f"cat /proc/{pid}/stat").strip()
        starttime = stat[stat.rfind(')') + 2:].split()[19]
        return {
            "pid": pid,
            "start": starttime,
            "pidns": machine.succeed(f"readlink /proc/{pid}/ns/pid").strip(),
            "mntns": machine.succeed(f"readlink /proc/{pid}/ns/mnt").strip(),
        }

    def parent_of(pid):
        return machine.succeed(
            f"awk '/^PPid:/ {{print $2}}' /proc/{pid}/status"
        ).strip()

    def chain_for_sleep(seconds):
        descendant = machine.succeed(
            f"pgrep -f '^/nix/store/.*/bin/sleep {seconds}$' | head -n1"
        ).strip()
        assert descendant, f"missing lifecycle descendant sleep {seconds}"
        target = parent_of(descendant)
        relay = parent_of(target)
        host_ns = parent_of(relay)
        stage1 = parent_of(host_ns)
        chain = {
            "descendant": proc_identity(descendant),
            "target": proc_identity(target),
            "relay": proc_identity(relay),
            "host_ns": proc_identity(host_ns),
            "stage1": proc_identity(stage1),
        }
        pidns = chain["target"]["pidns"]
        helper_pids = machine.succeed(
            "for p in /proc/[0-9]*; do "
            f"[ \"$(readlink $p/ns/pid 2>/dev/null)\" = {shlex.quote(pidns)} ] || continue; "
            "c=$(cat $p/comm 2>/dev/null || true); "
            "case $c in dbus-daemon|underwrap-secre*) echo ${p##*/}:$c;; esac; done"
        ).split()
        assert len(helper_pids) >= 2, helper_pids
        for helper in helper_pids:
            pid, comm = helper.split(":", 1)
            chain[f"helper-{comm}-{pid}"] = proc_identity(pid)
        return chain

    def identity_alive(identity):
        pid = identity["pid"]
        status, _ = machine.execute(
            f"test -r /proc/{pid}/stat && "
            f"test \"$(cat /proc/{pid}/stat | sed 's/.*) //')\" != '' && "
            f"test \"$(readlink /proc/{pid}/ns/pid)\" = {shlex.quote(identity['pidns'])} && "
            f"test \"$(readlink /proc/{pid}/ns/mnt)\" = {shlex.quote(identity['mntns'])}"
        )
        if status != 0:
            return False
        stat = machine.succeed(f"cat /proc/{pid}/stat").strip()
        return stat[stat.rfind(')') + 2:].split()[19] == identity["start"]

    def wait_gone(chain, label):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            if not any(identity_alive(value) for value in chain.values()):
                break
            time.sleep(0.05)
        survivors = [name for name, value in chain.items() if identity_alive(value)]
        assert survivors == [], f"{label}: identity-bound survivors: {survivors}"
        pidns = chain["target"]["pidns"]
        for namespace_file in ("pid", "pid_for_children"):
            holders = machine.succeed(
                "for p in /proc/[0-9]*; do "
                f"[ \"$(readlink $p/ns/{namespace_file} 2>/dev/null)\" = {shlex.quote(pidns)} ] && echo $p; "
                "done; true"
            ).strip()
            assert holders == "", (
                f"{label}: PID namespace {pidns} retained through "
                f"ns/{namespace_file} by {holders}"
            )
        mntns = chain["target"]["mntns"]
        holders = machine.succeed(
            "for p in /proc/[0-9]*; do "
            f"[ \"$(readlink $p/ns/mnt 2>/dev/null)\" = {shlex.quote(mntns)} ] && echo $p; "
            "done; true"
        ).strip()
        assert holders == "", f"{label}: mount namespace {mntns} retained by {holders}"

    def status_fields(pid):
        text = machine.succeed(f"cat /proc/{pid}/status")
        fields = {}
        for line in text.splitlines():
            if ":" in line:
                key, value = line.split(":", 1)
                fields[key] = value.strip()
        return fields

    def assert_topology(chain):
        target = chain["target"]["pid"]
        relay = chain["relay"]["pid"]
        target_fields = status_fields(target)
        relay_fields = status_fields(relay)
        assert target_fields["NSpid"].split()[-1] == "2", target_fields["NSpid"]
        assert relay_fields["NSpid"].split()[-1] == "1", relay_fields["NSpid"]
        assert target_fields["PPid"] == relay, (target_fields["PPid"], relay)
        target_stat = machine.succeed(f"cat /proc/{target}/stat")
        values = target_stat.rpartition(")")[2].split()
        assert values[2] == target and values[3] == target and values[4] == "0", values[:5]
        relay_uid = relay_fields["Uid"].split()
        target_uid = target_fields["Uid"].split()
        assert relay_uid == target_uid and len(set(target_uid)) == 1
        assert relay_fields["Gid"].split() == target_fields["Gid"].split()
        assert target_uid[0] not in ("0", "1099")
        for fields, label in ((relay_fields, "relay"), (target_fields, "target")):
            assert fields["NoNewPrivs"] == "1", (label, fields["NoNewPrivs"])
            for cap in ("CapInh", "CapPrm", "CapEff", "CapAmb"):
                assert fields[cap] == "0000000000000000", (label, cap, fields[cap])
        _assert_no_leaked_fds(target)
        resource_socket = machine.succeed(
            f"readlink /proc/{chain['host_ns']['pid']}/fd/20"
        ).strip()
        assert resource_socket.startswith("socket:["), resource_socket
        internal_pipes = set()
        for role in ("stage1", "host_ns", "relay"):
            pid = chain[role]["pid"]
            links = machine.succeed(
                f"for f in /proc/{pid}/fd/*; do readlink $f 2>/dev/null || true; done"
            ).splitlines()
            internal_pipes.update(link for link in links if link.startswith("pipe:["))
        helper_roles = {
            role: identity["pid"]
            for role, identity in chain.items()
            if role.startswith("helper-")
        }
        helper_pids = list(helper_roles.values())
        assert len(helper_pids) >= 2, helper_pids

        secretsd_pids = [
            pid for role, pid in helper_roles.items() if "underwrap-secre" in role
        ]
        assert len(secretsd_pids) == 1, helper_roles
        secretsd_pid = secretsd_pids[0]
        secretsd_argv = machine.succeed(
            f"tr '\\0' '\\n' < /proc/{secretsd_pid}/cmdline"
        ).splitlines()
        publication_index = secretsd_argv.index("--publication-fd") + 1
        publication_fd = secretsd_argv[publication_index]
        publisher_socket = machine.succeed(
            f"readlink /proc/{secretsd_pid}/fd/{publication_fd}"
        ).strip()
        assert publisher_socket.startswith("socket:["), publisher_socket

        host_socket_fds = machine.succeed(
            f"for f in /proc/{chain['host_ns']['pid']}/fd/*; do "
            "l=$(readlink $f 2>/dev/null || true); "
            "case $l in socket:*) echo ${f##*/}:$l;; esac; done"
        ).splitlines()
        host_sockets = dict(entry.split(":", 1) for entry in host_socket_fds)
        assert host_sockets.get("20") == resource_socket, host_sockets
        receiver_sockets = [link for fd, link in host_sockets.items() if fd != "20"]
        assert len(receiver_sockets) == 1, host_sockets
        receiver_socket = receiver_sockets[0]
        assert receiver_socket != publisher_socket

        for socket_link, expected_pid, label in (
            (publisher_socket, secretsd_pid, "publisher"),
            (receiver_socket, chain["host_ns"]["pid"], "receiver"),
        ):
            holders = machine.succeed(
                "for p in /proc/[0-9]*; do for f in $p/fd/*; do "
                f"[ \"$(readlink $f 2>/dev/null)\" = {shlex.quote(socket_link)} ] "
                "&& echo ${p##*/}; done; done; true"
            ).split()
            assert holders == [expected_pid], (label, socket_link, holders)
        machine.log(
            "PASS: host alone owns the publication receiver and secretsd alone "
            "owns the publisher; relay, target, dbus-daemon, and descendants retain neither"
        )
        for pid in helper_pids:
            helper_fields = status_fields(pid)
            assert helper_fields["Uid"].split() == target_uid, (pid, helper_fields["Uid"])
            assert helper_fields["Gid"].split() == target_fields["Gid"].split(), (
                pid,
                helper_fields["Gid"],
            )
            assert helper_fields["NoNewPrivs"] == "1", (pid, helper_fields["NoNewPrivs"])
            for cap in ("CapInh", "CapPrm", "CapEff", "CapAmb"):
                assert helper_fields[cap] == "0000000000000000", (
                    pid,
                    cap,
                    helper_fields[cap],
                )
        for pid in [target, *helper_pids]:
            links = machine.succeed(
                f"for f in /proc/{pid}/fd/*; do readlink $f 2>/dev/null || true; done"
            ).splitlines()
            overlap = internal_pipes.intersection(links)
            assert not overlap, f"PID {pid} retained lifecycle pipe(s) {overlap}"
            assert resource_socket not in links, (
                f"PID {pid} retained fd-20 resource channel {resource_socket}"
            )

    def launch(mode, seconds):
        marker = f"/tmp/life-ready-{mode}"
        status_path = f"/tmp/life-status-{mode}"
        machine.succeed(f"rm -f {marker} {status_path}")
        argv = [
            "/run/current-system/sw/bin/env",
            "--default-signal=INT",
            "--default-signal=TERM",
            "/run/wrappers/bin/underwrap-local",
            wrapper,
            mode,
            str(seconds),
        ]
        program = (
            "import subprocess;"
            f"r=subprocess.run({argv!r});"
            f"open({status_path!r},'w').write(str(r.returncode))"
        )
        machine.succeed(
            f"runuser -u testuser -- {shlex.quote(PYTHON3)} -c {shlex.quote(program)} "
            f">/tmp/life-parent-{mode}.log 2>&1 &"
        )
        machine.wait_until_succeeds(f"test -e {marker}")
        return status_path, chain_for_sleep(str(seconds))

    # Prime one canonical combined files/libsecret generation deliberately: the
    # fresh empty libsecret state acquires its provider defaults on first use.
    # All cancellation and unchanged comparisons bind to the resulting bytes.
    cursor = journal_cursor()
    machine.succeed(su(f"/run/wrappers/bin/underwrap-local {wrapper} seed"))
    assert_one_terminal(cursor, "resource_checkin")
    baseline = admin_get_blob(ADMIN_LOCAL, "local", resource)

    # A target-owned state pathname carries no publication authority.
    # Canonical, noncanonical, oversized, and semantically invalid bytes there
    # are never an input to host selection.
    for mode in (
        "publ-file-canonical",
        "publ-file-noncanonical",
        "publ-file-semantic",
        "publ-file-oversized",
    ):
        cursor = journal_cursor()
        machine.succeed(su(f"/run/wrappers/bin/underwrap-local {wrapper} {mode}"))
        assert admin_get_blob(ADMIN_LOCAL, "local", resource) == baseline, mode
        assert_one_terminal(cursor, "resource_usage_signal")
    machine.log(
        "PASS: live same-UID writes to every old final-state path attack shape "
        "are irrelevant to selected libsecret state"
    )

    # A legitimate generation near the independent private 1-MiB publication
    # cap remains below the 2-MiB complete-resource cap, so it must pass host
    # ACK, bulk Checkin, and backend persistence.
    before_near_cap = baseline
    cursor = journal_cursor()
    program = (
        "import subprocess;"
        f"r=subprocess.run({['/run/wrappers/bin/underwrap-local', wrapper, 'publ-near-cap']!r});"
        "print(r.returncode)"
    )
    near_cap_status = machine.succeed(
        f"runuser -u testuser -- {shlex.quote(PYTHON3)} -c {shlex.quote(program)}"
    ).strip()
    assert near_cap_status == "0", near_cap_status
    baseline = admin_get_blob(ADMIN_LOCAL, "local", resource)
    assert baseline != before_near_cap, "near-cap publication did not persist"
    near_cap_stored = base64.b64decode(baseline)
    near_cap_libsecret = json.dumps(
        json.loads(near_cap_stored)["libsecret"],
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    ).encode()
    assert 900_000 <= len(near_cap_libsecret) <= 1_048_576, (
        "persisted near-cap libsecret section is outside the publication range: "
        f"{len(near_cap_libsecret)} bytes"
    )
    assert_one_terminal(cursor, "resource_checkin")

    # A separate D-Bus lookup proves the near-cap generation made a subsequent
    # Checkout. The target verifies the deterministic item internally and emits
    # no value; this clean run must retain the persisted canonical bytes.
    cursor = journal_cursor()
    machine.succeed(
        su(f"/run/wrappers/bin/underwrap-local {wrapper} publ-near-cap-readback")
    )
    assert admin_get_blob(ADMIN_LOCAL, "local", resource) == baseline
    assert_one_terminal(cursor, "resource_usage_signal")
    machine.log(
        "PASS: near-cap libsecret publication persists through bulk Checkin and "
        "a subsequent D-Bus readback leaves the canonical blob unchanged"
    )

    cancellation_cases = [
        ("stage1-kill", 6211, "stage1", signal.SIGKILL),
        ("host-kill", 6212, "host_ns", signal.SIGKILL),
        ("relay-kill", 6213, "relay", signal.SIGKILL),
        ("stage1-int", 6214, "stage1", signal.SIGINT),
        ("stage1-term", 6215, "stage1", signal.SIGTERM),
        ("host-int", 6216, "host_ns", signal.SIGINT),
        ("host-term", 6217, "host_ns", signal.SIGTERM),
    ]
    for index, (mode, seconds, victim, sig) in enumerate(cancellation_cases):
        cursor = journal_cursor()
        status_path, chain = launch(mode, seconds)
        if index == 0:
            assert_topology(chain)
        assert identity_alive(chain["target"]), f"{mode}: target completed before cancellation"
        machine.succeed(f"kill -{sig} {chain[victim]['pid']}")
        machine.wait_until_succeeds(f"test -e {status_path}")
        observed = machine.succeed(f"cat {status_path}").strip()
        expected = 1 if victim == "relay" else -sig
        assert observed == str(expected), (mode, observed, expected)
        wait_gone(chain, mode)
        assert_no_terminal(cursor, mode)
        assert admin_get_blob(ADMIN_LOCAL, "local", resource) == baseline
        machine.log(f"PASS: lifecycle {mode} tears down complete namespace without terminal verb")

    # A healthy unchanged projection emits exactly UsageSignal through the
    # gate/status/clean-relay conjunction.
    cursor = journal_cursor()
    before_unchanged = admin_get_blob(ADMIN_LOCAL, "local", resource)
    machine.succeed(su(f"/run/wrappers/bin/underwrap-local {wrapper} unchanged"))
    assert admin_get_blob(ADMIN_LOCAL, "local", resource) == before_unchanged
    assert_one_terminal(cursor, "resource_usage_signal")

    # Healthy combined success and exit 37 retain exact writeback/status.
    for mode, expected_status in (("success", 0), ("exit37", 37)):
        cursor = journal_cursor()
        program = (
            "import subprocess;"
            f"r=subprocess.run({['/run/wrappers/bin/underwrap-local', wrapper, mode]!r});"
            "print(r.returncode)"
        )
        observed = machine.succeed(
            f"runuser -u testuser -- {shlex.quote(PYTHON3)} -c {shlex.quote(program)}"
        ).strip()
        assert observed == str(expected_status), (mode, observed)
        assert_one_terminal(cursor, "resource_checkin")
        blob = json.loads(base64.b64decode(admin_get_blob(ADMIN_LOCAL, "local", resource)))
        assert base64.b64decode(blob["files"][".config/life/token"]) == f"life-file-{mode}".encode()
        encoded_secret = base64.b64encode(f"life-libsecret-{mode}".encode()).decode()
        assert encoded_secret in json.dumps(blob["libsecret"]), blob["libsecret"]

    # Direct target signals retain writeback and exact observer status.
    for mode, seconds, sig in (
        ("target-term", 6221, signal.SIGTERM),
        ("target-kill", 6222, signal.SIGKILL),
    ):
        cursor = journal_cursor()
        status_path, chain = launch(mode, seconds)
        machine.succeed(f"kill -{sig} {chain['target']['pid']}")
        machine.wait_until_succeeds(f"test -e {status_path}")
        observed = machine.succeed(f"cat {status_path}").strip()
        assert observed == str(-sig), (mode, observed)
        wait_gone(chain, mode)
        assert_one_terminal(cursor, "resource_checkin")

    # Deterministically establish the documented late-dispatch race: stopping
    # stage 1 does not cancel its live descendants, so the target can finish and
    # Checkin can complete before stage 1 is subsequently killed. That completed
    # backend mutation cannot be retracted.
    cursor = journal_cursor()
    status_path, chain = launch("late-dispatch", 6223)
    machine.succeed(f"kill -STOP {chain['stage1']['pid']}")
    assert identity_alive(chain["target"]), "SIGSTOP was incorrectly treated as termination"
    stage1_stat = machine.succeed(f"cat /proc/{chain['stage1']['pid']}/stat").strip()
    assert stage1_stat[stage1_stat.rfind(')') + 2:].split()[0] == "T", stage1_stat
    machine.succeed(f"kill -TERM {chain['target']['pid']}")
    assert_one_terminal(cursor, "resource_checkin")
    late_blob = json.loads(base64.b64decode(admin_get_blob(ADMIN_LOCAL, "local", resource)))
    assert base64.b64decode(late_blob["files"][".config/life/token"]) == b"life-file-late-dispatch"
    encoded_secret = base64.b64encode(b"life-libsecret-late-dispatch").decode()
    assert encoded_secret in json.dumps(late_blob["libsecret"]), late_blob["libsecret"]
    assert identity_alive(chain["stage1"]), "stopped stage 1 exited before late cancellation"
    machine.succeed(f"kill -KILL {chain['stage1']['pid']}")
    machine.wait_until_succeeds(f"test -e {status_path}")
    assert machine.succeed(f"cat {status_path}").strip() == str(-signal.SIGKILL)
    wait_gone(chain, "late-dispatch")
    assert_one_terminal(cursor, "resource_checkin")
    machine.log("PASS: completed late Checkin remains committed after stopped stage 1 is killed")

    # A configured file-capability image executes with zero capabilities under
    # no_new_privs and cannot outlive relay teardown.
    filecap_wrapper = "test-life-filecap"
    filecap_resource = "test/life-filecap"
    filecap_target = "/run/underwrap-life-filecap/sleep"
    machine.succeed(
        "install -d -m 0755 /run/underwrap-life-filecap && "
        f"install -m 0755 /run/current-system/sw/bin/sleep {filecap_target} && "
        f"setcap cap_net_bind_service+ep {filecap_target}"
    )
    capability = machine.succeed(f"getcap {filecap_target}").strip()
    assert capability == f"{filecap_target} cap_net_bind_service=ep", capability
    set_local_unlocked_permission(filecap_wrapper)
    cursor = journal_cursor()
    machine.succeed(
        "runuser -u testuser -- /run/wrappers/bin/underwrap-local "
        "test-life-filecap 6230 >/tmp/life-filecap.log 2>&1 &"
    )
    machine.wait_until_succeeds(
        f"pgrep -f '^{filecap_target} 6230$' >/dev/null"
    )
    target = machine.succeed(
        f"pgrep -f '^{filecap_target} 6230$' | head -n1"
    ).strip()
    relay = parent_of(target)
    host_ns = parent_of(relay)
    stage1 = parent_of(host_ns)
    filecap_chain = {
        "target": proc_identity(target),
        "relay": proc_identity(relay),
        "host_ns": proc_identity(host_ns),
        "stage1": proc_identity(stage1),
    }
    fields = status_fields(target)
    assert fields["NoNewPrivs"] == "1"
    for cap in ("CapInh", "CapPrm", "CapEff", "CapAmb"):
        assert fields[cap] == "0000000000000000", (cap, fields[cap])
    assert machine.succeed(f"getcap {filecap_target}").strip() == capability
    assert identity_alive(filecap_chain["target"])
    machine.succeed(f"kill -KILL {stage1}")
    wait_gone(filecap_chain, "filecap-stage1-kill")
    machine.sleep(0.2)
    records = underwrapd_terminal_records_since(
        cursor, filecap_wrapper, LOCAL_RESOURCE_IDS[filecap_resource]
    )
    assert records == [], records
    machine.log(
        "PASS: configured file-capability target gains no caps and dies through stage1 cascade"
    )


def resource_size_contract():
    """Exercise the deployed complete-resource boundary through fd 20."""
    env_wrapper = "test-size-env"
    env_resource = "test/size-env"
    files_wrapper = "test-size-files"
    files_resource = "test/size-files"

    def report_for(value):
        return (
            f"length={len(value)} "
            f"sha256={hashlib.sha256(value).hexdigest()}"
        )

    def assert_one_terminal(cursor, expected_audit):
        deadline = time.monotonic() + 3
        while True:
            records = underwrapd_terminal_records_since(
                cursor, files_wrapper, LOCAL_RESOURCE_IDS[files_resource]
            )
            if records or time.monotonic() >= deadline:
                break
            time.sleep(0.05)
        time.sleep(0.25)
        records = underwrapd_terminal_records_since(
            cursor, files_wrapper, LOCAL_RESOURCE_IDS[files_resource]
        )
        audits = [record["parsed_fields"].get("audit") for record in records]
        assert audits == [expected_audit], records

    def assert_no_terminal(cursor):
        # The wrapper's completed process is the synchronization point; this
        # lookup proves no terminal daemon verb followed it.
        machine.sleep(0.2)
        records = underwrapd_terminal_records_since(
            cursor, files_wrapper, LOCAL_RESOURCE_IDS[files_resource]
        )
        assert records == [], records

    def assert_one_size_rejection(cursor, expected_length):
        def rejected_since_cursor():
            output = machine.succeed(
                "journalctl --no-pager "
                f"--after-cursor={shlex.quote(cursor)} "
                "SYSLOG_IDENTIFIER=underwrap-ns --output=json"
            )
            records = [
                json.loads(line)
                for line in output.splitlines()
                if line.strip()
            ]
            return [
                record for record in records
                if record.get("AUDIT") == "writeback_rejected"
                and record.get("REASON") == "resource_too_large"
                and record.get("RESOURCE_ID") == LOCAL_RESOURCE_IDS[files_resource]
                and record.get("RESOURCE_NAME") == files_resource
            ]

        deadline = time.monotonic() + 3
        while not rejected_since_cursor() and time.monotonic() < deadline:
            time.sleep(0.05)
        # The first record proves arrival, not uniqueness. Let journald finish
        # accepting this invocation's datagrams, then re-query the entire
        # post-cursor set before making the exact-one assertion.
        time.sleep(0.25)
        rejected = rejected_since_cursor()
        assert len(rejected) == 1, rejected
        record = rejected[0]
        assert record.get("MAXIMUM_LENGTH") == str(MAX_CANONICAL_RESOURCE_BYTES), record
        assert record.get("ATTEMPTED_LENGTH") == str(expected_length), record

    # Env values above the generic 64-KiB control limit travel only in Checkout
    # over the authenticated resource channel. The target prints its length and
    # digest, never the value.
    set_local_unlocked_permission(env_wrapper)
    admin_write_resource_value(
        ADMIN_LOCAL, "local", env_resource, "env", "SIZE_ENV_BULK", SIZE_ENV_VALUE
    )
    env_output = machine.succeed(
        su(f"/run/wrappers/bin/underwrap-local {env_wrapper}")
    ).strip()
    assert env_output == report_for(SIZE_ENV_VALUE), env_output

    # Build a real simple ResourceBlob shape with a small files value and a
    # valid padding collection. Tuning only an ASCII label makes Python's sorted
    # compact JSON exactly match the JCS representation used by this schema.
    blob = {
        "env": {},
        "files": {
            ".config/size-boundary/token": base64.b64encode(SIZE_FILES_INITIAL).decode(),
        },
        "libsecret": {
            "aliases": {"default": "padding"},
            "collections": {
                "padding": {
                    "label": "",
                    "created": 0,
                    "modified": 0,
                    "items": {},
                },
            },
        },
        "schema_version": 1,
    }
    assert set(blob) == {"env", "files", "libsecret", "schema_version"}
    assert blob["libsecret"]["aliases"] == {"default": "padding"}
    assert set(blob["libsecret"]["collections"]["padding"]) == {
        "label", "created", "modified", "items"
    }
    compact = lambda value: json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode()
    blob["libsecret"]["collections"]["padding"]["label"] = "x" * (
        MAX_CANONICAL_RESOURCE_BYTES - len(compact(blob))
    )
    canonical = compact(blob)
    assert canonical == compact(json.loads(canonical))
    assert len(canonical) == MAX_CANONICAL_RESOURCE_BYTES
    assert blob["libsecret"]["collections"]["padding"]["label"].isascii()
    append_candidate = json.loads(canonical)
    append_candidate["files"][".config/size-boundary/token"] = base64.b64encode(
        SIZE_FILES_OVERSIZE
    ).decode()
    append_candidate_length = len(compact(append_candidate))
    assert append_candidate_length > MAX_CANONICAL_RESOURCE_BYTES

    # Root-owned VM instrumentation seeds the local PlainBackend atomically;
    # normal wrapper traffic subsequently uses only the installed chain.
    install_local_plain_resource_blob(files_resource, canonical)
    stored = base64.b64decode(admin_get_blob(ADMIN_LOCAL, "local", files_resource))
    assert stored == canonical
    assert len(stored) == MAX_CANONICAL_RESOURCE_BYTES
    set_local_unlocked_permission(files_wrapper)

    cursor = journal_cursor()
    initial_output = machine.succeed(
        su(f"/run/wrappers/bin/underwrap-local {files_wrapper} verify")
    ).strip()
    assert initial_output == report_for(SIZE_FILES_INITIAL), initial_output
    assert_one_terminal(cursor, "resource_usage_signal")

    # One appended byte makes the complete canonical state oversized. Target
    # status remains authoritative, no resource terminal verb is sent, and the
    # backend's original canonical bytes survive byte-for-byte.
    cursor = journal_cursor()
    status, diagnostic = machine.execute(
        su(f"/run/wrappers/bin/underwrap-local {files_wrapper} append") + " 2>&1"
    )
    assert status == 0, diagnostic
    assert LOCAL_RESOURCE_IDS[files_resource] in diagnostic, diagnostic
    assert "complete resource size limit" in diagnostic, diagnostic
    assert f"maximum of {MAX_CANONICAL_RESOURCE_BYTES} bytes" in diagnostic, diagnostic
    assert f"(got {append_candidate_length})" in diagnostic, diagnostic
    for fixture in (
        SIZE_ENV_VALUE,
        SIZE_FILES_INITIAL,
        SIZE_FILES_REPLACEMENT,
        SIZE_FILES_OVERSIZE,
        SIZE_ADMIN_OVERSIZE,
    ):
        assert fixture.decode() not in diagnostic, "resource-size diagnostic exposed fixture material"
    assert_no_terminal(cursor)
    assert_one_size_rejection(cursor, append_candidate_length)
    assert base64.b64decode(admin_get_blob(ADMIN_LOCAL, "local", files_resource)) == canonical

    # An authenticated admin mutation also composes against the exact-MAX blob.
    # It must return the stable public error without a success envelope or a
    # persistence side effect.
    before_admin_attempt = admin_get_blob(ADMIN_LOCAL, "local", files_resource)
    admin_error = admin_api_expect_status(
        ADMIN_LOCAL,
        f"/v1/admin/stores/local/secrets/{_seg(LOCAL_RESOURCE_IDS[files_resource])}/env/SIZE_OVERSIZE",
        {"material_b64": base64.b64encode(SIZE_ADMIN_OVERSIZE).decode()},
        status=400,
        method="PUT",
    )
    assert admin_error.get("code") == "resource_too_large", admin_error
    assert admin_error.get("ok") is None, admin_error
    assert admin_get_blob(ADMIN_LOCAL, "local", files_resource) == before_admin_attempt

    # A same-length target replacement remains exactly MAX, emits one Checkin,
    # and becomes the next Checkout's verified files value.
    cursor = journal_cursor()
    replacement_output = machine.succeed(
        su(f"/run/wrappers/bin/underwrap-local {files_wrapper} replace")
    )
    assert replacement_output == "", replacement_output
    assert_one_terminal(cursor, "resource_checkin")
    replaced = base64.b64decode(admin_get_blob(ADMIN_LOCAL, "local", files_resource))
    assert len(replaced) == MAX_CANONICAL_RESOURCE_BYTES
    replaced_blob = json.loads(replaced)
    assert base64.b64decode(
        replaced_blob["files"][".config/size-boundary/token"]
    ) == SIZE_FILES_REPLACEMENT

    cursor = journal_cursor()
    readback_output = machine.succeed(
        su(f"/run/wrappers/bin/underwrap-local {files_wrapper} verify")
    ).strip()
    assert readback_output == report_for(SIZE_FILES_REPLACEMENT), readback_output
    assert base64.b64decode(admin_get_blob(ADMIN_LOCAL, "local", files_resource)) == replaced
    assert_one_terminal(cursor, "resource_usage_signal")
    machine.log(
        "PASS: resource-size exact-MAX resource crosses deployed Checkout/Checkin; "
        "oversize mutations preserve target status and canonical storage"
    )


def plaintext_runtime_backend_url_is_rejected_before_io():
    """A standalone handcrafted daemon config cannot let unusable
    credentials mask the HTTPS admission error or contact plaintext."""
    trap_port = 19666
    machine.succeed("rm -f /tmp/plain-url-trap-ready /tmp/plain-url-trap-count")
    trap_program = (
        "import socket;"
        "s=socket.socket();s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1);"
        f"s.bind(('127.0.0.1',{trap_port}));s.listen(1);s.settimeout(3);"
        "open('/tmp/plain-url-trap-ready','w').write('ready');count=0;"
        "\ntry:\n c,_=s.accept();count=1;c.close()\nexcept TimeoutError:\n pass\n"
        "open('/tmp/plain-url-trap-count','w').write(str(count));s.close()"
    )
    trap_command = " ".join(
        shlex.quote(part)
        for part in [PYTHON3, "-c", trap_program]
    )
    machine.succeed(
        f"{trap_command} >/tmp/plain-url-trap-python.log 2>&1 & echo $! >/tmp/plain-url-trap-pid"
    )
    machine.wait_until_succeeds("test -f /tmp/plain-url-trap-ready")

    config = {
        "backend": {
            "url": f"http://127.0.0.1:{trap_port}/runtime/base/",
            "ca_file": "/unusable/credentials/ca.pem",
            "client_identity_file": "/unusable/credentials/client.identity.pem",
        },
        "instance": "plaintext-test",
        "socket": "/run/underwrap-plaintext-test/wrapper.sock",
        "group": "root",
        "debug_tool_target": "/unusable/debug-tool",
        "target_runtime_dir_base": "/run/underwrap-runtime",
        "wrappers": {},
    }
    with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as body:
        json.dump(config, body, separators=(",", ":"))
        host_config = body.name
    guest_config = "/run/underwrap-plaintext-test.json"
    try:
        machine.copy_from_host(host_config, guest_config)
        machine.succeed(f"chmod 0600 {guest_config}")
        exit_code = int(
            machine.succeed(
                f"set +e; timeout 10s underwrapd --config {guest_config} "
                ">/tmp/plain-url-daemon.log 2>&1; status=$?; printf '%s\\n' \"$status\""
            ).strip()
        )
    finally:
        machine.succeed(f"rm -f {guest_config}")
        os.unlink(host_config)
    assert exit_code != 0, "plaintext standalone daemon unexpectedly started"
    assert exit_code != 124, "plaintext standalone daemon did not fail before the timeout"
    machine.wait_until_succeeds("test -f /tmp/plain-url-trap-count")
    assert machine.succeed("cat /tmp/plain-url-trap-count").strip() == "0"
    assert not machine.succeed(
        "test -S /run/underwrap-plaintext-test/wrapper.sock && echo present || true"
    ).strip()
    diagnostics = machine.succeed("cat /tmp/plain-url-daemon.log")
    assert diagnostics.count(
        "backend.url must be an absolute HTTPS URL with a host"
    ) == 1, diagnostics
    assert "daemon ready" not in diagnostics
    assert "/unusable/credentials" not in diagnostics
    for forbidden in (
        "BEGIN PRIVATE KEY",
        "BEGIN CERTIFICATE",
        "secret_via_service",
        "hello-locked",
    ):
        assert forbidden not in diagnostics
    machine.log(
        "PASS: plaintext runtime backend URL rejected before credential, socket, readiness, or network I/O"
    )


def encrypted_store_in_memory_keys():
    """Verifies the in-memory-master-key flow end-to-end against the
    local backend's admin API.

    Phases:
      1. Generate a fresh key without storage mutation, submit it to
         create an encrypted store, and verify that no `.key` file is
         left anywhere under the backend's state directory.
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
    key_resp = admin_api(ADMIN_LOCAL, "/v1/admin/store-keys")
    key_hex_original = key_resp["key_hex"]
    assert isinstance(key_hex_original, str) and len(key_hex_original) == 64
    create_resp = admin_api(
        ADMIN_LOCAL,
        "/v1/admin/stores",
        {
            "name": "vault",
            "backend": "encrypted",
            "key_hex": key_hex_original,
        },
    )
    store_id = create_resp["store"]["id"]
    assert "key_hex" not in create_resp
    assert create_resp["store"]["lifecycle_state"] == "unlocked"
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
    # Seed both the control/resources document and a resource blob so the
    # later offline recovery drill verifies representative control and secret
    # data through normal APIs.
    resource_resp = admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/resources",
        {"name": "vault/greeting"},
    )
    resource_id = resource_resp["resource"]["id"]
    resources_before = admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/resources",
        method="GET",
    )
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
    # In-memory session state was reset; the operator persists, so use the
    # ordinary login path again.
    login_admin_session(ADMIN_LOCAL)

    stores_after_restart = admin_api(
        ADMIN_LOCAL, "/v1/admin/stores", None, method="GET"
    )["items"]
    locked_item = next(s for s in stores_after_restart if s["id"] == store_id)
    assert locked_item["lifecycle_state"] == "locked", (
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
    wrong_key = (
        "1" if key_hex_original[0] == "0" else "0"
    ) + key_hex_original[1:]
    wrong_unlock = admin_api_expect_status(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/unlock",
        {"key_hex": wrong_key},
        status=403,
    )
    assert wrong_unlock["code"] == "key_mismatch", wrong_unlock
    stores_still_locked = admin_api(
        ADMIN_LOCAL, "/v1/admin/stores", None, method="GET"
    )["items"]
    assert next(s for s in stores_still_locked if s["id"] == store_id)[
        "lifecycle_state"
    ] == "locked", "wrong-hex unlock must not transition the registry"

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
    canonical = f"/var/lib/underwrap-backend-local/data-root/{store_id}"
    workspace_prefix = f".{store_id}.underwrap-rotation-"

    def encrypted_tree_snapshot(path):
        """Exact path/type/mode/raw-byte snapshot for one encrypted tree."""
        script = f"""set -euo pipefail
root={shlex.quote(path)}
find -P "$root" -printf '%P\\t%y\\t%m\\n' | LC_ALL=C sort
while IFS= read -r -d '' file; do
  rel="${{file#"$root"/}}"
  printf 'raw\\t%s\\t' "$rel"
  base64 -w0 -- "$file"
  printf '\\n'
done < <(find -P "$root" -type f -print0 | LC_ALL=C sort -z)
"""
        return machine.succeed("bash -c " + shlex.quote(script))

    def assert_encrypted_tree_modes(path):
        bad = machine.succeed(
            f"find -P {shlex.quote(path)} "
            "\\( -type d ! -perm 0700 -o -type f ! -perm 0600 -o -type l \\) -print"
        ).strip()
        assert bad == "", f"unexpected encrypted tree type/mode under {path}: {bad}"
    first_workspaces = machine.succeed(
        f"find {shlex.quote(os.path.dirname(canonical))} -mindepth 1 -maxdepth 1 "
        f"-type d -name {shlex.quote(workspace_prefix + '*')} -print"
    ).splitlines()
    assert len(first_workspaces) == 1, first_workspaces
    original_workspace = first_workspaces[0]
    assert_encrypted_tree_modes(original_workspace)

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

    # Restore, then rotate a second time. The first recovery generation must
    # remain byte-for-byte present while another unique generation is retained.
    admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/unlock",
        {"key_hex": key_hex_rotated},
    )
    original_snapshot = encrypted_tree_snapshot(original_workspace)
    second_rotate = admin_api(
        ADMIN_LOCAL, f"/v1/admin/stores/{store_id}/rotate-key", {}
    )
    key_hex_latest = second_rotate["key_hex"]
    assert key_hex_latest not in (key_hex_original, key_hex_rotated)
    assert encrypted_tree_snapshot(original_workspace) == original_snapshot
    all_workspaces = machine.succeed(
        f"find {shlex.quote(os.path.dirname(canonical))} -mindepth 1 -maxdepth 1 "
        f"-type d -name {shlex.quote(workspace_prefix + '*')} -print"
    ).splitlines()
    assert len(all_workspaces) == 2, all_workspaces
    rotated_workspace = next(path for path in all_workspaces if path != original_workspace)
    other_snapshot = encrypted_tree_snapshot(rotated_workspace)
    assert_encrypted_tree_modes(original_workspace)
    assert_encrypted_tree_modes(rotated_workspace)
    assert_encrypted_tree_modes(canonical)
    latest_snapshot = encrypted_tree_snapshot(canonical)

    # Independently verify the latest key through restart, unlock, canary, a
    # representative resource read, and the control/resources document.
    machine.succeed("systemctl restart underwrap-backend-local")
    machine.wait_for_unit("underwrap-backend-local.service")
    machine.wait_until_succeeds(
        f"curl -fsS --cacert {CA_CERT} "
        f"{ADMIN_LOCAL}/v1/admin/auth/bootstrap/status >/dev/null"
    )
    login_admin_session(ADMIN_LOCAL)
    admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/unlock",
        {"key_hex": key_hex_latest},
    )
    assert read_secret()["material_b64"] == slot_payload
    assert admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/resources",
        method="GET",
    ) == resources_before

    # Trusted-host recovery: stop/quiesce the backend, preserve the current
    # live generation without overwriting, and promote the selected original
    # backup. Startup does not reconcile anything; the old externally held key
    # unlocks the promoted snapshot.
    preserved_live = f"{canonical}.recovery-preserved-{uuid.uuid4().hex[:16]}"
    machine.succeed("systemctl stop underwrap-backend-local")
    machine.succeed(
        f"test ! -e {shlex.quote(preserved_live)} && test ! -L {shlex.quote(preserved_live)}; "
        f"test ! -L {shlex.quote(canonical)}; "
        f"if test -e {shlex.quote(canonical)}; then "
        f"  test -d {shlex.quote(canonical)}; "
        f"  mv --no-clobber --no-target-directory -- {shlex.quote(canonical)} {shlex.quote(preserved_live)}; "
        f"  test ! -e {shlex.quote(canonical)} && test ! -L {shlex.quote(canonical)}; "
        f"  test -d {shlex.quote(preserved_live)} && test ! -L {shlex.quote(preserved_live)}; "
        "else "
        f"  test ! -L {shlex.quote(canonical)}; "
        "fi; "
        f"test -d {shlex.quote(original_workspace + '/backup')} && test ! -L {shlex.quote(original_workspace + '/backup')}; "
        f"test ! -e {shlex.quote(canonical)} && test ! -L {shlex.quote(canonical)}; "
        f"mv --no-clobber --no-target-directory -- {shlex.quote(original_workspace + '/backup')} {shlex.quote(canonical)}; "
        f"test ! -e {shlex.quote(original_workspace + '/backup')} && test ! -L {shlex.quote(original_workspace + '/backup')}; "
        f"test -d {shlex.quote(canonical)} && test ! -L {shlex.quote(canonical)}"
    )
    machine.succeed("systemctl start underwrap-backend-local")
    machine.wait_for_unit("underwrap-backend-local.service")
    machine.wait_until_succeeds(
        f"curl -fsS --cacert {CA_CERT} "
        f"{ADMIN_LOCAL}/v1/admin/auth/bootstrap/status >/dev/null"
    )
    login_admin_session(ADMIN_LOCAL)
    admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/unlock",
        {"key_hex": key_hex_original},
    )
    assert read_secret()["material_b64"] == slot_payload
    assert admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/resources",
        method="GET",
    ) == resources_before

    # Restore the verified latest live generation non-destructively, verify it
    # again after restart/unlock, then remove only the selected old workspace.
    machine.succeed("systemctl stop underwrap-backend-local")
    machine.succeed(
        f"test ! -e {shlex.quote(original_workspace + '/backup')} && test ! -L {shlex.quote(original_workspace + '/backup')}; "
        f"mv --no-clobber --no-target-directory -- {shlex.quote(canonical)} {shlex.quote(original_workspace + '/backup')}; "
        f"test ! -e {shlex.quote(canonical)} && test ! -L {shlex.quote(canonical)}; "
        f"test -d {shlex.quote(original_workspace + '/backup')} && test ! -L {shlex.quote(original_workspace + '/backup')}; "
        f"test -d {shlex.quote(preserved_live)} && test ! -L {shlex.quote(preserved_live)}; "
        f"mv --no-clobber --no-target-directory -- {shlex.quote(preserved_live)} {shlex.quote(canonical)}; "
        f"test ! -e {shlex.quote(preserved_live)} && test ! -L {shlex.quote(preserved_live)}; "
        f"test -d {shlex.quote(canonical)} && test ! -L {shlex.quote(canonical)}"
    )
    machine.succeed("systemctl start underwrap-backend-local")
    machine.wait_for_unit("underwrap-backend-local.service")
    machine.wait_until_succeeds(
        f"curl -fsS --cacert {CA_CERT} "
        f"{ADMIN_LOCAL}/v1/admin/auth/bootstrap/status >/dev/null"
    )
    login_admin_session(ADMIN_LOCAL)
    admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/unlock",
        {"key_hex": key_hex_latest},
    )
    assert read_secret()["material_b64"] == slot_payload
    assert admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/resources",
        method="GET",
    ) == resources_before

    canonical_snapshot = encrypted_tree_snapshot(canonical)
    assert canonical_snapshot == latest_snapshot
    assert encrypted_tree_snapshot(rotated_workspace) == other_snapshot
    assert_encrypted_tree_modes(canonical)
    assert_encrypted_tree_modes(rotated_workspace)
    machine.succeed("systemctl stop underwrap-backend-local")
    machine.succeed(f"rm -rf -- {shlex.quote(original_workspace)}")
    assert not machine.succeed(
        f"test -e {shlex.quote(original_workspace)} && echo present || true"
    ).strip()
    assert encrypted_tree_snapshot(canonical) == canonical_snapshot
    assert encrypted_tree_snapshot(rotated_workspace) == other_snapshot
    assert_encrypted_tree_modes(canonical)
    assert_encrypted_tree_modes(rotated_workspace)
    machine.succeed("systemctl start underwrap-backend-local")
    machine.wait_for_unit("underwrap-backend-local.service")
    machine.wait_until_succeeds(
        f"curl -fsS --cacert {CA_CERT} "
        f"{ADMIN_LOCAL}/v1/admin/auth/bootstrap/status >/dev/null"
    )
    login_admin_session(ADMIN_LOCAL)
    admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/stores/{store_id}/unlock",
        {"key_hex": key_hex_latest},
    )
    assert read_secret()["material_b64"] == slot_payload

    journal = machine.succeed("journalctl -u underwrap-backend-local --no-pager")
    for forbidden in (key_hex_original, key_hex_rotated, key_hex_latest, "hello-locked"):
        assert forbidden not in journal
    stray = machine.succeed(
        "find /var/lib/underwrap-backend-local /run/underwrap-backend-local "
        "-name '*.key' 2>/dev/null || true"
    ).strip()
    assert stray == "", f"unexpected on-disk .key files: {stray}"

    machine.log("PASS: encrypted store retains recoverable generations and keeps master keys in memory only")


def remote_service_lifecycle():
    # ============================================================
    # REMOTE-STYLE INSTANCE: same-machine loopback backend lifecycle
    # ============================================================

    # Seed an Unlocked permission for the remote-style daemon's tuple.
    # The retained `tunnel` identifiers name the fixture and do not denote SSH transport.
    # The daemon has one `remote-echo` wrapper declaring the `remote/token` resource.
    admin_upsert_permission(
        ADMIN_REMOTE,
        "remote",
        REMOTE_CLIENT_INSTANCE,
        "remote-echo",
        "remote/token",
        {"kind": "unlocked"},
        writable=True,
    )

    # Store the resource value through the second loopback backend's admin API.
    admin_write_secret(ADMIN_REMOTE, "remote", "remote-echo", "remote/token/env/REMOTE_TOKEN", "secret_via_service")

    # The direct same-machine loopback path succeeds while the second backend is running.
    output = machine.succeed(su("XDG_RUNTIME_DIR=/tmp/evil-run /run/wrappers/bin/underwrap-tunnel remote-echo"))
    assert "REMOTE_TOKEN=secret_via_service" in output, f"expected REMOTE_TOKEN: {output}"
    # The remote-style target gets its own provisioned runtime directory.
    # The forced path is `<targetRuntimeDirBase>/<target-user-name>`, not the caller-supplied value.
    assert (
        "XDG_RUNTIME_DIR=/run/underwrap-test-runtime/underwrap-target-" in output
    ), f"expected forced provisioned XDG_RUNTIME_DIR: {output}"
    machine.log("PASS: remote-style resource reached the second backend over direct loopback HTTPS")

    # The separate local backend store does not receive the remote-style resource.
    machine.fail("test -f /var/lib/underwrap-backend-local/secrets/remote/token")
    machine.log("PASS: remote-style resource is absent from the separate local backend store")

    # Stopping the second loopback backend makes the remote-style wrapper fail.
    machine.succeed("systemctl stop underwrap-backend.service")
    time.sleep(0.5)
    machine.fail(su("/run/wrappers/bin/underwrap-tunnel remote-echo"))
    machine.log("PASS: remote-style loopback wrapper fails while its backend service is down")

    # Restarting the second loopback backend restores the same direct path.
    machine.succeed("systemctl start underwrap-backend.service")
    machine.wait_for_unit("underwrap-backend.service")
    time.sleep(1)
    output = machine.succeed(su("XDG_RUNTIME_DIR=/tmp/evil-run /run/wrappers/bin/underwrap-tunnel remote-echo"))
    assert "REMOTE_TOKEN=secret_via_service" in output
    assert (
        "XDG_RUNTIME_DIR=/run/underwrap-test-runtime/underwrap-target-" in output
    )
    machine.log("PASS: remote-style loopback wrapper works after backend service restart")


def local_backend_managed_permissions():
    # ============================================================
    # LOCAL INSTANCE: backend-managed permissions
    # ============================================================
    #
    # Permissions are now keyed on `(client_instance, wrapper, resource)`.
    # This section exercises the full lifecycle (no permission → denied,
    # Unlocked → allowed, deleted → denied, UnlockedUntil with a past
    # deadline → denied).

    # Remove the seed permission from the earlier section so we can
    # exercise the "no permission" path.
    admin_delete_permission(ADMIN_LOCAL, "local", LOCAL_CLIENT_INSTANCE, "test-echo2", "test/other")
    machine.fail(su("/run/wrappers/bin/underwrap-local test-echo2 hello"))
    machine.log("PASS: wrapper with no permission row is denied")

    # Unlocked permission → wrapper can read.
    admin_upsert_permission(
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
    machine.log("PASS: active permission row allows wrapper action")

    # Deleting the permission revokes future reads.
    admin_delete_permission(ADMIN_LOCAL, "local", LOCAL_CLIENT_INSTANCE, "test-echo2", "test/other")
    machine.fail(su("/run/wrappers/bin/underwrap-local test-echo2 hello"))
    machine.log("PASS: deleting the permission row revokes future wrapper action")

    # UnlockedUntil with a deadline guaranteed in the past
    # (1970-01-02) is denied lazily on the next request — no flaky
    # sleep needed and no risk of host/guest clock skew.
    admin_upsert_permission(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        "test-echo2",
        "test/other",
        {"kind": "unlocked-until", "until_epoch": 86400},
        reason="expired allow",
    )
    machine.fail(su("/run/wrappers/bin/underwrap-local test-echo2 hello"))
    machine.log("PASS: expired UnlockedUntil permission is not honored")

    # Clean up so the next test starts fresh.
    admin_delete_permission(ADMIN_LOCAL, "local", LOCAL_CLIENT_INSTANCE, "test-echo2", "test/other")



def local_backend_policy_control_center():
    # ============================================================
    # LOCAL INSTANCE: backend-managed permission mode control
    # ============================================================

    # Remove the seed permission so we can prove "no permission → denied".
    admin_delete_permission(ADMIN_LOCAL, "local", LOCAL_CLIENT_INSTANCE, "test-echo", "test/token")
    machine.fail(su("/run/wrappers/bin/underwrap-local test-echo"))
    machine.log("PASS: wrapper with no permission is denied")

    # Unlocked permission → wrapper can read.
    set_local_unlocked_permission("test-echo")
    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo"))
    assert "MY_TOKEN=overwrite" in output, f"expected MY_TOKEN under Unlocked permission: {output}"
    machine.log("PASS: Unlocked permission allows wrapper read")

    # Locked permission → wrapper denied.
    admin_upsert_permission(
        ADMIN_LOCAL,
        "local",
        LOCAL_CLIENT_INSTANCE,
        "test-echo",
        "test/token",
        {"kind": "locked"},
        reason="maintenance",
    )
    machine.fail(su("/run/wrappers/bin/underwrap-local test-echo"))
    machine.log("PASS: Locked permission denies wrapper read")

    # Re-Unlock → wrapper allowed again.
    set_local_unlocked_permission("test-echo")
    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-echo"))
    assert "MY_TOKEN=overwrite" in output, f"expected MY_TOKEN after restoring Unlocked: {output}"
    machine.log("PASS: re-Unlocking the permission restores wrapper access")

    # A second wrapper becomes usable when its permission is Unlocked.
    set_local_unlocked_permission("test-locked")
    output = machine.succeed(su("/run/wrappers/bin/underwrap-local test-locked"))
    assert "LOCKED_TOKEN=locked_secret" in output, (
        f"expected Unlocked wrapper to work: {output}"
    )
    machine.log("PASS: active Unlocked permission admits the wrapper")


def instance_isolation():
    # ============================================================
    # INSTANCE ISOLATION: cross-instance access blocked
    # ============================================================

    # The local instance's `underwrap-local` entrypoint cannot reach tunnel
    # instance secrets: it connects only to the local instance socket
    # (gated by the local instance group egid) and the local daemon talks
    # only to the local backend.
    set_local_unlocked_permission("test-echo")
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
    set_local_unlocked_permission("test-echo")  # mode = Unlocked
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
    admin_upsert_permission(
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

    # The Debug variant must run under the SAME sandbox uid as test-echo's Real
    # target — the uid is keyed on (instance, wrapper), not the binary, so debug
    # (which brokers the same secret) shares the wrapper's uid and never a
    # host-wide debug-tool UID. The debug tool prints "uid: <n>".
    debug_uid = next(
        (l.split(":", 1)[1].strip() for l in out.splitlines() if l.startswith("uid:")),
        None,
    )
    assert debug_uid is not None, f"debug summary must report its uid, got: {out}"
    sandbox_user = machine.succeed(
        "awk -F'\\t' '$1==\"local\" && $2==\"test-echo\"{print $3}' "
        "/etc/underwrap/target-uids"
    ).strip()
    expected_uid = machine.succeed(f"id -u {sandbox_user}").strip()
    assert debug_uid == expected_uid, (
        f"debug ran as uid {debug_uid}, expected test-echo's sandbox uid "
        f"{expected_uid} ({sandbox_user})"
    )
    assert debug_uid != "0", "debug must not run as root"
    machine.log(
        f"PASS: Debug runs under test-echo's sandbox uid {expected_uid} "
        f"({sandbox_user}), not a shared debug-tool uid"
    )

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
    # permission as not-yet-expired and (wrongly for the fixture) allows the
    # Debug variant. Two minutes keeps the test outside whole-second truncation
    # and plausible host/VM skew.
    past = int(time.time()) - 120
    admin_upsert_permission(
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

    # Restore the fixture permission so later sections see test-echo Unlocked.
    set_local_unlocked_permission("test-echo")


def runtime_api_invariants():
    # ============================================================
    # RUNTIME API INVARIANTS
    # ============================================================
    #
    # Pins the three-endpoint runtime API's trust-boundary properties end to
    # end: fresh connections from the cached client, explicit credential
    # cutover, no storage-key route on the wire, a closed permission-mode enum,
    # and env validation on every finalize.
    # ============================================================

    # ----------------------------------------------------------
    # Backend restart self-heal — idle pooling is disabled, so the
    # daemon's cached reqwest client opens a fresh HTTPS connection on
    # the next wrapper call. This does not rebuild the client or reload
    # credential files.
    # ----------------------------------------------------------
    set_local_unlocked_permission("test-echo")
    wrapper_success_command = su(
        '/run/wrappers/bin/underwrap-local test-echo | grep -q "^MY_TOKEN="'
    )
    machine.succeed(wrapper_success_command)

    machine.succeed("systemctl restart underwrap-backend-local.service")
    machine.wait_for_unit("underwrap-backend-local.service")
    machine.wait_until_succeeds(wrapper_success_command)
    machine.log("PASS: cached runtime client reconnects after backend restart")

    # The in-memory `tower-sessions` store does not survive the
    # restart above, so the admin session captured at boot is gone.
    # Re-login before any admin API call after a backend restart.
    login_admin_session(ADMIN_LOCAL)

    # ----------------------------------------------------------
    # Runtime credential rotation requires daemon restart. A successful
    # keygen replaces the frontend identity file, but the running daemon keeps
    # its cached old identity until the operator restarts the matching service.
    # ----------------------------------------------------------
    stable_client_fields = (
        "id",
        "display_name",
        "current_store_id",
        "current_store_name",
        "status",
        "auto_create_wrapper_assets",
        "current_credential_fingerprint",
    )

    def local_client_snapshot():
        items = admin_api(
            ADMIN_LOCAL, "/v1/admin/client-instances", None, method="GET"
        )["items"]
        matches = [item for item in items if item["id"] == LOCAL_CLIENT_INSTANCE]
        assert len(matches) == 1, "expected exactly one active local client record"
        return {field: matches[0][field] for field in stable_client_fields}

    def daemon_main_pid():
        value = machine.succeed(
            "systemctl show underwrapd-local.service -p MainPID --value"
        ).strip()
        assert value.isdecimal() and int(value) > 0, "underwrapd-local has no MainPID"
        return int(value)

    machine.succeed(wrapper_success_command)
    client_before_rotation = local_client_snapshot()
    assert client_before_rotation["status"] == "active"
    old_daemon_pid = daemon_main_pid()

    replacement_cert_pem = machine.succeed(
        "underwrap-keygen --instance-config /etc/underwrap/daemon/local.yaml"
    )
    assert replacement_cert_pem.startswith("-----BEGIN CERTIFICATE-----\n")
    assert replacement_cert_pem.endswith("-----END CERTIFICATE-----\n")
    identity_metadata = machine.succeed(
        "stat -c '%U:%G %a' "
        "/var/lib/underwrap-local/client-local.identity.pem"
    ).strip()
    assert identity_metadata == "underwrap-local:underwrap-local 400", (
        "keygen installed unexpected identity ownership or mode"
    )

    assert daemon_main_pid() == old_daemon_pid
    machine.succeed(wrapper_success_command)
    assert daemon_main_pid() == old_daemon_pid

    admin_api(
        ADMIN_LOCAL,
        f"/v1/admin/client-instances/{_seg(LOCAL_CLIENT_INSTANCE)}",
        {"cert_pem": replacement_cert_pem},
        method="PUT",
    )
    client_after_rotation = local_client_snapshot()
    assert (
        client_after_rotation["current_credential_fingerprint"]
        != client_before_rotation["current_credential_fingerprint"]
    ), "credential rotation did not change the backend fingerprint"
    expected_after_rotation = dict(client_before_rotation)
    expected_after_rotation["current_credential_fingerprint"] = (
        client_after_rotation["current_credential_fingerprint"]
    )
    assert client_after_rotation == expected_after_rotation, (
        "credential-only update changed another stable client field"
    )

    assert daemon_main_pid() == old_daemon_pid
    enrollment_error = machine.fail(f"{wrapper_success_command} 2>&1")
    assert "this underwrap instance is not enrolled yet" in enrollment_error, (
        "cached old identity did not produce the existing enrollment guidance"
    )
    assert daemon_main_pid() == old_daemon_pid

    machine.succeed("systemctl restart underwrapd-local.service")
    machine.wait_until_succeeds(
        "pid=$(systemctl show underwrapd-local.service -p MainPID --value); "
        f"test \"$pid\" -gt 0 && test \"$pid\" -ne {old_daemon_pid}"
    )
    new_daemon_pid = daemon_main_pid()
    assert new_daemon_pid != old_daemon_pid
    machine.wait_until_succeeds(wrapper_success_command)
    machine.log("PASS: runtime credential rotation requires daemon restart")

    # ----------------------------------------------------------
    # Wrapper-permission upsert rejects an unsupported permission-mode variant.
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
        "PASS: admin wrapper-permissions upsert rejects an unsupported mode"
    )

    # ----------------------------------------------------------
    # An authenticated runtime mTLS client still receives 404 for guessed
    # storage-key/status routes outside the three-endpoint runtime surface.
    # ----------------------------------------------------------
    runtime_identity = "/var/lib/underwrap-local/client-local.identity.pem"

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
            "--cert", runtime_identity,
            "--key", runtime_identity,
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

    for forbidden_path in ("/v1/status", "/v1/read", "/v1/write", "/v1/list", "/v1/delete",
                           "/v1/read-prefixes", "/v1/runtime/wrappers/register"):
        _runtime_curl("POST", forbidden_path, body={}, expect_status=404)
    machine.log("PASS: non-runtime storage/status routes return 404 on mTLS")

    # ----------------------------------------------------------
    # Env validation through the full mTLS path: open returns the canonical
    # blob, and finalize rejects a writeback that mutates the env section even
    # when the permission is writable.
    # ----------------------------------------------------------
    admin_upsert_permission(
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
    assert open_resp["writable"] is True, "permission flipped to writable above"
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

    # Clean up the writable permission so later sections don't trip over
    # it; reset the env-only baseline by re-writing through the
    # admin API.
    admin_upsert_permission(
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

    canary_key = "AUDIT_LOG_CANARY"
    canary = f"underwrap-audit-canary-{uuid.uuid4().hex}".encode("ascii")
    admin_write_resource_value(
        ADMIN_LOCAL, "local", "test/token", "env", canary_key, canary
    )
    set_local_unlocked_permission("test-echo")
    machine.succeed(
        su("/run/wrappers/bin/underwrap-local test-echo >/dev/null 2>&1")
    )

    canonical = base64.b64decode(admin_get_blob(ADMIN_LOCAL, "local", "test/token"))
    canonical_text = canonical.decode("utf-8")
    canary_b64 = base64.b64encode(canary).decode("ascii")
    env_entry_signature = (
        f"{json.dumps(canary_key, ensure_ascii=True)}:"
        f"{json.dumps(canary_b64, ensure_ascii=True)}"
    )
    assert env_entry_signature in canonical_text, (
        "AUDIT canonical resource omitted the dedicated env-entry signature"
    )

    needle_labels = {}

    def add_needle(label, needle):
        assert len(needle) >= 16, f"AUDIT representation too short: {label}"
        needle_labels.setdefault(needle, []).append(label)

    def add_binary_encodings(prefix, value):
        standard = base64.b64encode(value).decode("ascii")
        urlsafe = base64.urlsafe_b64encode(value).decode("ascii")
        add_needle(f"{prefix}/base64-standard-padded", standard)
        add_needle(f"{prefix}/base64-standard-unpadded", standard.rstrip("="))
        add_needle(f"{prefix}/base64-url-padded", urlsafe)
        add_needle(f"{prefix}/base64-url-unpadded", urlsafe.rstrip("="))
        add_needle(f"{prefix}/hex-lower", value.hex())

    add_needle("canary/plaintext", canary.decode("ascii"))
    add_binary_encodings("canary", canary)
    add_needle("canonical/raw-utf8", canonical_text)
    add_needle(
        "canonical/json-escaped",
        json.dumps(canonical_text, ensure_ascii=True)[1:-1],
    )
    add_binary_encodings("canonical", canonical)
    add_needle("canonical/env-entry-signature", env_entry_signature)

    corpora = {}
    for unit in (
        "underwrap-backend.service",
        "underwrap-backend-local.service",
        "underwrapd-local.service",
        "underwrapd-tunnel.service",
    ):
        corpora[unit] = ANSI_ESCAPE_RE.sub(
            "",
            machine.succeed(f"journalctl -u {shlex.quote(unit)} --no-pager --all"),
        )
    namespace_corpus = "SYSLOG_IDENTIFIER=underwrap-ns"
    corpora[namespace_corpus] = ANSI_ESCAPE_RE.sub(
        "",
        machine.succeed("journalctl SYSLOG_IDENTIFIER=underwrap-ns --no-pager --all"),
    )

    for needle, labels in needle_labels.items():
        representation = "/".join(labels)
        for corpus_label, corpus in corpora.items():
            assert needle not in corpus, (
                f"AUDIT log hygiene failure: {representation} in {corpus_label}"
            )
    machine.log("PASS: dynamic canary absent from selected journal representations")

    logs = "\n".join(corpora.values())
    read_allowed_lines = [
        line for line in logs.splitlines() if 'audit="read_allowed"' in line
    ]
    assert read_allowed_lines, "missing read_allowed"
    assert any(" INFO " in line for line in read_allowed_lines), (
        "read_allowed was not emitted at INFO"
    )
    assert all('resource="' in line for line in read_allowed_lines), (
        "read_allowed omitted the resource field"
    )
    assert all("secret=" not in line for line in read_allowed_lines), (
        "read_allowed retained the legacy secret field"
    )
    assert all('"audit":"read_allowed"' not in line for line in read_allowed_lines), (
        "underwrapd audit output unexpectedly used JSON"
    )
    assert 'audit="read_granted"' not in logs, "legacy read_granted remained"

    assert "ghp_testvmtoken" not in logs, "secret leaked!"
    assert "locked_secret" not in logs, "secret leaked!"
    assert "secret_via_service" not in logs, "secret leaked!"
    assert "my_gh_token_value" not in logs, "libsecret secret leaked!"
    assert "libsecret_two_slot_value" not in logs, "two-slot secret leaked!"
    assert "iso_files_secret_val" not in logs, "isolated files secret leaked!"
    assert "iso_libsecret_val" not in logs, "isolated libsecret secret leaked!"
    publication_near_cap_prefix = b"publ-cap-secret-xx"
    publication_near_cap_value = publication_near_cap_prefix + b"x" * 3982
    assert publication_near_cap_prefix.decode() not in logs, "PUBL near-cap fixture leaked in plaintext!"
    assert base64.b64encode(publication_near_cap_value).decode() not in logs, (
        "PUBL complete near-cap item leaked in exact base64 form!"
    )

    size_values = {
        "env": SIZE_ENV_VALUE,
        "files-initial": SIZE_FILES_INITIAL,
        "files-replacement": SIZE_FILES_REPLACEMENT,
        "files-oversize": SIZE_FILES_OVERSIZE,
        "admin-oversize": SIZE_ADMIN_OVERSIZE,
    }
    for label, value in size_values.items():
        assert value.decode() not in logs, f"SIZE {label} fixture leaked in plaintext"
        assert base64.b64encode(value).decode() not in logs, (
            f"SIZE {label} fixture leaked in exact base64 form"
        )

    life_modes = (
        "seed",
        "success",
        "exit37",
        "stage1-kill",
        "host-kill",
        "relay-kill",
        "stage1-int",
        "stage1-term",
        "host-int",
        "host-term",
        "target-term",
        "target-kill",
        "late-dispatch",
    )
    life_values = [("initial-files", b"life-before")]
    for mode in life_modes:
        life_values.extend(
            (
                (f"files/{mode}", f"life-file-{mode}".encode()),
                (f"libsecret/{mode}", f"life-libsecret-{mode}".encode()),
            )
        )
    assert "life-file-" not in logs, "LIFE files fixture leaked in plaintext!"
    assert "life-libsecret-" not in logs, "LIFE libsecret fixture leaked in plaintext!"
    for label, value in life_values:
        plaintext = value.decode()
        encoded = base64.b64encode(value).decode()
        assert plaintext not in logs, f"LIFE {label} fixture leaked in plaintext"
        assert encoded not in logs, f"LIFE {label} fixture leaked in base64 form"
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
local_ns_handoff_authentication()
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
files_staging_provenance()
local_atomic_writeback()
pre_exec_writeback_gate()
supervisor_lifecycle()
resource_size_contract()
remote_service_lifecycle()
instance_isolation()
local_debug_subcommand()
local_backend_policy_control_center()
local_backend_managed_permissions()
plaintext_runtime_backend_url_is_rejected_before_io()
encrypted_store_in_memory_keys()
runtime_api_invariants()
audit_and_no_secret_leaks()

