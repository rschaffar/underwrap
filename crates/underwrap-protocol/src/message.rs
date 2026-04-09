//! IPC message types exchanged over Unix domain sockets.
//!
//! All messages use the binary framing/codec defined in [`crate::ipc`].

use crate::config::{AccessMode, UnlockMode};
use crate::ipc::{WireDecode, WireEncode, WireReader};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Debug, Clone, Zeroize, ZeroizeOnDrop, PartialEq, Eq)]
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn as_utf8(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.0)
    }

    pub fn into_utf8_string(mut self) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(std::mem::take(&mut self.0))
    }
}

impl std::ops::Deref for SecretBytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<[u8]> for SecretBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for SecretBytes {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl From<String> for SecretBytes {
    fn from(value: String) -> Self {
        Self(value.into_bytes())
    }
}

impl From<&str> for SecretBytes {
    fn from(value: &str) -> Self {
        Self(value.as_bytes().to_vec())
    }
}

impl From<&[u8]> for SecretBytes {
    fn from(value: &[u8]) -> Self {
        Self(value.to_vec())
    }
}

impl From<SecretBytes> for Vec<u8> {
    fn from(mut value: SecretBytes) -> Self {
        std::mem::take(&mut value.0)
    }
}

impl PartialEq<&str> for SecretBytes {
    fn eq(&self, other: &&str) -> bool {
        self.0 == other.as_bytes()
    }
}

impl PartialEq<SecretBytes> for &str {
    fn eq(&self, other: &SecretBytes) -> bool {
        self.as_bytes() == other.0.as_slice()
    }
}

impl PartialEq<String> for SecretBytes {
    fn eq(&self, other: &String) -> bool {
        self.0 == other.as_bytes()
    }
}

impl PartialEq<SecretBytes> for String {
    fn eq(&self, other: &SecretBytes) -> bool {
        self.as_bytes() == other.0.as_slice()
    }
}

impl<const N: usize> PartialEq<&[u8; N]> for SecretBytes {
    fn eq(&self, other: &&[u8; N]) -> bool {
        self.0 == other.as_slice()
    }
}

impl<const N: usize> PartialEq<SecretBytes> for &[u8; N] {
    fn eq(&self, other: &SecretBytes) -> bool {
        self.as_slice() == other.0.as_slice()
    }
}

impl WireEncode for SecretBytes {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.0.encode(out)
    }
}

impl WireDecode for SecretBytes {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self(Vec::<u8>::decode(reader)?))
    }
}

impl WireEncode for AccessMode {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::ReadOnly => 0u8.encode(out),
            Self::ReadWrite => 1u8.encode(out),
        }
    }
}

impl WireDecode for AccessMode {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::ReadOnly),
            1 => Ok(Self::ReadWrite),
            other => Err(anyhow!("invalid access mode discriminant: {other}")),
        }
    }
}

impl WireEncode for UnlockMode {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::AlwaysUnlocked => 0u8.encode(out),
            Self::Always => 1u8.encode(out),
            Self::Optional => 2u8.encode(out),
        }
    }
}

impl WireDecode for UnlockMode {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::AlwaysUnlocked),
            1 => Ok(Self::Always),
            2 => Ok(Self::Optional),
            other => Err(anyhow!("invalid unlock mode discriminant: {other}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Typed status payloads
// ---------------------------------------------------------------------------

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
    #[serde(rename = "use")]
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

// ---------------------------------------------------------------------------
// Wrapper-socket messages
// ---------------------------------------------------------------------------

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
        env: HashMap<String, SecretBytes>,
        directories: Vec<ReadDirectoryBinding>,
    },
    UnlockOk,
    Challenge {
        nonce: Vec<u8>,
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
            Self::UnlockOk => 1u8.encode(out),
            Self::Challenge { nonce } => {
                2u8.encode(out)?;
                nonce.encode(out)
            }
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
                env: HashMap::<String, SecretBytes>::decode(reader)?,
                directories: Vec::<ReadDirectoryBinding>::decode(reader)?,
            }),
            1 => Ok(Self::UnlockOk),
            2 => Ok(Self::Challenge {
                nonce: Vec::<u8>::decode(reader)?,
            }),
            3 => Ok(Self::Error {
                code: ErrorCode::decode(reader)?,
                message: String::decode(reader)?,
            }),
            other => Err(anyhow!("invalid wrapper response discriminant: {other}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Control-path messages
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Secretsd channel messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretsdRequest {
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
}

impl WireEncode for SecretsdRequest {
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
        }
    }
}

impl WireDecode for SecretsdRequest {
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
            other => Err(anyhow!("invalid secretsd request discriminant: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretsdResponse {
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
    Error {
        code: SecretsdErrorCode,
        message: String,
    },
}

impl WireEncode for SecretsdResponse {
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
            Self::Error { code, message } => {
                4u8.encode(out)?;
                code.encode(out)?;
                message.encode(out)
            }
        }
    }
}

