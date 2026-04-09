use anyhow::{Result, anyhow};

use crate::ipc::{WireDecode, WireEncode, WireReader};

use super::common::SecretBytes;
use super::errors::ErrorCode;
use super::status::{CredentialStatus, WrapperStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlRequest {
    StatusAll,
    StatusTarget {
        target: String,
    },
    Store {
        secret_id: String,
        material: SecretBytes,
    },
    Unlock {
        target: String,
        scope: Option<String>,
        seconds: Option<u64>,
        grant_user: Option<String>,
        grant_all: bool,
    },
    Lock {
        target: Option<String>,
        scope: Option<String>,
    },
}

impl WireEncode for ControlRequest {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::StatusAll => 0u8.encode(out),
            Self::StatusTarget { target } => {
                1u8.encode(out)?;
                target.encode(out)
            }
            Self::Store {
                secret_id,
                material,
            } => {
                2u8.encode(out)?;
                secret_id.encode(out)?;
                material.encode(out)
            }
            Self::Unlock {
                target,
                scope,
                seconds,
                grant_user,
                grant_all,
            } => {
                3u8.encode(out)?;
                target.encode(out)?;
                scope.encode(out)?;
                seconds.encode(out)?;
                grant_user.encode(out)?;
                grant_all.encode(out)
            }
            Self::Lock { target, scope } => {
                4u8.encode(out)?;
                target.encode(out)?;
                scope.encode(out)
            }
        }
    }
}

impl WireDecode for ControlRequest {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::StatusAll),
            1 => Ok(Self::StatusTarget {
                target: String::decode(reader)?,
            }),
            2 => Ok(Self::Store {
                secret_id: String::decode(reader)?,
                material: SecretBytes::decode(reader)?,
            }),
            3 => Ok(Self::Unlock {
                target: String::decode(reader)?,
                scope: Option::<String>::decode(reader)?,
                seconds: Option::<u64>::decode(reader)?,
                grant_user: Option::<String>::decode(reader)?,
                grant_all: bool::decode(reader)?,
            }),
            4 => Ok(Self::Lock {
                target: Option::<String>::decode(reader)?,
                scope: Option::<String>::decode(reader)?,
            }),
            other => Err(anyhow!("invalid control request discriminant: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlResponse {
    StatusAllOk {
        wrappers: Vec<String>,
        credentials: Vec<String>,
    },
    StatusWrapperOk {
        status: WrapperStatus,
    },
    StatusCredentialOk {
        status: CredentialStatus,
    },
    StoreOk,
    UnlockOk,
    LockOk,
    Error {
        code: ErrorCode,
        message: String,
    },
}

impl WireEncode for ControlResponse {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::StatusAllOk {
                wrappers,
                credentials,
            } => {
                0u8.encode(out)?;
                wrappers.encode(out)?;
                credentials.encode(out)
            }
            Self::StatusWrapperOk { status } => {
                1u8.encode(out)?;
                status.encode(out)
            }
            Self::StatusCredentialOk { status } => {
                2u8.encode(out)?;
                status.encode(out)
            }
            Self::StoreOk => 3u8.encode(out),
            Self::UnlockOk => 4u8.encode(out),
            Self::LockOk => 5u8.encode(out),
            Self::Error { code, message } => {
                6u8.encode(out)?;
                code.encode(out)?;
                message.encode(out)
            }
        }
    }
}

impl WireDecode for ControlResponse {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::StatusAllOk {
                wrappers: Vec::<String>::decode(reader)?,
                credentials: Vec::<String>::decode(reader)?,
            }),
            1 => Ok(Self::StatusWrapperOk {
                status: WrapperStatus::decode(reader)?,
            }),
            2 => Ok(Self::StatusCredentialOk {
                status: CredentialStatus::decode(reader)?,
            }),
            3 => Ok(Self::StoreOk),
            4 => Ok(Self::UnlockOk),
            5 => Ok(Self::LockOk),
            6 => Ok(Self::Error {
                code: ErrorCode::decode(reader)?,
                message: String::decode(reader)?,
            }),
            other => Err(anyhow!("invalid control response discriminant: {other}")),
        }
    }
}
