# Reporting vulnerabilities

Report suspected vulnerabilities through [GitHub private vulnerability reporting](https://github.com/rschaffar/underwrap/security/advisories/new).
Do not disclose exploit details in a public issue or pull request before coordinating with the maintainer.
If the private form is unavailable, request a confidential contact through a public issue without describing the vulnerability or including sensitive material.

Include the affected version or source revision, frontend/backend platform, relevant configuration with values redacted, expected and observed behavior, and a minimal reproduction using synthetic data.
Explain which asset and trust boundary are affected.
Never submit live credentials, resource contents, private keys, session cookies, or unredacted diagnostic archives.

Underwrap is pre-release software without an independent end-to-end security audit.
There is no stable-release support window or guaranteed response deadline.
A report or fix does not establish that other paths are free of vulnerabilities.

The [security model](docs/security.md) defines the caller/target boundary, trusted components, supported posture, and limitations.
Operational failures and recovery prerequisites are documented in [Operations](docs/operations.md).
This reporting channel is not an emergency recovery or key-custody service.
