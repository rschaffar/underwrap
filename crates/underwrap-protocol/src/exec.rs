use crate::config::AccessMode;
use crate::ipc::{WireDecode, WireEncode, WireReader};
use crate::message::SecretBytes;
use anyhow::Result;
use std::collections::HashMap;

/// Fixed descriptor on which `underwrap-ns` receives the execution bundle.
pub const EXEC_BUNDLE_FD: i32 = 3;

/// Fixed descriptor on which `underwrap-ns` receives the daemon-created
/// wrapper-resource socket. The descriptor number is part of the trusted local
/// ABI, not data carried in the untrusted exec bundle.
pub const WRAPPER_RESOURCE_CHANNEL_FD: i32 = 20;

/// Trusted exec-stage hint that tells `underwrap-ns` to create its idmap helper
/// before reading the secret-bearing fd-3 bundle. `underwrap-exec` always
/// removes caller input under this name and sets exactly `1` when access paths
/// are configured; `underwrap-ns` verifies the hint against the decoded bundle.
pub const IDMAP_REQUIRED_ENV: &str = "UNDERWRAP_IDMAP_REQUIRED";

/// The wrapper-resource binding flowing through the exec → ns handoff.
/// `underwrap-ns` drives `Checkout` / `Checkin` / `UsageSignal` over the fixed,
/// separately authenticated [`WRAPPER_RESOURCE_CHANNEL_FD`]. Each wrapper run
/// has exactly one resource, so the bundle carries exactly one binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecResourceBinding {
    pub resource_id: String,
    /// Operator-facing resource name carried alongside the id so ns
    /// can name the resource in journal and stderr diagnostics.
    pub resource_name: String,
    pub access: AccessMode,
    /// Whether secretsd should be launched for this resource.
    pub libsecret: bool,
    /// Target-home-relative paths at which the files staging dir for
    /// this resource should be bind-mounted (always read-write at the
    /// mount layer; the backend gates persistence).
    pub files_target_paths: Vec<String>,
}

impl WireEncode for ExecResourceBinding {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.resource_id.encode(out)?;
        self.resource_name.encode(out)?;
        self.access.encode(out)?;
        self.libsecret.encode(out)?;
        self.files_target_paths.encode(out)
    }
}

impl WireDecode for ExecResourceBinding {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            resource_id: String::decode(reader)?,
            resource_name: String::decode(reader)?,
            access: AccessMode::decode(reader)?,
            libsecret: bool::decode(reader)?,
            files_target_paths: Vec::<String>::decode(reader)?,
        })
    }
}

/// The full exec → ns handoff payload.
///
/// **Authority invariant:** this bundle carries no target UID/GID.
/// `underwrap-ns` resolves the dedicated identity from the root-owned
/// `/etc/underwrap/target-uids` table using the `(instance, wrapper)` selector
/// supplied separately by `underwrap-exec`. Adding an identity field here would
/// let a compromised stage redirect the capability-backed UID switch, so its
/// absence is load-bearing. See `exec_bundle_carries_no_target_identity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecBundle {
    pub env: HashMap<String, SecretBytes>,
    pub env_passthrough: Vec<String>,
    pub env_set: HashMap<String, String>,
    /// Base directory under which each isolated target's own runtime
    /// directory is provisioned by the system. ns derives the default
    /// `XDG_RUNTIME_DIR` as `<target_runtime_dir_base>/<target-user-name>`
    /// — the executing dedicated uid's own dir, not the caller's.
    pub target_runtime_dir_base: String,
    /// Metadata for this run's single resource. The bulk-blob IPC descriptor
    /// is fixed and authenticated out of band; it is deliberately absent from
    /// the bundle.
    pub resource: ExecResourceBinding,
    /// Caller-owned directories to idmap (caller<->dedicated uid) into the
    /// target's mount namespace (`~`/`~/rel`/absolute).
    pub access_paths: Vec<String>,
}

impl WireEncode for ExecBundle {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.env.encode(out)?;
        self.env_passthrough.encode(out)?;
        self.env_set.encode(out)?;
        self.target_runtime_dir_base.encode(out)?;
        self.resource.encode(out)?;
        self.access_paths.encode(out)
    }
}

impl WireDecode for ExecBundle {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            env: HashMap::<String, SecretBytes>::decode(reader)?,
            env_passthrough: Vec::<String>::decode(reader)?,
            env_set: HashMap::<String, String>::decode(reader)?,
            target_runtime_dir_base: String::decode(reader)?,
            resource: ExecResourceBinding::decode(reader)?,
            access_paths: Vec::<String>::decode(reader)?,
        })
    }
}

#[cfg(test)]
mod tests;
