use anyhow::{Result, anyhow};

use crate::ipc::{WireDecode, WireEncode, WireReader};

use super::common::SecretBytes;
use super::errors::DirectoryErrorCode;

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
