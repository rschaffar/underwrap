use crate::config::AccessMode;
use crate::ipc::{WireDecode, WireEncode, WireReader};
use crate::message::SecretBytes;
use anyhow::Result;
use std::collections::HashMap;

/// The wrapper-resource binding flowing through the exec → ns handoff.
/// `channel_fd` is the ns-side fd number that backs the authenticated
/// socketpair to the daemon for this resource; ns drives `Checkout` /
/// `Checkin` / `UsageSignal` over that fd. Each wrapper run has
/// exactly one resource, so the bundle carries exactly one binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecResourceBinding {
    pub resource_id: String,
    /// Operator-facing resource name carried alongside the id so ns
    /// can name the resource in journal and stderr diagnostics.
    pub resource_name: String,
    pub access: AccessMode,
    pub channel_fd: u32,
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
        self.channel_fd.encode(out)?;
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
            channel_fd: u32::decode(reader)?,
            libsecret: bool::decode(reader)?,
            files_target_paths: Vec::<String>::decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecBundle {
    pub env: HashMap<String, SecretBytes>,
    pub env_passthrough: Vec<String>,
    pub env_set: HashMap<String, String>,
    /// Base directory from which ns derives the final target
    /// `XDG_RUNTIME_DIR` as `<xdg_runtime_dir_base>/<uid>`.
    pub xdg_runtime_dir_base: String,
    /// The wrapper-resource binding for this run's single resource,
    /// carrying the fd for the bulk-blob IPC channel
    /// (`Checkout` / `Checkin` / `UsageSignal`).
    pub resource: ExecResourceBinding,
}

impl WireEncode for ExecBundle {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.env.encode(out)?;
        self.env_passthrough.encode(out)?;
        self.env_set.encode(out)?;
        self.xdg_runtime_dir_base.encode(out)?;
        self.resource.encode(out)
    }
}

impl WireDecode for ExecBundle {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            env: HashMap::<String, SecretBytes>::decode(reader)?,
            env_passthrough: Vec::<String>::decode(reader)?,
            env_set: HashMap::<String, String>::decode(reader)?,
            xdg_runtime_dir_base: String::decode(reader)?,
            resource: ExecResourceBinding::decode(reader)?,
        })
    }
}

#[cfg(test)]
mod tests;
