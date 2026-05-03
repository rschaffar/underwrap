use anyhow::{Result, anyhow};

use crate::config::AccessMode;
use crate::ipc::{WireDecode, WireEncode, WireReader};

use super::common::SecretBytes;
use super::errors::ErrorCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrapperRequest {
    /// Normal secret read; may trigger an `ApprovalRequired` response when the
    /// wrapper is in `PerCallApproval` mode.
    Read { scope: Option<String> },
    /// Follow-up to an `ApprovalRequired` response: the wrapper presents an
    /// SSH-agent signature over the daemon-issued nonce.
    ApprovalProof {
        challenge_id: String,
        signature: Vec<u8>,
        public_key_blob: Vec<u8>,
    },
}

impl WireEncode for WrapperRequest {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Read { scope } => {
                0u8.encode(out)?;
                scope.encode(out)
            }
            Self::ApprovalProof {
                challenge_id,
                signature,
                public_key_blob,
            } => {
                1u8.encode(out)?;
                challenge_id.encode(out)?;
                signature.encode(out)?;
                public_key_blob.encode(out)
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
            1 => Ok(Self::ApprovalProof {
                challenge_id: String::decode(reader)?,
                signature: Vec::<u8>::decode(reader)?,
                public_key_blob: Vec::<u8>::decode(reader)?,
            }),
            other => Err(anyhow!("invalid wrapper request discriminant: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadDirectoryBinding {
    pub resource_id: String,
    pub target_path: String,
    pub access: AccessMode,
}

impl WireEncode for ReadDirectoryBinding {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.resource_id.encode(out)?;
        self.target_path.encode(out)?;
        self.access.encode(out)
    }
}

impl WireDecode for ReadDirectoryBinding {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            resource_id: String::decode(reader)?,
            target_path: String::decode(reader)?,
            access: AccessMode::decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrapperResponse {
    ReadOk {
        env: std::collections::HashMap<String, SecretBytes>,
        directories: Vec<ReadDirectoryBinding>,
    },
    /// The wrapper is in per-call approval mode. The caller must sign `nonce`
    /// with the private half of `approval_key` (OpenSSH public key) and return
    /// the signature in a `WrapperRequest::ApprovalProof` on the same
    /// connection.
    ApprovalRequired {
        challenge_id: String,
        nonce: Vec<u8>,
        approval_key: String,
    },
    Error {
        code: ErrorCode,
        message: String,
    },
}

impl WireEncode for WrapperResponse {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::ReadOk { env, directories } => {
                0u8.encode(out)?;
                env.encode(out)?;
                directories.encode(out)
            }
            Self::ApprovalRequired {
                challenge_id,
                nonce,
                approval_key,
            } => {
                1u8.encode(out)?;
                challenge_id.encode(out)?;
                nonce.encode(out)?;
                approval_key.encode(out)
            }
            Self::Error { code, message } => {
                2u8.encode(out)?;
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
                directories: Vec::<ReadDirectoryBinding>::decode(reader)?,
            }),
            1 => Ok(Self::ApprovalRequired {
                challenge_id: String::decode(reader)?,
                nonce: Vec::<u8>::decode(reader)?,
                approval_key: String::decode(reader)?,
            }),
            2 => Ok(Self::Error {
                code: ErrorCode::decode(reader)?,
                message: String::decode(reader)?,
            }),
            other => Err(anyhow!("invalid wrapper response discriminant: {other}")),
        }
    }
}