impl WireDecode for SecretsdResponse {
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
            4 => Ok(Self::Error {
                code: SecretsdErrorCode::decode(reader)?,
                message: String::decode(reader)?,
            }),
            other => Err(anyhow!("invalid secretsd response discriminant: {other}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Directory channel protocol (daemon <-> underwrap-ns over scoped socketpair)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryRequest {
    ListFiles,
    ReadFile { path: String },
    WriteFile { path: String, content: SecretBytes },
    DeleteFile { path: String },
}

impl WireEncode for DirectoryRequest {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::ListFiles => 0u8.encode(out),
            Self::ReadFile { path } => {
                1u8.encode(out)?;
                path.encode(out)
            }
            Self::WriteFile { path, content } => {
                2u8.encode(out)?;
                path.encode(out)?;
                content.encode(out)
            }
            Self::DeleteFile { path } => {
                3u8.encode(out)?;
                path.encode(out)
            }
        }
    }
}

impl WireDecode for DirectoryRequest {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::ListFiles),
            1 => Ok(Self::ReadFile {
                path: String::decode(reader)?,
            }),
            2 => Ok(Self::WriteFile {
                path: String::decode(reader)?,
                content: SecretBytes::decode(reader)?,
            }),
            3 => Ok(Self::DeleteFile {
                path: String::decode(reader)?,
            }),
            other => Err(anyhow!("invalid directory request discriminant: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryResponse {
    ListFilesOk {
        paths: Vec<String>,
    },
    ReadFileOk {
        content: Option<SecretBytes>,
    },
    WriteFileOk,
    DeleteFileOk {
        existed: bool,
    },
    Error {
        code: DirectoryErrorCode,
        message: String,
    },
}

impl WireEncode for DirectoryResponse {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::ListFilesOk { paths } => {
                0u8.encode(out)?;
                paths.encode(out)
            }
            Self::ReadFileOk { content } => {
                1u8.encode(out)?;
                content.encode(out)
            }
            Self::WriteFileOk => 2u8.encode(out),
            Self::DeleteFileOk { existed } => {
                3u8.encode(out)?;
                existed.encode(out)
            }
            Self::Error { code, message } => {
                4u8.encode(out)?;
                code.encode(out)?;
                message.encode(out)
            }
        }
    }
}

impl WireDecode for DirectoryResponse {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::ListFilesOk {
                paths: Vec::<String>::decode(reader)?,
            }),
            1 => Ok(Self::ReadFileOk {
                content: Option::<SecretBytes>::decode(reader)?,
            }),
            2 => Ok(Self::WriteFileOk),
            3 => Ok(Self::DeleteFileOk {
                existed: bool::decode(reader)?,
            }),
            4 => Ok(Self::Error {
                code: DirectoryErrorCode::decode(reader)?,
                message: String::decode(reader)?,
            }),
            other => Err(anyhow!("invalid directory response discriminant: {other}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Backend protocol
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Shared error codes
// ---------------------------------------------------------------------------

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{decode_from_slice, encode_to_vec};

    #[test]
    fn wrapper_request_roundtrip() {
        let req = WrapperRequest::Read {
            scope: Some("session-42".to_string()),
        };
        let encoded = encode_to_vec(&req).unwrap();
        let parsed: WrapperRequest = decode_from_slice(&encoded).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn wrapper_response_roundtrip() {
        let resp = WrapperResponse::ReadOk {
            env: HashMap::from([("TOKEN".to_string(), SecretBytes::from("secret123"))]),
            directories: vec![ReadDirectoryBinding {
                credential_ref: "gws/state".to_string(),
                target_path: ".config/gws".to_string(),
                access: AccessMode::ReadOnly,
            }],
        };
        let encoded = encode_to_vec(&resp).unwrap();
        let parsed: WrapperResponse = decode_from_slice(&encoded).unwrap();
        assert_eq!(parsed, resp);
    }

    #[test]
    fn control_status_roundtrip() {
        let resp = ControlResponse::StatusCredentialOk {
            status: CredentialStatus {
                credential: "a/b".to_string(),
                stored: true,
                storage_error: None,
            },
        };
        let encoded = encode_to_vec(&resp).unwrap();
        let parsed: ControlResponse = decode_from_slice(&encoded).unwrap();
        assert_eq!(parsed, resp);
    }

    #[test]
    fn backend_write_roundtrip() {
        let req = BackendRequest::Write {
            secret_id: "tok".to_string(),
            material: vec![0, 1, 2, 255].into(),
        };
        let encoded = encode_to_vec(&req).unwrap();
        let parsed: BackendRequest = decode_from_slice(&encoded).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn secretsd_response_roundtrip() {
        let resp = SecretsdResponse::ReadOk {
            material: Some(vec![7, 8, 9].into()),
        };
        let encoded = encode_to_vec(&resp).unwrap();
        let parsed: SecretsdResponse = decode_from_slice(&encoded).unwrap();
        assert_eq!(parsed, resp);
    }
}
