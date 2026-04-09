use anyhow::{Result, anyhow};

use crate::config::AccessMode;
use crate::ipc::{WireDecode, WireEncode, WireReader};

use super::common::SecretBytes;
use super::errors::ErrorCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrapperRequest {
    Read {
        scope: Option<String>,
    },
    Unlock {
        scope: Option<String>,
        seconds: Option<u64>,
    },
    UnlockResponse {
        scope: Option<String>,
        seconds: Option<u64>,
        challenge: Vec<u8>,
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
            Self::Unlock { scope, seconds } => {
                1u8.encode(out)?;
                scope.encode(out)?;
                seconds.encode(out)
            }
            Self::UnlockResponse {
                scope,
                seconds,
                challenge,
                signature,
                public_key_blob,
            } => {
                2u8.encode(out)?;
                scope.encode(out)?;
                seconds.encode(out)?;
                challenge.encode(out)?;
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
            1 => Ok(Self::Unlock {
                scope: Option::<String>::decode(reader)?,
                seconds: Option::<u64>::decode(reader)?,
            }),
            2 => Ok(Self::UnlockResponse {
                scope: Option::<String>::decode(reader)?,
                seconds: Option::<u64>::decode(reader)?,
                challenge: Vec::<u8>::decode(reader)?,
                signature: Vec::<u8>::decode(reader)?,
                public_key_blob: Vec::<u8>::decode(reader)?,
            }),
            other => Err(anyhow!("invalid wrapper request discriminant: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadDirectoryBinding {
    pub credential_ref: String,
    pub target_path: String,
    pub access: AccessMode,
}

impl WireEncode for ReadDirectoryBinding {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.credential_ref.encode(out)?;
        self.target_path.encode(out)?;
        self.access.encode(out)
    }
}

impl WireDecode for ReadDirectoryBinding {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            credential_ref: String::decode(reader)?,
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
    Challenge {
        nonce: Vec<u8>,
    },
    UnlockOk,
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
            Self::Challenge { nonce } => {
                1u8.encode(out)?;
                nonce.encode(out)
            }
            Self::UnlockOk => 2u8.encode(out),
            Self::Error { code, message } => {
                3u8.encode(out)?;
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
            1 => Ok(Self::Challenge {
                nonce: Vec::<u8>::decode(reader)?,
            }),
            2 => Ok(Self::UnlockOk),
            3 => Ok(Self::Error {
                code: ErrorCode::decode(reader)?,
                message: String::decode(reader)?,
            }),
            other => Err(anyhow!("invalid wrapper response discriminant: {other}")),
        }
    }
}
