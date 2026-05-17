# Task: Add the backend invite and certificate-issuance flow

Parent: `330_story_invite_first_instance_enrollment.md`

## Scope

Add backend support for invite-first enrollment:

- enrollment/invite records
- operator-approved enrollment start
- CSR submission handling
- backend-controlled certificate issuance

## Acceptance criteria

- [ ] Backend can create and persist invite/enrollment records.
- [ ] Enrollment begins from an operator-approved flow, not an open CSR inbox.
- [ ] Backend issues the certificate from the submitted CSR while controlling the issued identity fields.
