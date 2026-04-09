use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::ipc::{WireDecode, WireEncode, WireReader};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecretsdErrorCode {
    InvalidRequest,
    Unauthorized,
    Internal,
}

impl WireEncode for SecretsdErrorCode {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::InvalidRequest => 0u8.encode(out),
            Self::Unauthorized => 1u8.encode(out),
            Self::Internal => 2u8.encode(out),
        }
    }
}

impl WireDecode for SecretsdErrorCode {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::InvalidRequest),
            1 => Ok(Self::Unauthorized),
            2 => Ok(Self::Internal),
            other => Err(anyhow!("invalid secretsd error code discriminant: {other}")),
        }
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DirectoryErrorCode {
    InvalidRequest,
    Unauthorized,
    Internal,
}

impl WireEncode for DirectoryErrorCode {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::InvalidRequest => 0u8.encode(out),
            Self::Unauthorized => 1u8.encode(out),
            Self::Internal => 2u8.encode(out),
        }
    }
}

impl WireDecode for DirectoryErrorCode {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::InvalidRequest),
            1 => Ok(Self::Unauthorized),
            2 => Ok(Self::Internal),
            other => Err(anyhow!(
                "invalid directory error code discriminant: {other}"
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackendErrorCode {
    Internal,
}

impl WireEncode for BackendErrorCode {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Internal => 0u8.encode(out),
        }
    }
}

impl WireDecode for BackendErrorCode {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::Internal),
            other => Err(anyhow!("invalid backend error code discriminant: {other}")),
        }
    }
}
