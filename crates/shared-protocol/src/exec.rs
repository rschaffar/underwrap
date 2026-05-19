use crate::config::AccessMode;
use crate::ipc::{WireDecode, WireEncode, WireReader};
use crate::message::SecretBytes;
use anyhow::Result;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecDirectoryBinding {
    pub resource_id: String,
    /// Target-home-relative paths at which the same staging directory
    /// should be bind-mounted. A single `channel_fd` backs all of them
    /// so writeback through one resource cannot race with itself.
    pub target_paths: Vec<String>,
    pub access: AccessMode,
    pub channel_fd: u32,
}

impl WireEncode for ExecDirectoryBinding {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.resource_id.encode(out)?;
        self.target_paths.encode(out)?;
        self.access.encode(out)?;
        self.channel_fd.encode(out)
    }
}

impl WireDecode for ExecDirectoryBinding {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            resource_id: String::decode(reader)?,
            target_paths: Vec::<String>::decode(reader)?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{decode_from_slice, encode_to_vec};

    #[test]
    fn exec_directory_binding_roundtrip() {
        let binding = ExecDirectoryBinding {
            resource_id: "gws/state".to_string(),
            target_paths: vec![".config/gh".to_string()],
            access: AccessMode::ReadWrite,
            channel_fd: 12,
        };

        let encoded = encode_to_vec(&binding).unwrap();
        let decoded: ExecDirectoryBinding = decode_from_slice(&encoded).unwrap();
        assert_eq!(decoded, binding);
    }

    #[test]
    fn exec_bundle_roundtrip_empty() {
        let bundle = ExecBundle {
            env: HashMap::new(),
            env_passthrough: Vec::new(),
            env_set: HashMap::new(),
            directories: Vec::new(),
        };

        let encoded = encode_to_vec(&bundle).unwrap();
        let decoded: ExecBundle = decode_from_slice(&encoded).unwrap();
        assert_eq!(decoded, bundle);
    }

    #[test]
    fn exec_bundle_roundtrip_populated() {
        let bundle = ExecBundle {
            env: HashMap::from([
                ("GH_TOKEN".to_string(), b"ghp_test".as_slice().into()),
                (
                    "AWS_SESSION_TOKEN".to_string(),
                    b"session-token".as_slice().into(),
                ),
            ]),
            env_passthrough: vec!["SSH_AUTH_SOCK".to_string(), "DISPLAY".to_string()],
            env_set: HashMap::from([
                ("PATH".to_string(), "/run/current-system/sw/bin".to_string()),
                ("UNDERWRAP_LIBSECRET".to_string(), "1".to_string()),
            ]),
            directories: vec![
                ExecDirectoryBinding {
                    resource_id: "gws/state".to_string(),
                    target_paths: vec![".config/gh".to_string()],
                    access: AccessMode::ReadWrite,
                    channel_fd: 10,
                },
                ExecDirectoryBinding {
                    resource_id: "aws/cache".to_string(),
                    target_paths: vec![".aws/sso/cache".to_string(), ".cache/aws-sso".to_string()],
                    access: AccessMode::ReadOnly,
                    channel_fd: 11,
                },
            ],
        };

        let encoded = encode_to_vec(&bundle).unwrap();
        let decoded: ExecBundle = decode_from_slice(&encoded).unwrap();
        assert_eq!(decoded, bundle);
    }

    #[test]
    fn exec_bundle_decode_rejects_trailing_bytes() {
        let bundle = ExecBundle {
            env: HashMap::from([("GH_TOKEN".to_string(), b"ghp_test".as_slice().into())]),
            env_passthrough: vec!["SSH_AUTH_SOCK".to_string()],
            env_set: HashMap::new(),
            directories: Vec::new(),
        };

        let mut encoded = encode_to_vec(&bundle).unwrap();
        encoded.push(0);

        let err = decode_from_slice::<ExecBundle>(&encoded).unwrap_err();
        assert!(err.to_string().contains("trailing bytes"));
    }
}
