use anyhow::{Result, anyhow};

use crate::ipc::{WireDecode, WireEncode, WireReader};

use super::common::SecretBytes;
use super::errors::BackendErrorCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendRequest {
    Read {
        secret_id: String,
    },
    Write {
        secret_id: String,
        material: SecretBytes,
    },
    List {
        prefix: String,
    },
    Delete {
        secret_id: String,
    },
    Status,
}

impl WireEncode for BackendRequest {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Read { secret_id } => {
                0u8.encode(out)?;
                secret_id.encode(out)
            }
            Self::Write {
                secret_id,
                material,
            } => {
                1u8.encode(out)?;
                secret_id.encode(out)?;
                material.encode(out)
            }
            Self::List { prefix } => {
                2u8.encode(out)?;
                prefix.encode(out)
            }
            Self::Delete { secret_id } => {
                3u8.encode(out)?;
                secret_id.encode(out)
            }
            Self::Status => 4u8.encode(out),
        }
    }
}

impl WireDecode for BackendRequest {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::Read {
                secret_id: String::decode(reader)?,
            }),
            1 => Ok(Self::Write {
                secret_id: String::decode(reader)?,
                material: SecretBytes::decode(reader)?,
            }),
            2 => Ok(Self::List {
                prefix: String::decode(reader)?,
            }),
            3 => Ok(Self::Delete {
                secret_id: String::decode(reader)?,
            }),
            4 => Ok(Self::Status),
            other => Err(anyhow!("invalid backend request discriminant: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendResponse {
    ReadOk {
        material: Option<SecretBytes>,
    },
    WriteOk,
    ListOk {
        ids: Vec<String>,
    },
    DeleteOk {
        existed: bool,
    },
    StatusOk,
    Error {
        code: BackendErrorCode,
        message: String,
    },
}

impl WireEncode for BackendResponse {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::ReadOk { material } => {
                0u8.encode(out)?;
                material.encode(out)
            }
            Self::WriteOk => 1u8.encode(out),
            Self::ListOk { ids } => {
                2u8.encode(out)?;
                ids.encode(out)
            }
            Self::DeleteOk { existed } => {
                3u8.encode(out)?;
                existed.encode(out)
            }
            Self::StatusOk => 4u8.encode(out),
            Self::Error { code, message } => {
                5u8.encode(out)?;
                code.encode(out)?;
                message.encode(out)
            }
        }
    }
}

impl WireDecode for BackendResponse {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::ReadOk {
                material: Option::<SecretBytes>::decode(reader)?,
            }),
            1 => Ok(Self::WriteOk),
            2 => Ok(Self::ListOk {
                ids: Vec::<String>::decode(reader)?,
            }),
            3 => Ok(Self::DeleteOk {
                existed: bool::decode(reader)?,
            }),
            4 => Ok(Self::StatusOk),
            5 => Ok(Self::Error {
                code: BackendErrorCode::decode(reader)?,
                message: String::decode(reader)?,
            }),
            other => Err(anyhow!("invalid backend response discriminant: {other}")),
        }
    }
}
