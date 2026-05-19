use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::ipc::{WireDecode, WireEncode, WireReader};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorCode {
    Unauthorized,
    NotFound,
    Locked,
    Internal,
}

impl WireEncode for ErrorCode {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Unauthorized => 0u8.encode(out),
            Self::NotFound => 1u8.encode(out),
            Self::Locked => 2u8.encode(out),
            Self::Internal => 3u8.encode(out),
        }
    }
}

impl WireDecode for ErrorCode {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::Unauthorized),
            1 => Ok(Self::NotFound),
            2 => Ok(Self::Locked),
            3 => Ok(Self::Internal),
            other => Err(anyhow!("invalid error code discriminant: {other}")),
        }
    }
}
