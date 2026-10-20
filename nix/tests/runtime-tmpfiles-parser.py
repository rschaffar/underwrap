"""Round-trip real module rules through tmpfiles without any activation."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

cases = json.loads(Path(sys.argv[1]).read_text())
output = Path(sys.argv[2])
for name, case in cases.items():
    base = case["base"]
    daemon = json.loads(Path(case["daemonConfig"]).read_text())
    assert daemon["target_runtime_dir_base"] == base, name
    instance, wrapper, user, group = case["targetTable"].strip().split("\t")
    assert (instance, wrapper, user) == ("probe", "files", group), name
    child = base.rstrip("/") + "/" + user
    # Isolate the base and target rules from unrelated NixOS/daemon rules.
    rules = [rule for rule in case["rules"] if f" 0700 {user} {group} -" in rule]
    base_rules = [rule for rule in case["rules"] if rule.endswith(" 0755 root root -")]
    assert len(rules) == 1, name
    # Other root-owned rules exist. The base rule directly precedes its child.
    index = case["rules"].index(rules[0])
    base_rule = case["rules"][index - 1]
    assert base_rule in base_rules, name
    rules.insert(0, base_rule)
    with tempfile.TemporaryDirectory(prefix="tmpfiles-literal-") as directory:
        root = Path(directory)
        (root / "etc").mkdir()
        (root / "etc/passwd").write_text(f"root:x:0:0::/:/bin/false\n{user}:x:12345:12345::/:/bin/false\n")
        (root / "etc/group").write_text(f"root:x:0:\n{group}:x:12345:\n")
        result = subprocess.run(
            ["systemd-tmpfiles", "--dry-run", "--create", f"--root={root}", "--inline", *rules],
            text=True, capture_output=True,
            env={**os.environ, "SYSTEMD_LOG_LEVEL": "debug", "LC_ALL": "C.UTF-8"},
        )
        log = result.stdout + result.stderr
        assert result.returncode == 0, f"{name}: tmpfiles parser rejected generated rules: {log}"
        prefix = "Running create action for entry d "
        selected = [line.removeprefix(prefix) for line in log.splitlines() if line.startswith(prefix)]
        expected = [str(root) + path for path in (base, child)]
        assert sorted(selected) == sorted(expected), f"{name}: parser-selected paths differ: {selected!r} != {expected!r}"
        # A successful dry-run must not have provisioned even the permitted control.
        assert not (root / child.lstrip("/")).exists(), name
        (output / f"{name}.log").write_text(log)
    (output / f"{name}.rules").write_text("\n".join(rules) + "\n")
    print(f"PASS: {name}: literal daemon/base/child parser paths agree (dry-run only)")
