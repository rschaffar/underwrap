use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::{AccessMode, UnlockMode};
use crate::ipc::{WireDecode, WireEncode, WireReader};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnlockGrantStatus {
    pub scope: Option<String>,
    pub uid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_seconds: Option<u64>,
}

impl WireEncode for UnlockGrantStatus {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.scope.encode(out)?;
        self.uid.encode(out)?;
        self.remaining_seconds.encode(out)
    }
}

impl WireDecode for UnlockGrantStatus {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            scope: Option::<String>::decode(reader)?,
            uid: u32::decode(reader)?,
            remaining_seconds: Option::<u64>::decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnlockStatus {
    pub wrapper: String,
    pub unlocked: bool,
    pub grants: Vec<UnlockGrantStatus>,
}

impl WireEncode for UnlockStatus {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.wrapper.encode(out)?;
        self.unlocked.encode(out)?;
        self.grants.encode(out)
    }
}

impl WireDecode for UnlockStatus {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            wrapper: String::decode(reader)?,
            unlocked: bool::decode(reader)?,
            grants: Vec::<UnlockGrantStatus>::decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CredentialUsageStatus {
    pub credential: String,
    pub access: AccessMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stored: Option<bool>,
}

impl WireEncode for CredentialUsageStatus {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.credential.encode(out)?;
        self.access.encode(out)?;
        self.stored.encode(out)
    }
}

impl WireDecode for CredentialUsageStatus {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            credential: String::decode(reader)?,
            access: AccessMode::decode(reader)?,
            stored: Option::<bool>::decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WrapperUnlockStatus {
    pub mode: UnlockMode,
    #[serde(rename = "maxSeconds")]
    pub max_seconds: Option<u64>,
    pub status: UnlockStatus,
}

impl WireEncode for WrapperUnlockStatus {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.mode.encode(out)?;
        self.max_seconds.encode(out)?;
        self.status.encode(out)
    }
}

impl WireDecode for WrapperUnlockStatus {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            mode: UnlockMode::decode(reader)?,
            max_seconds: Option::<u64>::decode(reader)?,
            status: UnlockStatus::decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WrapperStatus {
    pub wrapper: String,
    pub target: String,
    pub usage: Vec<CredentialUsageStatus>,
    pub unlock: WrapperUnlockStatus,
}

impl WireEncode for WrapperStatus {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.wrapper.encode(out)?;
        self.target.encode(out)?;
        self.usage.encode(out)?;
        self.unlock.encode(out)
    }
}

impl WireDecode for WrapperStatus {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            wrapper: String::decode(reader)?,
            target: String::decode(reader)?,
            usage: Vec::<CredentialUsageStatus>::decode(reader)?,
            unlock: WrapperUnlockStatus::decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CredentialStatus {
    pub credential: String,
    pub stored: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_error: Option<String>,
}

impl WireEncode for CredentialStatus {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.credential.encode(out)?;
        self.stored.encode(out)?;
        self.storage_error.encode(out)
    }
}

impl WireDecode for CredentialStatus {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            credential: String::decode(reader)?,
            stored: bool::decode(reader)?,
            storage_error: Option::<String>::decode(reader)?,
        })
    }
}
