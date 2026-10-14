# RF-0002 — Dynamic-debug SPA fallback can disclose an outside symlink target

**Priority:** High (conditional) · **State:** archived · **Resolution:** fixed

**Review aliases:** `B-01` · **Confidence:** High: regression fails before the fix; debug/release handler and disposable HTTPS controls pass after it.

## Why it matters

A development backend could send an outside file to someone who had not logged in, if a suitable symlink existed and its target was readable.

## Fix and caveat

Website files are now bundled into every backend build, including development builds.
Website requests no longer look up files on the server's disk.
Developers must rebuild the backend after rebuilding the website; normal release behavior is unchanged.

Fixed in `b899a0be47090977084808a4c23b203b8d23876e`.
Checks used harmless synthetic files and disposable local servers, not real private files or existing deployments.
Files deliberately included by the trusted build remain public.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
