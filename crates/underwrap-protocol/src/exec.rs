use crate::ipc::{WireDecode, WireEncode, WireReader};
use crate::message::SecretBytes;
use anyhow::Result;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecBundle {
    pub env: HashMap<String, SecretBytes>,
}

impl WireEncode for ExecBundle {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.env.encode(out)
    }
}

impl WireDecode for ExecBundle {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            env: HashMap::<String, SecretBytes>::decode(reader)?,
        })
    }
}
