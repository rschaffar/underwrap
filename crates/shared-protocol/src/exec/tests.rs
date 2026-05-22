use super::*;
use crate::ipc::{decode_from_slice, encode_to_vec};

#[test]
fn exec_resource_binding_roundtrip() {
    let binding = ExecResourceBinding {
        resource_id: "res_gws_state".to_string(),
        resource_name: "gws/state".to_string(),
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
        resource_id: "res_gws_state".to_string(),
        resource_name: "gws/state".to_string(),
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
            resource_id: "res_x_y".to_string(),
            resource_name: "x/y".to_string(),
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
