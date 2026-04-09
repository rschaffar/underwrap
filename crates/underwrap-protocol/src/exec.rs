use crate::config::AccessMode;
use crate::ipc::{WireDecode, WireEncode, WireReader};
use crate::message::SecretBytes;
use anyhow::Result;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecDirectoryBinding {
    pub credential_ref: String,
    pub target_path: String,
    pub access: AccessMode,
    pub channel_fd: u32,
}

impl WireEncode for ExecDirectoryBinding {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.credential_ref.encode(out)?;
        self.target_path.encode(out)?;
        self.access.encode(out)?;
        self.channel_fd.encode(out)
    }
}

impl WireDecode for ExecDirectoryBinding {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            credential_ref: String::decode(reader)?,
            target_path: String::decode(reader)?,
            access: AccessMode::decode(reader)?,
            channel_fd: u32::decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecBundle {
    pub env: HashMap<String, SecretBytes>,
    pub env_passthrough: Vec<String>,
    pub env_set: HashMap<String, String>,
    pub directories: Vec<ExecDirectoryBinding>,
}

impl WireEncode for ExecBundle {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.env.encode(out)?;
        self.env_passthrough.encode(out)?;
        self.env_set.encode(out)?;
        self.directories.encode(out)
    }
}

impl WireDecode for ExecBundle {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            env: HashMap::<String, SecretBytes>::decode(reader)?,
            env_passthrough: Vec::<String>::decode(reader)?,
            env_set: HashMap::<String, String>::decode(reader)?,
            directories: Vec::<ExecDirectoryBinding>::decode(reader)?,
        })
    }
}
