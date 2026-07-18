#!/usr/bin/env python3
"""Emit a content-hash manifest for the current underwrap review scope."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
from typing import Any


def git(root: Path, *args: str) -> bytes:
    return subprocess.check_output(["git", "-C", str(root), *args])


def repository_root() -> Path:
    try:
        return Path(git(Path.cwd(), "rev-parse", "--show-toplevel").decode().strip())
    except (subprocess.CalledProcessError, UnicodeDecodeError) as error:
        raise SystemExit(f"not inside a Git repository: {error}") from error


def normalize_scope(root: Path, values: list[str]) -> list[str]:
    if not values:
        return ["."]

    result: list[str] = []
    for value in values:
        candidate = Path(value)
        absolute = candidate.resolve() if candidate.is_absolute() else (Path.cwd() / candidate).resolve()
        try:
            relative = absolute.relative_to(root.resolve())
        except ValueError as error:
            raise SystemExit(f"scope path is outside repository: {value}") from error
        result.append(relative.as_posix() or ".")
    return sorted(set(result))


def hash_regular_file(path: Path, before: os.stat_result) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    after = path.lstat()
    identity_before = (before.st_ino, before.st_mode, before.st_size, before.st_mtime_ns)
    identity_after = (after.st_ino, after.st_mode, after.st_size, after.st_mtime_ns)
    if identity_before != identity_after:
        raise RuntimeError(f"file changed while hashing: {path}")
    return digest.hexdigest()


def describe(root: Path, relative: str) -> dict[str, Any]:
    path = root / relative
    try:
        before = path.lstat()
    except FileNotFoundError:
        return {"path": relative, "type": "missing"}

    mode = stat.S_IMODE(before.st_mode)
    if stat.S_ISREG(before.st_mode):
        return {
            "path": relative,
            "type": "file",
            "mode": f"{mode:04o}",
            "size": before.st_size,
            "sha256": hash_regular_file(path, before),
        }
    if stat.S_ISLNK(before.st_mode):
        target = os.readlink(path)
        after = path.lstat()
        if (before.st_ino, before.st_mode, before.st_mtime_ns) != (
            after.st_ino,
            after.st_mode,
            after.st_mtime_ns,
        ):
            raise RuntimeError(f"symlink changed while hashing: {path}")
        return {
            "path": relative,
            "type": "symlink",
            "mode": f"{mode:04o}",
            "sha256": hashlib.sha256(os.fsencode(target)).hexdigest(),
        }
    raise RuntimeError(
        f"unsupported non-regular scope entry (exclude it explicitly): {path}"
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Hash tracked and non-ignored untracked files in a review scope. "
            "File contents are never emitted."
        )
    )
    parser.add_argument("scope", nargs="*", help="repository-relative paths; default: whole repository")
    args = parser.parse_args()

    root = repository_root()
    scope = normalize_scope(root, args.scope)
    command = ["ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", *scope]
    raw_paths = git(root, *command)
    paths = sorted({os.fsdecode(item) for item in raw_paths.split(b"\0") if item})

    try:
        entries = [describe(root, path) for path in paths]
    except (OSError, RuntimeError) as error:
        print(str(error), file=sys.stderr)
        return 2

    manifest = {
        "version": 1,
        "scope": scope,
        "entries": entries,
    }
    json.dump(manifest, sys.stdout, sort_keys=True, separators=(",", ":"))
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
