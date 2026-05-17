# Story: Invite-first per-instance enrollment

Parent: `epic_dynamic_runtime_clients_enrollment_and_stores.md`

## Background

Manual certificate provisioning is too awkward, but we also do not want open CSR submission or automatic client registration. Enrollment should begin intentionally from the operator side after an unenrolled wrapper invocation points them to the correct page.

## Desired design

- A configured client instance can exist in an **unenrolled** state.
- When an unenrolled wrapper is executed, it prints:
  - a short explanation
  - a backend enrollment URL containing only non-sensitive hints (for example hostname and instance name)
- The operator opens the enrollment page, logs in, and chooses:
  - display name
  - existing store or create-new-store inline
- The backend creates an invite/enrollment record.
- The UI shows the exact enrollment command.
- A dedicated enrollment CLI/helper runs locally on the client host.
- The CLI prompts interactively for the enrollment secret instead of taking it in argv.
- The CLI generates the per-instance private key locally, creates the CSR locally, receives the signed cert, and writes files atomically.
- The backend decides the issued certificate contents.

## Acceptance criteria

- [ ] Unenrolled wrapper execution clearly guides the operator into enrollment.
- [ ] Enrollment begins from an operator-approved flow, not an open CSR inbox.
- [ ] Enrollment command uses interactive secret entry rather than argv-exposed secrets.
- [ ] Keypair generation stays local to the client instance.
- [ ] Enrollment can target an existing store or create a new one inline.
- [ ] Successful enrollment installs a per-instance key/cert pair and enables runtime access.
