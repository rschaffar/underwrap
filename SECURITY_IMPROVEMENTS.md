# underwrap — security improvements

This document describes two categories of security improvement:

1. **Mitigating `/proc/<pid>/environ` exposure** — preventing same-UID
   processes from reading injected secrets out of the child's environment.
2. **Per-tool user isolation** — running each wrapped tool under a dedicated
   system user so it cannot access the calling user's files, other tools'
   processes, or other tools' secrets.

Both are additive — they do not change the protocol, storage, derivation,
unlock, or audit subsystems. The per-tool user model also solves the open
wrapper identity spoofing problem (CONCEPT.md, open question #1).

## Table of contents

- [Part 1: /proc/\<pid\>/environ mitigation](#part-1-procpidenviron-mitigation)
  - [Background](#background)
  - [Improvement A: PR_SET_DUMPABLE (already implemented)](#improvement-a-pr_set_dumpable)
  - [Improvement B: memfd_create fd injection mode](#improvement-b-memfd_create-fd-injection-mode)
  - [Improvement C: pipe/stdin injection mode](#improvement-c-pipestdin-injection-mode)
  - [Improvement D: tempfile injection with O_TMPFILE](#improvement-d-tempfile-injection-with-o_tmpfile)
  - [Improvement E: system-level recommendations](#improvement-e-system-level-recommendations)
- [Part 2: per-tool user isolation](#part-2-per-tool-user-isolation)
  - [Motivation](#motivation)
  - [Mechanism](#mechanism)
  - [Security properties](#security-properties)
  - [Implementation: NixOS module changes](#implementation-nixos-module-changes)
  - [Implementation: underwrap-exec changes](#implementation-underwrap-exec-changes)
  - [Implementation: daemon policy changes](#implementation-daemon-policy-changes)
  - [Implementation: per-wrapper sockets (optional, recommended)](#implementation-per-wrapper-sockets)
  - [Implementation: protocol changes](#implementation-protocol-changes)
  - [Worked example](#worked-example)
  - [Limitations and edge cases](#limitations-and-edge-cases)

---

## Part 1: /proc/\<pid\>/environ mitigation

### Background

When `underwrap-exec` injects a secret via an environment variable and then
`exec()`s the target tool, the secret appears in `/proc/<pid>/environ` of the
child process for the duration of its lifetime.

On Linux, `/proc/<pid>/environ` is mode `0400` and owned by the process's
effective UID. This means:

- **Same-UID processes** can open and read it.
- **Root** can always read it.
- Other users **cannot** read it.

Additionally, with Yama `ptrace_scope=0` (classic mode), same-UID processes
can `PTRACE_ATTACH` and read arbitrary process memory. With `ptrace_scope=1`
(restricted, the common default), only ancestor processes can ptrace.

The practical threat: a compromised same-user process (e.g., a malicious
dependency, an unsandboxed AI agent, a browser extension's native helper)
reads `/proc/<pid>/environ` of the wrapped tool and extracts the secret.

### Improvement A: PR_SET_DUMPABLE

**Status: already implemented** (see `underwrap-exec/src/main.rs`, the
`pre_exec` hook).

**Mechanism:** Before exec, the wrapper calls `prctl(PR_SET_DUMPABLE, 0)` in a
`pre_exec` hook. This causes the kernel to set `/proc/<pid>/*` ownership to
`root:root`, which:

- Blocks same-UID processes from reading `/proc/<pid>/environ`.
- Blocks ptrace attach from same-UID non-root processes, even with
  `ptrace_scope=0`.
- Prevents core dumps that might contain the secret.

**What it does not protect against:** Root can still read the process's memory.
The child process itself could call `prctl(PR_SET_DUMPABLE, 1)` to undo the
protection, but a compromised child already has the secret in its own memory.

**No further action needed** — this is already in the codebase.

### Improvement B: memfd_create fd injection mode

**Status: not yet implemented.**

**Mechanism:** Instead of setting an environment variable, the wrapper:

1. Calls `memfd_create("underwrap", MFD_CLOEXEC)` to create an anonymous
   in-memory file.
2. Writes the secret into it.
3. Optionally seals it with `F_SEAL_WRITE | F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_SEAL`
   to prevent modification.
4. Clears `FD_CLOEXEC` on this fd so it survives exec (or dup2s it to a
   well-known fd number).
5. Passes the fd number to the target tool. Two sub-modes:
   - **fd-path mode:** pass `/proc/self/fd/N` as a file path argument
     (e.g., `--token-file=/proc/self/fd/3`). Works with tools that accept a
     file path for credentials.
   - **fd-env mode:** set an env var to just the fd number (e.g.,
     `SECRET_FD=3`). Requires the target tool to support fd-based reading.

**Why this is better than env injection:**

- The secret never appears in `/proc/<pid>/environ` at all.
- The memfd has no filesystem entry — cannot be found by directory scanning.
- The memfd memory is never swapped.
- Combined with `PR_SET_DUMPABLE=0`, the fd is completely inaccessible to
  same-UID processes (they cannot read `/proc/<pid>/fd/3`).

**Applicability:** Works for tools that accept `--token-file`, `--key-file`,
`--credentials-file`, or similar arguments. Many tools do: `gcloud`, `vault`,
`curl -H @file`, etc. For tools that accept `/dev/stdin`, stdin injection
(Improvement C) is simpler.

**Implementation location:** `crates/underwrap-exec/src/main.rs` and
`crates/underwrap-protocol/src/config.rs`.

#### Changes to config.rs

Add new variants to `InjectionMode`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode")]
pub enum InjectionMode {
    /// Set an environment variable.
    #[serde(rename = "env")]
    Env {
        #[serde(default = "default_env_var")]
        variable: String,
    },
    /// Pass secret via an anonymous in-memory file descriptor.
    /// The target tool receives a path like /proc/self/fd/N.
    #[serde(rename = "fd")]
    Fd {
        /// The CLI argument template. Use {fd_path} as placeholder.
        /// Example: "--token-file={fd_path}"
        /// If absent, the fd path is set in an env var instead.
        #[serde(default)]
        arg_template: Option<String>,
        /// Env var to set to the fd path (default: UNDERWRAP_SECRET_FILE).
        #[serde(default = "default_fd_env_var")]
        variable: String,
    },
    /// Pipe secret to the target's stdin.
    #[serde(rename = "stdin")]
    Stdin,
    /// Write secret to a temporary file, pass the path.
    #[serde(rename = "tempfile")]
    Tempfile {
        /// The CLI argument template. Use {path} as placeholder.
        #[serde(default)]
        arg_template: Option<String>,
        /// Env var to set to the file path (default: UNDERWRAP_SECRET_FILE).
        #[serde(default = "default_fd_env_var")]
        variable: String,
    },
}
```

#### Changes to underwrap-exec/src/main.rs

For the `Fd` mode, before building the child `Command`:

```rust
// Pseudocode for fd injection:
let fd = unsafe { libc::memfd_create(b"underwrap\0".as_ptr() as _, libc::MFD_CLOEXEC) };
if fd < 0 { bail!("memfd_create failed"); }

// Write secret.
use std::os::unix::io::FromRawFd;
let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
file.write_all(material.as_bytes())?;
file.seek(std::io::SeekFrom::Start(0))?;

// Seal it (prevent modification by child).
unsafe {
    libc::fcntl(fd, libc::F_ADD_SEALS,
        libc::F_SEAL_WRITE | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_SEAL);
}

// Clear CLOEXEC so it survives exec. Pick a stable fd number.
let target_fd = 3; // or next available
unsafe { libc::dup2(fd, target_fd); }
// dup2 result does NOT inherit CLOEXEC, so target_fd survives exec.

let fd_path = format!("/proc/self/fd/{target_fd}");
// Then either:
//   cmd.arg(arg_template.replace("{fd_path}", &fd_path))
// or:
//   cmd.env(&variable, &fd_path)
```

#### YAML config example

```yaml
wrappers:
  gws-castlabs:
    target: /usr/bin/gws
    secret: google/castlabs
    injection:
      mode: fd
      variable: GOOGLE_WORKSPACE_CLI_TOKEN_FILE
    derivation: oauth_refresh_to_access_token
```

Or with an arg template for tools that take a file path argument:

```yaml
wrappers:
  vault-reader:
    target: /usr/bin/vault
    secret: vault/token
    injection:
      mode: fd
      arg_template: "-token-file={fd_path}"
```

### Improvement C: pipe/stdin injection mode

**Status: not yet implemented (in TODO.md).**

**Mechanism:** The wrapper creates a `pipe()`, writes the secret into the write
end, closes the write end (so the child sees EOF after reading the secret), and
connects the read end to the child's stdin via `dup2(read_fd, 0)`.

**Implementation:** In `underwrap-exec`, for `InjectionMode::Stdin`:

```rust
let (read_fd, write_fd) = pipe()?;
write_all(write_fd, material.as_bytes())?;
close(write_fd);

// In pre_exec or via Command::stdin:
cmd.stdin(unsafe { std::process::Stdio::from_raw_fd(read_fd) });
```

**Applicability:** Works for tools that read credentials from stdin.
Example: `echo "$TOKEN" | some-tool --token-from-stdin`.

**Limitation:** Uses stdin, which some tools need for other purposes. The fd
injection mode (Improvement B) is more general.

### Improvement D: tempfile injection with O_TMPFILE

**Status: not yet implemented (in TODO.md).**

**Mechanism:** The wrapper creates a temp file using `O_TMPFILE` (Linux 3.11+,
linkat-less — no directory entry at all) or falls back to `mkstemp` + `unlink`.
Writes the secret, seeks to 0, passes the fd to the child via
`/proc/self/fd/N` (same as Improvement B).

This is essentially the same as Improvement B but uses a filesystem-backed temp
file instead of a memfd. The main reason to have this as a separate mode is for
systems where `memfd_create` is unavailable (pre-3.17 kernels), though this is
increasingly rare.

**Recommendation:** Implement Improvement B (memfd) first. Tempfile becomes a
fallback.

### Improvement E: system-level recommendations

These are not code changes but should be documented in the README and/or
generated by the NixOS module.

#### hidepid=2 on /proc

Mount `/proc` with `hidepid=invisible` (or `hidepid=2`). This makes each
user's `/proc/<pid>` directory invisible to other users. It does **not** help
within the same UID, but it complements the per-tool user isolation (Part 2)
where each tool runs as a different user.

The NixOS module should optionally set this:

```nix
# In the underwrap NixOS module, as an opt-in option:
boot.specialFileSystems."/proc".options = [ "hidepid=2" "gid=${toString config.ids.gids.proc}" ];
```

#### Yama ptrace_scope

Document that `ptrace_scope=1` (restricted, the common default on most
distributions) is strongly recommended. The NixOS module could assert or warn
if a weaker setting is detected.

#### memfd_secret for daemon memory

On Linux 5.14+, the daemon could hold decrypted secret material in
`memfd_secret(2)` pages, which are removed from the kernel's own page tables.
This protects against kernel memory access (crash dumps, cold boot attacks on
kernel memory). This is a future hardening for the daemon's in-memory secret
storage, not for the injection path. It should not block any other work.

---

## Part 2: per-tool user isolation

### Motivation

Currently, when `gws-castlabs` execs the `gws` binary, that process runs as
the calling user (e.g., `robert`). It has the injected secret **and** full
read access to:

- `~robert/.ssh/` (SSH keys)
- `~robert/.gnupg/` (GPG keys)
- `~robert/.config/` (other application credentials)
- `~robert/projects/` (source code, other repos)
- Other tools' `/proc/<pid>/environ` (if same UID, without PR_SET_DUMPABLE)
- The underwrap daemon socket (could request other wrappers' secrets)

If the target tool or any of its dependencies is compromised, the attacker has
robert's full local identity.

For tools that are essentially **network API clients** — they take a credential,
make HTTP requests, and return output on stdout — this is far more access than
they need. Examples:

- `gws` (Google Workspace CLI)
- `gh` (GitHub CLI, when only doing API operations)
- `bx` (web search CLI)
- `slack-cli` (Slack API)
- OAuth token exchange tools

These tools need: the secret, network access, stdout/stderr. They do **not**
need access to the user's files.

### Mechanism

For each wrapper that opts in, create a dedicated system user. The wrapper
binary is a **copy** (not symlink — setuid on symlinks is ignored on Linux)
of `underwrap-exec`, owned by the tool user, with the **setuid bit** set.

```
/run/wrappers/bin/gws-castlabs
  owner:    u-gws-castlabs:u-gws-castlabs
  mode:     4755 (setuid, world-executable)
  content:  copy of the underwrap-exec binary
```

When the calling user (robert) executes this binary:

1. Kernel sets `euid = u-gws-castlabs` (from setuid bit on the file).
   `ruid = robert` (preserved).
2. `underwrap-exec` detects it is elevated (`euid != ruid`).
3. It loads the wrapper sidecar config from the trusted fixed path
   `/etc/underwrap/wrappers/gws-castlabs.yaml` (the `UNDERWRAP_CONFIG_DIR` env
   var is ignored under elevation — this is already implemented).
4. It connects to the daemon's read socket.
5. The daemon calls `getsockopt(SO_PEERCRED)` and sees `euid = u-gws-castlabs`.
6. Daemon policy: UID `u-gws-castlabs` may read secret `google/castlabs` → grants.
7. The wrapper receives the secret material.
8. **Instead of dropping back to robert**, the wrapper **commits to the tool
   user identity**:
   - `setgroups(0, NULL)` — clear supplementary groups.
   - `setgid(egid)` — set real GID = effective GID = `u-gws-castlabs`.
   - `setuid(euid)` — set real UID = effective UID = `u-gws-castlabs`.
     This is **irreversible** — the process cannot regain robert's UID or root.
9. Injects the secret (env, fd, pipe — any mode).
10. Sets `PR_SET_DUMPABLE=0` (in pre_exec hook).
11. Execs the target tool (`/usr/bin/gws`).
12. The target tool runs fully as `u-gws-castlabs`. stdout/stderr are still
    connected to robert's terminal (file descriptors survive setuid + exec).

**Root is never involved at runtime.** Root is only needed at **system
activation time** (NixOS module rebuild) to create the user, copy the binary,
chown it, and set the setuid bit.

### Security properties

| Attack vector | Without per-tool user | With per-tool user |
|---|---|---|
| Compromised tool reads `~robert/` | Full access — same UID | **Denied** — different UID |
| Compromised tool reads other tool's `/proc/<pid>/environ` | Possible — same UID | **Denied** — different UIDs |
| Compromised tool connects to daemon socket, requests other secrets | Possible — spoofs wrapper name | **Denied** — wrong UID, daemon rejects |
| `robert`'s processes read tool's `/proc/<pid>/environ` | Possible — same UID | **Denied** — different UID (plus PR_SET_DUMPABLE=0) |
| Attacker creates fake setuid binary for `u-gws-castlabs` | N/A | **Impossible** — only root can chown + set setuid on files, and `u-gws-castlabs` is a nologin system user |

**This also solves the wrapper identity spoofing problem.** Currently, the
daemon trusts a `wrapper` name string in the `ReadRequest` message — any
process that can connect to the read socket can claim to be any wrapper. With
per-tool users, wrapper identity **is the peer UID**, verified by the kernel
via `SO_PEERCRED`. The daemon does not need to trust any client-supplied
string. The UID is unforgeable by non-root processes.

### Implementation: NixOS module changes

**File:** `nix/modules/underwrap.nix`

#### New option on wrapperSubmodule: `runAs`

```nix
runAs = lib.mkOption {
  type = lib.types.nullOr lib.types.str;
  default = null;
  description = ''
    If set, the wrapper runs the target tool as this dedicated system user
    instead of the calling user. The NixOS module will:
    - Create the system user (nologin, home=/var/empty).
    - Copy underwrap-exec as a setuid binary owned by this user.
    - Configure daemon policy to allow this UID to read the wrapper's secret.
    A value of null (default) preserves the current behavior: setgid wrapper,
    target runs as the calling user.
  '';
  example = "u-gws-castlabs";
};
```

#### Auto-create system users

For each wrapper where `runAs` is set, generate a system user:

```nix
users.users = lib.mkMerge (lib.mapAttrsToList (name: w:
  lib.optionalAttrs (w.runAs != null) {
    ${w.runAs} = {
      isSystemUser = true;
      group = w.runAs;
      home = "/var/empty";
      shell = "/run/current-system/sw/bin/nologin";
      description = "underwrap tool user for ${name}";
    };
  }
) cfg.wrappers);

users.groups = lib.mkMerge (lib.mapAttrsToList (_name: w:
  lib.optionalAttrs (w.runAs != null) {
    ${w.runAs} = {};
  }
) cfg.wrappers);
```

#### Generate setuid wrapper copies

The current module creates symlinks:

```nix
ln -s ${cfg.package}/bin/underwrap-exec $out/bin/${name}
```

For `runAs` wrappers, the module must instead produce **copies** with the
setuid bit set. NixOS has a built-in mechanism for this: `security.wrappers`.
The underwrap module should generate entries in `security.wrappers` for each
`runAs` wrapper.

Alternatively (and likely simpler), the module can use a systemd-tmpfiles rule
or activation script that copies the binary and sets permissions:

```nix
# For each runAs wrapper, in a system activation script:
system.activationScripts.underwrap-wrappers = lib.mkIf cfg.enable (
  lib.concatStringsSep "\n" (lib.mapAttrsToList (name: w:
    lib.optionalString (w.runAs != null) ''
      # Copy the wrapper binary (setuid on symlinks is ignored on Linux).
      install -m 4755 -o ${w.runAs} -g ${w.runAs} \
        ${cfg.package}/bin/underwrap-exec \
        /run/wrappers/bin/${name}
    ''
  ) cfg.wrappers)
);
```

However, the **recommended approach** is to use NixOS's `security.wrappers`
mechanism, which is specifically designed for this purpose and handles the
`/run/wrappers/bin/` path, nosuid mount avoidance, and activation ordering:

```nix
security.wrappers = lib.mkMerge (lib.mapAttrsToList (name: w:
  lib.optionalAttrs (w.runAs != null) {
    ${name} = {
      source = "${cfg.package}/bin/underwrap-exec";
      owner = w.runAs;
      group = w.runAs;
      setuid = true;
      # Not setgid — the setuid is sufficient.
    };
  }
) cfg.wrappers);
```

This places the binary at `/run/wrappers/bin/${name}` with the correct
ownership and setuid bit. `/run/wrappers/bin` is already on `$PATH` in NixOS.

For wrappers **without** `runAs`, the current symlink model continues to work
unchanged.

#### Generate sidecar configs

The sidecar YAML files in `/etc/underwrap/wrappers/` stay the same. They are
read from the trusted fixed path, which is used when the binary detects
elevation (already implemented).

Each sidecar config needs one new optional field:

```yaml
# /etc/underwrap/wrappers/gws-castlabs.yaml
target: /nix/store/.../bin/gws
env_variable: GOOGLE_WORKSPACE_CLI_TOKEN
read_socket: /run/underwrap/read.sock    # or per-wrapper socket path
run_as_mode: true                        # signals to commit to euid, not drop to ruid
```

#### Generate per-wrapper tmpdir (recommended)

Tool users need somewhere to write temp files. A per-user tmpdir prevents
cross-tool temp file snooping:

```nix
systemd.tmpfiles.rules = lib.mkMerge (lib.mapAttrsToList (name: w:
  lib.optionals (w.runAs != null) [
    "d /run/underwrap/tmp/${name} 0700 ${w.runAs} ${w.runAs} -"
  ]
) cfg.wrappers);
```

The wrapper can set `TMPDIR=/run/underwrap/tmp/${name}` in the child
environment before exec.

#### Generate daemon policy

The daemon config needs to know which UIDs may read which secrets. The NixOS
module should resolve `runAs` user names to UIDs and include them in the
daemon's allowlist. Two approaches:

**Approach 1 (simple):** The daemon config's `read_allow_uids` includes all
tool user UIDs, and the per-wrapper secret access is enforced by the fact that
each tool user's UID is unique and the daemon checks `SO_PEERCRED` uid against
the wrapper definition's expected uid.

This requires a new field on `WrapperDef` in the daemon config:

```yaml
wrappers:
  gws-castlabs:
    target: /usr/bin/gws
    secret: google/castlabs
    expected_uid: 990     # UID of u-gws-castlabs
    injection:
      mode: env
      variable: GOOGLE_WORKSPACE_CLI_TOKEN
```

The daemon then enforces: the peer UID from `SO_PEERCRED` must match
`expected_uid` for this wrapper. This replaces the client-supplied wrapper
name as the identity proof.

**Approach 2 (per-wrapper socket, see below):** Each wrapper gets its own
socket, owned by its tool user, mode 0600. Only that UID can connect. The
daemon doesn't need per-wrapper UID config because the socket itself is the
access control.

### Implementation: underwrap-exec changes

**File:** `crates/underwrap-exec/src/main.rs`

#### New sidecar config field

Add `run_as_mode` to the `WrapperFile` struct:

```rust
#[derive(serde::Deserialize)]
struct WrapperFile {
    target: String,
    #[serde(default = "default_env")]
    env_variable: String,
    #[serde(default = "default_socket")]
    read_socket: String,
    /// If true, commit to the effective UID/GID instead of dropping to real UID/GID.
    /// Used when the wrapper binary is setuid to a dedicated tool user.
    #[serde(default)]
    run_as_mode: bool,
}
```

#### Modified drop_privilege function

The current `drop_privilege()` always drops to `ruid`/`rgid`. It needs to
support both modes:

```rust
/// Drop or commit privileges before exec.
///
/// In run_as_mode (setuid to tool user):
///   Commits to the effective UID/GID. The process becomes fully and
///   irreversibly the tool user. It cannot return to the calling user
///   or escalate to root.
///
/// In normal mode (setgid to underwrap group):
///   Drops back to the calling user's real UID/GID. Same as current behavior.
fn drop_privilege(run_as_mode: bool) -> Result<()> {
    if run_as_mode {
        let euid = unsafe { libc::geteuid() };
        let egid = unsafe { libc::getegid() };

        // Clear supplementary groups.
        let ret = unsafe { libc::setgroups(0, std::ptr::null()) };
        if ret != 0 {
            bail!("setgroups(0) failed: {}", std::io::Error::last_os_error());
        }

        // Commit GID: set real = effective.
        let ret = unsafe { libc::setgid(egid) };
        if ret != 0 {
            bail!("setgid({egid}) failed: {}", std::io::Error::last_os_error());
        }

        // Commit UID: set real = effective. This is irreversible.
        let ret = unsafe { libc::setuid(euid) };
        if ret != 0 {
            bail!("setuid({euid}) failed: {}", std::io::Error::last_os_error());
        }

        // Verify we are fully committed.
        assert_eq!(unsafe { libc::getuid() }, euid, "ruid != euid after setuid");
        assert_eq!(unsafe { libc::geteuid() }, euid, "euid changed unexpectedly");
        assert_eq!(unsafe { libc::getgid() }, egid, "rgid != egid after setgid");
        assert_eq!(unsafe { libc::getegid() }, egid, "egid changed unexpectedly");
    } else {
        // Current behavior: drop to real UID/GID.
        let ruid = unsafe { libc::getuid() };
        let rgid = unsafe { libc::getgid() };

        let ret = unsafe { libc::setgroups(0, std::ptr::null()) };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::EPERM) {
                bail!("setgroups(0) failed: {err}");
            }
        }

        if unsafe { libc::getegid() } != rgid {
            let ret = unsafe { libc::setgid(rgid) };
            if ret != 0 {
                bail!("setgid({rgid}) failed: {}", std::io::Error::last_os_error());
            }
        }

        if unsafe { libc::geteuid() } != ruid {
            let ret = unsafe { libc::setuid(ruid) };
            if ret != 0 {
                bail!("setuid({ruid}) failed: {}", std::io::Error::last_os_error());
            }
        }
    }

    Ok(())
}
```

#### Modified main()

```rust
fn main() -> Result<()> {
    let wrapper_name = detect_wrapper_name()?;
    let args: Vec<String> = std::env::args().skip(1).collect();

    let wrapper_config = load_wrapper_config(&wrapper_name)?;

    // Request secret from daemon (while still elevated).
    let material = request_secret(&wrapper_name, &wrapper_config.read_socket)?;

    // Drop or commit privileges BEFORE building the command.
    drop_privilege(wrapper_config.run_as_mode)?;

    // Build child command.
    let mut cmd = std::process::Command::new(&wrapper_config.target);
    cmd.args(&args);
    cmd.env(&wrapper_config.env_variable, &material);

    // If run_as_mode, set a clean TMPDIR for the tool user.
    if wrapper_config.run_as_mode {
        if let Some(ref tmpdir) = wrapper_config.tmpdir {
            cmd.env("TMPDIR", tmpdir);
        }
        // Set a minimal HOME to avoid tools trying to use robert's home.
        cmd.env("HOME", "/var/empty");
    }

    // pre_exec hook for PR_SET_DUMPABLE=0.
    unsafe {
        cmd.pre_exec(|| {
            let ret = libc::prctl(libc::PR_SET_DUMPABLE, 0);
            if ret != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let err = cmd.exec();
    bail!("exec failed: {err}");
}
```

#### Security: environment scrubbing in run_as_mode

When running as a tool user, the wrapper should scrub the inherited environment
to prevent the calling user from influencing the tool's behavior via env vars.
The wrapper should build a **minimal** environment for the child:

```rust
if wrapper_config.run_as_mode {
    cmd.env_clear();
    cmd.env(&wrapper_config.env_variable, &material);
    cmd.env("HOME", "/var/empty");
    cmd.env("PATH", "/usr/bin:/bin:/run/current-system/sw/bin");
    cmd.env("LANG", std::env::var("LANG").unwrap_or_else(|_| "C.UTF-8".into()));
    cmd.env("TERM", std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".into()));
    if let Some(ref tmpdir) = wrapper_config.tmpdir {
        cmd.env("TMPDIR", tmpdir);
    }
}
```

This prevents attacks where the caller sets `LD_PRELOAD`, `LD_LIBRARY_PATH`,
`http_proxy`, or other env vars to redirect the tool's behavior. (Note: the
kernel already strips `LD_PRELOAD` and `LD_LIBRARY_PATH` from setuid binaries,
but `http_proxy` and others are not stripped. Explicit `env_clear()` is the
safest approach.)

### Implementation: daemon policy changes

**File:** `crates/underwrap-protocol/src/config.rs`

Add an optional `expected_uid` field to `WrapperDef`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrapperDef {
    pub target: PathBuf,
    pub secret: String,
    #[serde(default)]
    pub injection: InjectionMode,
    #[serde(default)]
    pub derivation: DerivationMode,
    #[serde(default)]
    pub require_unlock: bool,
    /// If set, the daemon enforces that the peer's UID (from SO_PEERCRED)
    /// matches this value. This ties wrapper identity to a kernel-verified
    /// UID, preventing spoofing.
    #[serde(default)]
    pub expected_uid: Option<u32>,
}
```

**File:** `crates/underwrapd/src/server.rs`

In `process_read()`, after looking up the wrapper definition, add a UID check:

```rust
// Enforce expected_uid if configured.
if let Some(expected) = wrapper_def.expected_uid {
    match peer_uid {
        Some(uid) if uid == expected => { /* ok */ }
        Some(uid) => {
            let reason = format!(
                "peer uid {uid} does not match expected uid {expected} for wrapper '{}'",
                request.wrapper
            );
            audit::read_denied(&request.wrapper, &reason, peer_uid, request.scope.as_deref());
            return ReadResponse::Error {
                code: ErrorCode::Unauthorized,
                message: reason,
            };
        }
        None => {
            return ReadResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "cannot verify peer uid".to_string(),
            };
        }
    }
}
```

With this in place, the client-supplied `wrapper` name in the `ReadRequest` is
used only for looking up which `WrapperDef` to use — the actual authorization
is based on the kernel-provided UID. If an attacker connects and sends
`wrapper: "gws-castlabs"` but their UID doesn't match, the request is denied.

### Implementation: per-wrapper sockets

**Status: optional but recommended. Listed in TODO.md.**

For the strongest isolation, each wrapper gets its own dedicated read socket:

```
/run/underwrap/read-gws-castlabs.sock
  owner: u-gws-castlabs:u-gws-castlabs
  mode:  0600
```

Only `u-gws-castlabs` can connect to this socket (file permission check). The
daemon listens on all per-wrapper sockets. When a connection arrives on a
per-wrapper socket, the daemon knows which wrapper it is without any
client-supplied name — the socket path itself identifies the wrapper.

This provides **defense in depth**: file permissions AND `SO_PEERCRED` must
both pass.

#### NixOS module

Generate a socket per `runAs` wrapper:

```nix
systemd.tmpfiles.rules = lib.mkMerge (lib.mapAttrsToList (name: w:
  lib.optionals (w.runAs != null) [
    "d /run/underwrap 0755 underwrap underwrap -"
    # Per-wrapper socket directory entry not needed — the daemon creates the socket.
    # But we need tmpfiles for the tmp dir:
    "d /run/underwrap/tmp/${name} 0700 ${w.runAs} ${w.runAs} -"
  ]
) cfg.wrappers);
```

The daemon config includes per-wrapper socket paths:

```yaml
wrappers:
  gws-castlabs:
    target: /usr/bin/gws
    secret: google/castlabs
    expected_uid: 990
    read_socket: /run/underwrap/read-gws-castlabs.sock
    read_socket_mode: 0600
    read_socket_owner: 990   # UID of u-gws-castlabs
```

The daemon binds each per-wrapper socket, sets ownership to the tool user, and
sets mode 0600.

#### Wrapper sidecar config

The sidecar YAML points to the per-wrapper socket:

```yaml
# /etc/underwrap/wrappers/gws-castlabs.yaml
target: /nix/store/.../bin/gws
env_variable: GOOGLE_WORKSPACE_CLI_TOKEN
read_socket: /run/underwrap/read-gws-castlabs.sock
run_as_mode: true
tmpdir: /run/underwrap/tmp/gws-castlabs
```

### Implementation: protocol changes

**No protocol changes are needed.** The `ReadRequest` and `ReadResponse`
message types remain the same. The `wrapper` field in `ReadRequest` is still
sent (the daemon uses it to look up the `WrapperDef`), but authorization
shifts from trusting this field to verifying the peer UID.

In the future, if per-wrapper sockets are implemented, even the `wrapper` field
becomes redundant — the daemon knows which wrapper based on which socket
received the connection. The field can be kept for logging and backward
compatibility.

### Worked example

NixOS configuration:

```nix
services.underwrap = {
  enable = true;
  package = underwrap.packages.x86_64-linux.default;

  encryption = {
    enable = true;
    keyFile = "/var/lib/underwrap/master.key";
  };

  wrappers.gws-castlabs = {
    target = "${pkgs.gws}/bin/gws";
    secret = "google/castlabs";
    injection = { mode = "env"; variable = "GOOGLE_WORKSPACE_CLI_TOKEN"; };
    envVariable = "GOOGLE_WORKSPACE_CLI_TOKEN";
    derivation = "oauth_refresh_to_access_token";
    requireUnlock = true;
    runAs = "u-gws-castlabs";    # ← NEW: per-tool user
  };

  wrappers.gh-authorized = {
    target = "${pkgs.gh}/bin/gh";
    secret = "github/token";
    injection = { mode = "env"; variable = "GH_TOKEN"; };
    envVariable = "GH_TOKEN";
    # No runAs — gh needs to read git repos owned by the calling user.
  };

  secrets."google/castlabs" = { mutable = true; };
  secrets."github/token" = { mutable = false; };
};
```

What this produces:

| Item | gws-castlabs (with runAs) | gh-authorized (without runAs) |
|---|---|---|
| System user | `u-gws-castlabs` (nologin, home=/var/empty) | — |
| Binary | `/run/wrappers/bin/gws-castlabs` (copy, setuid u-gws-castlabs, mode 4755) | `/run/current-system/sw/bin/gh-authorized` (symlink, setgid underwrap) |
| Daemon auth | `SO_PEERCRED` uid = u-gws-castlabs, enforced by `expected_uid` | `SO_PEERCRED` gid = underwrap, wrapper name trusted |
| Target tool runs as | `u-gws-castlabs` | robert (calling user) |
| Access to `~robert/` | ❌ | ✅ |
| Access to other tools' secrets | ❌ | ⚠️ (can spoof wrapper name) |

### Limitations and edge cases

#### Tools that need file access

Some tools legitimately need to read the calling user's files. For example,
`gh pr create` reads the current git repository. These tools **cannot** use
`runAs` because the tool user has no access to the calling user's files.

`runAs` is opt-in per wrapper. Tools that need file access simply don't set it,
and they continue to work exactly as before.

If selective file access is needed in the future (e.g., read-only access to the
current directory but nothing else), that would require namespace/bind-mount
isolation (bubblewrap territory), which is out of scope for this improvement.

#### Interactive prompts

Tools running as a different user can still read from stdin and write to
stdout/stderr because those file descriptors are inherited across the uid
change. However, if a tool tries to access `/dev/tty` directly, it may fail
depending on tty permissions. This is unlikely for API client tools but should
be tested.

#### NixOS dependency

The per-tool user model is practical only with NixOS (or a similar declarative
system) because it requires creating system users, copying binaries with
specific ownership, and setting the setuid bit — all of which must happen at
system build/activation time. This is fine: the model is opt-in, NixOS-only,
and the rest of underwrap continues to work on generic Linux.

#### Wrapper binary audit surface

The wrapper binary runs setuid. It must be audited carefully:

- It reads from a **fixed, trusted config path** when elevated (already
  implemented — `UNDERWRAP_CONFIG_DIR` is ignored).
- It validates that the target path is absolute (already implemented).
- It connects to a specific socket path from the config (not user-controlled).
- It does not interpret user-supplied data in any dangerous way before exec.
- It should `env_clear()` in `run_as_mode` to prevent env-based attacks.

The binary is ~170 lines of Rust (plus the new mode). The attack surface is
small.

#### Interaction with PR_SET_DUMPABLE

When a setuid binary executes, the kernel automatically sets `dumpable = 0`
(based on `/proc/sys/fs/suid_dumpable`, default 0). After the wrapper calls
`setuid(euid)` (committing to the tool user), and then `exec()`s the target
(a normal non-setuid binary), the kernel may reset `dumpable` to 1.

The `pre_exec` hook that calls `prctl(PR_SET_DUMPABLE, 0)` handles this
correctly — it runs after fork but before exec, ensuring the child starts
non-dumpable. However, because `pre_exec` runs before exec and the kernel
may reset dumpable during exec of a non-suid binary, the implementation
should verify that the combination of credential changes (setuid in the
wrapper) plus exec of a normal binary preserves dumpable=0. If not, an
additional mechanism may be needed (e.g., a tiny shim that re-sets dumpable
after exec).

In practice: when `suid_dumpable=0` (the default), a process that underwent
credential changes will have `dumpable=0` after exec. This should be verified
in the VM integration tests.
