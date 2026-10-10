# RF-0008 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `W-02`.
Original review owners: `web.md`, `web-probes.mjs`, `web-probes.log`, `web-command-results.json`, `report.md`, `finding-index.md`, `input-ledger.md`, `claim-evidence-join.md`, `substantive-inputs.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Reviewed baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The application paths are listed in the technical detail.
The inspected installed dependency was `web/node_modules/openapi-fetch/src/index.js:120-122,577-612`; this is dependency evidence, not a tracked application file.

## Recorded observations

The prior specialist's external combined W-02/W-03 probe ran under Node v22.23.3, exited 0 in 0.82635 seconds, and recorded zero network requests.
It transpiled the actual helper in memory and used the installed client with intercepted fetch.
It did not import and execute the application's endpoint wrappers or contact an admin backend.
Exit 0 means the defect assertions reproduced.

Using placeholders instead of the original fixture identifiers, the observed suffixes were:

- Collection DELETE, `cid=.`: `/libsecret/collections/`.
- Collection DELETE, `cid=..`: `/libsecret/`.
- Ordinary collection-ID control: `/libsecret/collections/{cid}`.
- Item-secret GET, ordinary collection and `iid=..`: `/libsecret/collections/{cid}/secret`.

The real helper accepted both dot-only inputs.
Existing endpoint tests used ordinary identifiers and did not cover these cases.

## Reproduction or verification procedure

This is a normalized reconstruction of the prior non-network methodology, not a newly executed command or a claim that its one-off script is tracked.

1. Use the matching checkout and installed locked web dependencies; load/transpile the actual ID helper.
2. Evaluate the two dot-only IDs with `optionalLibsecretIdField().safeParse`.
3. Instantiate the installed client with a non-network fetch replacement; call the collection DELETE and item-secret GET templates above.
4. Compare captured Request pathnames with an ordinary-ID control, recording only IDs/path structure and decision metadata.

## Limits and missing evidence

Node v22.23.3 was the probe runtime, not the declared Node 24 build baseline.
Item `.` and a real browser/backend response were not separate executed cases.
Backend acceptance and save behavior were source-inspected.
No new probe, live service access or fix verification occurred during migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
