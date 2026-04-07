# underwrap — TODO

## Future work

- [ ] stdin and tempfile injection modes in `underwrap-exec`
- [ ] `underwrapctl reauth` workflow for OAuth credential refresh
- [ ] SSH-agent challenge auth for unlock
- [ ] Self-service unlock for non-root users (root can unlock for them today)
- [ ] Secret metadata / versioning
- [ ] Home Manager helpers (scope setup, completions)
- [ ] Generic Linux packaging (deb, rpm)
- [ ] Encrypted backend: passphrase-derived key via Argon2 (currently plaintext key file)
- [ ] Hardware-assisted unseal: TPM, YubiKey
- [ ] External secret providers via exec-based commands behind `StorageBackend` trait
- [ ] sops / age import pipeline
