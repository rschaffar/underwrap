# RF-0019 — A held peer pidfd does not pin numeric procfs lookup

**Priority:** Low · **State:** open

**Review aliases:** `D-01` · **Confidence:** High on the peer-binding contradiction; Low demonstrated supported-product impact, with no authorization bypass established.

## Why it matters

Peer death/reaping and numeric PID reuse can make the daemon read another process's credentials despite holding the original pidfd.
The race-free explanation is false.

## Suggested fix

Establish original-peer binding through credential lookup, retain fail-closed handling and the effective-GID gate, and correct the numeric-pinning claim.

## Decision and caveat

Open, Low demonstrated impact.
The final bounded disposition supersedes the original Medium proposal; no fix is implemented or authorized.
A useful bypass additionally needs an admitted, retained client transport.
The shipped socket DAC and sole internally created entry endpoint remain independent barriers; no supported-chain resource authority or secret disclosure was demonstrated.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
