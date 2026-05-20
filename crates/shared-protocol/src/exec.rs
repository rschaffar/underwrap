use crate::config::AccessMode;
use crate::ipc::{WireDecode, WireEncode, WireReader};
use crate::message::SecretBytes;
use anyhow::Result;
use std::collections::HashMap;

/// The wrapper-resource binding flowing through the exec → ns handoff.
/// `channel_fd` is the ns-side fd number that backs the authenticated
/// socketpair to the daemon for this resource; ns drives `Checkout` /
/// `Checkin` / `UsageSignal` over that fd. Each wrapper run has
/// exactly one resource, so the bundle carries exactly one binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecResourceBinding {
    pub resource_id: String,
    pub access: AccessMode,
    pub channel_fd: u32,
    /// Whether secretsd should be launched for this resource.
    pub libsecret: bool,
    /// Target-home-relative paths at which the files staging dir for
    /// this resource should be bind-mounted (always read-write at the
    /// mount layer; the backend gates persistence).
    pub files_target_paths: Vec<String>,
}

impl WireEncode for ExecResourceBinding {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.resource_id.encode(out)?;
        self.access.encode(out)?;
        self.channel_fd.encode(out)?;
        self.libsecret.encode(out)?;
        self.files_target_paths.encode(out)
    }
}

impl WireDecode for ExecResourceBinding {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            resource_id: String::decode(reader)?,
            access: AccessMode::decode(reader)?,
            channel_fd: u32::decode(reader)?,
            libsecret: bool::decode(reader)?,
            files_target_paths: Vec::<String>::decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecBundle {
    pub env: HashMap<String, SecretBytes>,
    pub env_passthrough: Vec<String>,
    pub env_set: HashMap<String, String>,
    /// The wrapper-resource binding for this run's single resource,
    /// carrying the fd for the bulk-blob IPC channel
    /// (`Checkout` / `Checkin` / `UsageSignal`).
    pub resource: ExecResourceBinding,
}

impl WireEncode for ExecBundle {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.env.encode(out)?;
        self.env_passthrough.encode(out)?;
        self.env_set.encode(out)?;
        self.resource.encode(out)
    }
}

impl WireDecode for ExecBundle {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            env: HashMap::<String, SecretBytes>::decode(reader)?,
            env_passthrough: Vec::<String>::decode(reader)?,
            env_set: HashMap::<String, String>::decode(reader)?,
            resource: ExecResourceBinding::decode(reader)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{decode_from_slice, encode_to_vec};

    #[test]
    fn exec_resource_binding_roundtrip() {
        let binding = ExecResourceBinding {
            resource_id: "gws/state".to_string(),
            access: AccessMode::ReadWrite,
            channel_fd: 20,
            libsecret: false,
            files_target_paths: vec![".config/gh".to_string()],
        };

        let encoded = encode_to_vec(&binding).unwrap();
        let decoded: ExecResourceBinding = decode_from_slice(&encoded).unwrap();
        assert_eq!(decoded, binding);
    }

    fn sample_binding() -> ExecResourceBinding {
        ExecResourceBinding {
            resource_id: "gws/state".to_string(),
            access: AccessMode::ReadWrite,
            channel_fd: 20,
            libsecret: false,
            files_target_paths: vec![".config/gh".to_string()],
        }
    }

    #[test]
    fn exec_bundle_roundtrip_minimal() {
        let bundle = ExecBundle {
            env: HashMap::new(),
            env_passthrough: Vec::new(),
            env_set: HashMap::new(),
            resource: ExecResourceBinding {
                resource_id: "x/y".to_string(),
                access: AccessMode::ReadOnly,
                channel_fd: 20,
                libsecret: false,
                files_target_paths: Vec::new(),
            },
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
            resource: sample_binding(),
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
            resource: sample_binding(),
        };

        let mut encoded = encode_to_vec(&bundle).unwrap();
        encoded.push(0);

        let err = decode_from_slice::<ExecBundle>(&encoded).unwrap_err();
        assert!(err.to_string().contains("trailing bytes"));
    }
}
