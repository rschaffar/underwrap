use anyhow::{Result, anyhow};
use std::path::PathBuf;

use crate::config::AccessMode;
use crate::ipc::{WireDecode, WireEncode, WireReader};
use crate::variant::WrapperVariant;

use super::common::SecretBytes;
use super::errors::ErrorCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrapperRequest {
    /// Normal secret read. The daemon authenticates the peer (instance
    /// group egid), validates the client-supplied `wrapper_name` against
    /// its config, asks the backend to authorise the invocation for the
    /// given `variant`, and answers with `ReadOk` (or `Error`).
    Read {
        /// Client-supplied wrapper name; validated against config. The
        /// kernel no longer binds identity to a per-wrapper group, so the
        /// daemon must check this against its known wrappers.
        wrapper_name: String,
        /// Real target or debug tool. The daemon, not the client, picks
        /// the binary based on the variant the backend authorised.
        variant: WrapperVariant,
        scope: Option<String>,
    },
}

impl WireEncode for WrapperRequest {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Read {
                wrapper_name,
                variant,
                scope,
            } => {
                0u8.encode(out)?;
                wrapper_name.encode(out)?;
                variant.encode(out)?;
                scope.encode(out)
            }
        }
    }
}

impl WireDecode for WrapperRequest {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::Read {
                wrapper_name: String::decode(reader)?,
                variant: WrapperVariant::decode(reader)?,
                scope: Option::<String>::decode(reader)?,
            }),
            other => Err(anyhow!("invalid wrapper request discriminant: {other}")),
        }
    }
}

// ReadOk carries the full exec plan and is the dominant variant; Error is
// rare and small. Boxing the hot path to equalise variant sizes would add an
// allocation on every successful read for no real benefit.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrapperResponse {
    ReadOk {
        env: std::collections::HashMap<String, SecretBytes>,
        /// The single resource the wrapper projects. The daemon sends
        /// exactly one wrapper-resource channel fd via SCM_RIGHTS
        /// after the `ReadOk` response itself, regardless of which
        /// slots (`env`, `files`, `libsecret`) the resource opts into.
        resource_id: String,
        /// Operator-facing resource name (e.g. `github/token`).
        /// Carried alongside the id so the wrapper can produce
        /// human- and journal-readable diagnostics that name the
        /// resource without an extra backend round-trip.
        resource_name: String,
        access: AccessMode,
        /// Whether secretsd should be launched for this resource.
        libsecret: bool,
        /// Target-home-relative paths at which the files staging dir
        /// for this resource should be bind-mounted in the namespace.
        /// Empty when the wrapper does not project a `files` slot.
        files_target_paths: Vec<String>,
        /// Daemon-chosen binary to exec: the configured `target` for
        /// `variant = Real`, or the instance's `debug_tool_target` for
        /// `variant = Debug`. The wrapper never chooses this — it is the
        /// trust-boundary control that stops a client from running the
        /// debug tool under a `Real` authorisation.
        target: PathBuf,
        /// User env vars the target may inherit (from the wrapper config).
        env_passthrough: Vec<String>,
        /// Static env vars injected into the target (from the config).
        env_set: std::collections::HashMap<String, String>,
        /// Base dir from which ns derives `XDG_RUNTIME_DIR`.
        xdg_runtime_dir_base: String,
        /// Caller-owned directories to idmap (caller<->dedicated uid) into the
        /// target's mount namespace (`~`/`~/rel`/absolute).
        access_paths: Vec<String>,
        /// Idmap the caller's runtime dir (`<xdg_runtime_dir_base>/<caller-uid>`)
        /// into the target namespace; ns resolves the caller uid at invocation.
        access_runtime_dir: bool,
    },
    Error {
        code: ErrorCode,
        message: String,
    },
}

impl WireEncode for WrapperResponse {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::ReadOk {
                env,
                resource_id,
                resource_name,
                access,
                libsecret,
                files_target_paths,
                target,
                env_passthrough,
                env_set,
                xdg_runtime_dir_base,
                access_paths,
                access_runtime_dir,
            } => {
                0u8.encode(out)?;
                env.encode(out)?;
                resource_id.encode(out)?;
                resource_name.encode(out)?;
                access.encode(out)?;
                libsecret.encode(out)?;
                files_target_paths.encode(out)?;
                target.encode(out)?;
                env_passthrough.encode(out)?;
                env_set.encode(out)?;
                xdg_runtime_dir_base.encode(out)?;
                access_paths.encode(out)?;
                access_runtime_dir.encode(out)
            }
            Self::Error { code, message } => {
                1u8.encode(out)?;
                code.encode(out)?;
                message.encode(out)
            }
        }
    }
}

impl WireDecode for WrapperResponse {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::ReadOk {
                env: std::collections::HashMap::<String, SecretBytes>::decode(reader)?,
                resource_id: String::decode(reader)?,
                resource_name: String::decode(reader)?,
                access: AccessMode::decode(reader)?,
                libsecret: bool::decode(reader)?,
                files_target_paths: Vec::<String>::decode(reader)?,
                target: PathBuf::decode(reader)?,
                env_passthrough: Vec::<String>::decode(reader)?,
                env_set: std::collections::HashMap::<String, String>::decode(reader)?,
                xdg_runtime_dir_base: String::decode(reader)?,
                access_paths: Vec::<String>::decode(reader)?,
                access_runtime_dir: bool::decode(reader)?,
            }),
            1 => Ok(Self::Error {
                code: ErrorCode::decode(reader)?,
                message: String::decode(reader)?,
            }),
            other => Err(anyhow!("invalid wrapper response discriminant: {other}")),
        }
    }
}
