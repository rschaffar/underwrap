# Task: Build the dedicated client enrollment CLI

Parent: `330_story_invite_first_instance_enrollment.md`

## Scope

Build the per-instance enrollment helper/CLI:

- prompt interactively for the enrollment secret
- generate the private key locally
- generate the CSR locally
- submit to the backend
- write returned cert/key material atomically

## Acceptance criteria

- [ ] Enrollment secret entry is interactive rather than argv-exposed.
- [ ] Keypair generation happens locally on the client instance.
- [ ] The CLI can install the returned credential atomically.
