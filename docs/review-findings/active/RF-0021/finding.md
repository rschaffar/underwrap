# RF-0021 — Post-Armed wording omits the initialized-generation baseline

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`docs/local-protocol.md:99-101` says that death after Armed but before exec/failure observation can finalize at most an already validated, fully materialized “unchanged projection.” An empty libsecret section, or one lacking provider defaults, is initialized with a default collection/alias before generation zero and readiness (`crates/underwrap-secretsd/src/section_store.rs:58-78`; `crates/underwrap-secretsd/src/main.rs:155-160`).
That baseline can already differ from Checkout before Armed (`crates/underwrap-ns/src/main.rs:614-616`).

## Consequence and boundary

Within the expressly documented narrow Armed-to-exec ambiguity, accepted gate/status/relay/publication evidence can select initialized state even if the configured target never executed.
Composition compares it with original Checkout (`crates/underwrap-ns/src/writeback.rs:104-149`); actual Checkin still requires changed bytes and writable authorization.
Calling this state “unchanged” without saying relative to initialization can mislead maintainers/operators.

This is wording precision, not newly discovered arbitrary partial-state acceptance.
Keep the distinction between authorized lifecycle and successful exec, validated/ACK-written generation and publisher-observed ACK, and either ACK and durable backend persistence.

## Correction and alternatives

Describe the selectable state as the fully validated initialized generation/projection, which may include provider defaults absent from Checkout.
Preserve intentional initialization and the documented ambiguity; suppressing default initialization or redesigning the exec gate is not required by this finding.

## Acceptance criteria

- Wording explicitly identifies the initialization baseline and does not promise Checkout byte equality.
- A focused empty-libsecret qualification test should distinguish initialized generation zero from Checkout and model death after Armed/before exec, retaining every other lifecycle and writeback gate.
- Keep malformed/retained-writer/failed-gate suppression and avoid describing acceptance as proof that the target executed.
  No such new test or wording fix is claimed complete.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
