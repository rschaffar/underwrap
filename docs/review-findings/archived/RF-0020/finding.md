# RF-0020 — Cleanup dialog says YAML was removed instead of a registry record

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

Every deletion returning a cleanup hint reaches `web/src/pages/stores/dialogs/cleanup-hint.tsx:29-30`, which says that a YAML definition was removed.
The backend instead removes a dynamic store registry record at `crates/underwrap-backend/src/admin_api/stores/crud.rs:591-599`, using the registry deletion path at `crates/underwrap-backend/src/store_registry.rs:307-312`.
It does not edit YAML.

## Consequence and boundary

The message can misdirect configuration or recovery reconciliation.
Maintained owners identify JSON registry records as store-definition authority (`docs/backend.md:24-26,208`; `docs/configuration.md:210-213`).
This is Low operator-guidance debt, not an authorization, persistence or confidentiality defect.

Registry-only deletion remains intentional: canonical data and retained generations are not automatically erased.
The separate unquoted command in this dialog is [RF-0005](../RF-0005/README.md); correcting this sentence alone would not resolve command safety.

## Correction and alternatives

Describe removal of the store registry record, retaining clear warnings about remaining data and retained generations.
Prefer terminology shared with Backend and Configuration.
Do not imply that deletion edits source configuration, removes backing data or performs secure erasure.

## Acceptance criteria

- Inspect/render a representative cleanup-hint response and verify it names registry removal rather than YAML.
- Retain the manual-cleanup and retained-generation warnings.
- Compare the message against the backend's actual unlink-only effect and maintained owner terminology.
  Verification follows the shared work owner below.

## Work ownership

The user-authorized joint correction uses [RF-0005's scope/evidence checklist](../RF-0005/finding.md#work-ownership) as its single implementation owner.
This finding retains its separate wording acceptance criteria and disposition.
Implementation `63a68753da4436e33c040dbc01302ac5e3a7f975` satisfies those criteria through the component and real/synthetic desktop Chromium checks in the [resolution evidence](evidence.md#resolution-evidence).
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
