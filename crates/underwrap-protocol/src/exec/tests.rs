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
        xdg_runtime_dir_base: "/run/user".to_string(),
        resource: ExecResourceBinding {
            resource_id: "res_x_y".to_string(),
            resource_name: "x/y".to_string(),
            access: AccessMode::ReadOnly,
            channel_fd: 20,
            libsecret: false,
            files_target_paths: Vec::new(),
        },
        access_paths: Vec::new(),
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
        xdg_runtime_dir_base: "/run/underwrap-runtime".to_string(),
        resource: sample_binding(),
        access_paths: vec!["~/.config/gh".to_string(), "/srv/data".to_string()],
    };

    let encoded = encode_to_vec(&bundle).unwrap();
    let decoded: ExecBundle = decode_from_slice(&encoded).unwrap();
    assert_eq!(decoded, bundle);
}

#[test]
fn exec_bundle_carries_no_target_identity() {
    // Authority invariant (story 2600): the dedicated sandbox uid/gid an isolated
    // target is switched to is resolved by underwrap-ns from the root-owned
    // /etc/underwrap/target-uids table, keyed on the daemon-chosen target path —
    // NEVER carried on this wire form (nor env/argv/daemon response). This test
    // pins it: the encoded bundle is byte-exactly its six documented fields
    // concatenated, with nothing extra (e.g. a smuggled uid/gid). If a field is
    // ever added to ExecBundle, both this length check and the exhaustive struct
    // literals in the round-trip tests above fail, forcing a deliberate review of
    // whether the new field belongs on the wire and whether it weakens the
    // confused-deputy guard.
    let bundle = ExecBundle {
        env: HashMap::new(),
        env_passthrough: Vec::new(),
        env_set: HashMap::new(),
        xdg_runtime_dir_base: "/run/user".to_string(),
        resource: sample_binding(),
        access_paths: vec!["~/.config/gh".to_string()],
    };

    let encoded = encode_to_vec(&bundle).unwrap();

    let mut expected = Vec::new();
    bundle.env.encode(&mut expected).unwrap();
    bundle.env_passthrough.encode(&mut expected).unwrap();
    bundle.env_set.encode(&mut expected).unwrap();
    bundle.xdg_runtime_dir_base.encode(&mut expected).unwrap();
    bundle.resource.encode(&mut expected).unwrap();
    bundle.access_paths.encode(&mut expected).unwrap();

    assert_eq!(
        encoded, expected,
        "ExecBundle wire form must be exactly its six documented fields — no \
         target uid/gid or other smuggled identity"
    );
}

#[test]
fn exec_bundle_decode_rejects_trailing_bytes() {
    let bundle = ExecBundle {
        env: HashMap::from([("GH_TOKEN".to_string(), b"ghp_test".as_slice().into())]),
        env_passthrough: vec!["SSH_AUTH_SOCK".to_string()],
        env_set: HashMap::new(),
        xdg_runtime_dir_base: "/run/user".to_string(),
        resource: sample_binding(),
        access_paths: Vec::new(),
    };

    let mut encoded = encode_to_vec(&bundle).unwrap();
    encoded.push(0);

    let err = decode_from_slice::<ExecBundle>(&encoded).unwrap_err();
    assert!(err.to_string().contains("trailing bytes"));
}
