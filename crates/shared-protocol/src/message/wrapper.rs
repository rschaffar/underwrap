use anyhow::{Result, anyhow};

use crate::config::AccessMode;
use crate::ipc::{WireDecode, WireEncode, WireReader};

use super::common::SecretBytes;
use super::errors::ErrorCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrapperRequest {
    /// Normal secret read. The daemon authenticates the peer, asks the
    /// backend to authorise the invocation, and answers with `ReadOk`
    /// (or `Error`).
    Read { scope: Option<String> },
}

impl WireEncode for WrapperRequest {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Read { scope } => {
                0u8.encode(out)?;
                scope.encode(out)
            }
        }
    }
}

impl WireDecode for WrapperRequest {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::Read {
                scope: Option::<String>::decode(reader)?,
            }),
            other => Err(anyhow!("invalid wrapper request discriminant: {other}")),
        }
    }
}

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
            } => {
                0u8.encode(out)?;
                env.encode(out)?;
                resource_id.encode(out)?;
                resource_name.encode(out)?;
                access.encode(out)?;
                libsecret.encode(out)?;
                files_target_paths.encode(out)
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
            }),
            1 => Ok(Self::Error {
                code: ErrorCode::decode(reader)?,
                message: String::decode(reader)?,
            }),
            other => Err(anyhow!("invalid wrapper response discriminant: {other}")),
        }
    }
}
