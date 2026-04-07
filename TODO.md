# underwrap — TODO

## Future work

- [ ] `underwrapctl reauth` workflow for re-doing OAuth browser flows
- [ ] SSH-agent challenge auth for unlock
- [ ] Self-service unlock for non-root users (root can unlock for them today)
- [ ] stdin and tempfile injection modes in `underwrap-exec`
- [ ] Secret metadata / versioning
- [ ] Encrypted backend: passphrase-derived key via Argon2 (currently plaintext key file)
- [ ] Hardware-assisted unseal: TPM, YubiKey
- [ ] External secret providers via exec-based commands behind `StorageBackend` trait
- [ ] sops / age import pipeline
- [ ] Home Manager helpers (scope setup, completions)
- [ ] Generic Linux packaging (deb, rpm)
