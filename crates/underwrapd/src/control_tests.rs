use super::process_control;
use crate::server::State;
use crate::unlock::UnlockState;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use underwrap_protocol::config::{
    AccessMode, CredentialDef, CredentialMode, CredentialUse, DaemonConfig, SocketConfig,
    StorageConfig, UnlockMode, UnlockPolicy, WrapperDef,
};
use underwrap_protocol::message::{ControlRequest, ControlResponse, ErrorCode, SecretBytes};

struct MemStore {
    data: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemStore {
    fn with_entries(entries: &[(&str, &[u8])]) -> Self {
        Self {
            data: Mutex::new(
                entries
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), v.to_vec()))
                    .collect(),
            ),
        }
    }
}

impl underwrap_store::StorageBackend for MemStore {
    fn read(&self, secret_id: &str) -> anyhow::Result<Option<SecretBytes>> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .get(secret_id)
            .cloned()
            .map(Into::into))
    }

    fn write(&self, secret_id: &str, material: &[u8]) -> anyhow::Result<()> {
        self.data
            .lock()
            .unwrap()
            .insert(secret_id.to_string(), material.to_vec());
        Ok(())
    }

    fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        let mut ids: Vec<_> = self
            .data
            .lock()
            .unwrap()
            .keys()
            .filter(|id| id.starts_with(prefix))
            .cloned()
            .collect();
        ids.sort();
        Ok(ids)
    }

    fn delete(&self, secret_id: &str) -> anyhow::Result<bool> {
        Ok(self.data.lock().unwrap().remove(secret_id).is_some())
    }
}

fn env_credential(variable: &str) -> CredentialDef {
    CredentialDef {
        mode: CredentialMode::Env {
            variable: variable.to_string(),
        },
    }
}

fn directory_credential(path: &str) -> CredentialDef {
    CredentialDef {
        mode: CredentialMode::Directory {
            path: path.to_string(),
        },
    }
}

fn wrapper_with_uses(uses: &[(&str, CredentialUse)]) -> WrapperDef {
    WrapperDef {
        target: PathBuf::from("/usr/bin/true"),
        use_map: uses
            .iter()
            .map(|(name, usage)| ((*name).to_string(), usage.clone()))
            .collect(),
        unlock: UnlockPolicy::default(),
        socket: PathBuf::from("/tmp/test.sock"),
        group: "test-group".to_string(),
        unlock_key: None,
    }
}

fn state_with_config_and_store(
    wrappers: HashMap<String, WrapperDef>,
    credentials: HashMap<String, CredentialDef>,
    allowed_user_uids: std::collections::HashSet<u32>,
    store: MemStore,
) -> State {
    State {
        config: DaemonConfig {
            sockets: SocketConfig::default(),
            storage: StorageConfig::Plain {
                dir: PathBuf::from("/tmp/underwrap-test"),
            },
            allowed_users: Vec::new(),
            credentials,
            wrappers,
        },
        store: Arc::new(store),
        allowed_user_uids,
        unlock: UnlockState::new(),
    }
}

#[test]
fn process_control_status_all_and_status_target_cover_wrapper_and_credential() {
    let wrappers = HashMap::from([(
        "gh-authorized".to_string(),
        wrapper_with_uses(&[(
            "github/token",
            CredentialUse {
                access: AccessMode::ReadOnly,
            },
        )]),
    )]);
    let credentials = HashMap::from([("github/token".to_string(), env_credential("GH_TOKEN"))]);
    let state = state_with_config_and_store(
        wrappers,
        credentials,
        std::collections::HashSet::new(),
        MemStore::with_entries(&[("github/token", b"ghp_test")]),
    );

    let resp = process_control(&ControlRequest::StatusAll, &state, Some(1000));
    assert_eq!(
        resp,
        ControlResponse::StatusAllOk {
            wrappers: vec!["gh-authorized".to_string()],
            credentials: vec!["github/token".to_string()],
        }
    );

    let resp = process_control(
        &ControlRequest::StatusTarget {
            target: "gh-authorized".to_string(),
        },
        &state,
        Some(1000),
    );
    match resp {
        ControlResponse::StatusWrapperOk { status } => {
            assert_eq!(status.wrapper, "gh-authorized");
            assert_eq!(status.usage.len(), 1);
            assert_eq!(status.usage[0].stored, Some(true));
        }
        other => panic!("expected StatusWrapperOk, got {other:?}"),
    }

    let resp = process_control(
        &ControlRequest::StatusTarget {
            target: "github/token".to_string(),
        },
        &state,
        Some(1000),
    );
    match resp {
        ControlResponse::StatusCredentialOk { status } => {
            assert_eq!(status.credential, "github/token");
            assert!(status.stored);
        }
        other => panic!("expected StatusCredentialOk, got {other:?}"),
    }
}

#[test]
fn process_control_unlock_enforces_authorization_rules() {
    let mut wrapper = wrapper_with_uses(&[(
        "github/token",
        CredentialUse {
            access: AccessMode::ReadOnly,
        },
    )]);
    wrapper.unlock = UnlockPolicy {
        mode: UnlockMode::Always,
        max_seconds: Some(30),
    };

    let base_wrappers = HashMap::from([("locked-w".to_string(), wrapper)]);
    let base_credentials =
        HashMap::from([("github/token".to_string(), env_credential("GH_TOKEN"))]);

    let state = state_with_config_and_store(
        base_wrappers.clone(),
        base_credentials.clone(),
        std::collections::HashSet::from([0]),
        MemStore::with_entries(&[]),
    );

    let resp = process_control(
        &ControlRequest::Unlock {
            target: "locked-w".to_string(),
            scope: None,
            seconds: Some(60),
            grant_user: None,
            grant_all: false,
        },
        &state,
        Some(1000),
    );
    match resp {
        ControlResponse::Error { code, message } => {
            assert_eq!(code, ErrorCode::Unauthorized);
            assert!(message.contains("exceeds maxSeconds"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    let resp = process_control(
        &ControlRequest::Unlock {
            target: "locked-w".to_string(),
            scope: None,
            seconds: Some(10),
            grant_user: Some("root".to_string()),
            grant_all: false,
        },
        &state,
        Some(1000),
    );
    match resp {
        ControlResponse::Error { code, message } => {
            assert_eq!(code, ErrorCode::Unauthorized);
            assert!(message.contains("only root may use --for-user or --for-all"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    let resp = process_control(
        &ControlRequest::Unlock {
            target: "locked-w".to_string(),
            scope: None,
            seconds: Some(10),
            grant_user: None,
            grant_all: false,
        },
        &state,
        Some(0),
    );
    match resp {
        ControlResponse::Error { code, message } => {
            assert_eq!(code, ErrorCode::Unauthorized);
            assert!(message.contains("root must specify --for-user or --for-all"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    let not_allowed_state = state_with_config_and_store(
        base_wrappers,
        base_credentials,
        std::collections::HashSet::from([123456]),
        MemStore::with_entries(&[]),
    );
    let resp = process_control(
        &ControlRequest::Unlock {
            target: "locked-w".to_string(),
            scope: None,
            seconds: Some(10),
            grant_user: Some("root".to_string()),
            grant_all: false,
        },
        &not_allowed_state,
        Some(0),
    );
    match resp {
        ControlResponse::Error { code, message } => {
            assert_eq!(code, ErrorCode::Unauthorized);
            assert!(message.contains("is not allowed for this instance"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    let mut wrapper = wrapper_with_uses(&[(
        "github/token",
        CredentialUse {
            access: AccessMode::ReadOnly,
        },
    )]);
    wrapper.unlock = UnlockPolicy {
        mode: UnlockMode::Always,
        max_seconds: Some(30),
    };
    let grant_all_state = state_with_config_and_store(
        HashMap::from([("locked-w".to_string(), wrapper)]),
        HashMap::from([("github/token".to_string(), env_credential("GH_TOKEN"))]),
        std::collections::HashSet::from([0]),
        MemStore::with_entries(&[]),
    );
    let resp = process_control(
        &ControlRequest::Unlock {
            target: "locked-w".to_string(),
            scope: Some("session-1".to_string()),
            seconds: Some(10),
            grant_user: None,
            grant_all: true,
        },
        &grant_all_state,
        Some(0),
    );
    assert_eq!(resp, ControlResponse::UnlockOk);
    assert!(
        grant_all_state
            .unlock
            .check("locked-w", Some("session-1"), 0)
            .is_ok()
    );
    assert!(
        grant_all_state
            .unlock
            .check("locked-w", Some("session-1"), 1000)
            .is_err()
    );
}

#[test]
fn process_control_lock_and_store_rejections() {
    let mut wrapper = wrapper_with_uses(&[(
        "gws/state",
        CredentialUse {
            access: AccessMode::ReadOnly,
        },
    )]);
    wrapper.unlock = UnlockPolicy {
        mode: UnlockMode::Always,
        max_seconds: Some(30),
    };

    let state = state_with_config_and_store(
        HashMap::from([("locked-w".to_string(), wrapper)]),
        HashMap::from([
            ("github/token".to_string(), env_credential("GH_TOKEN")),
            (
                "gws/state".to_string(),
                directory_credential(".config/gcloud"),
            ),
        ]),
        std::collections::HashSet::new(),
        MemStore::with_entries(&[]),
    );

    let resp = process_control(
        &ControlRequest::Unlock {
            target: "locked-w".to_string(),
            scope: Some("session-1".to_string()),
            seconds: Some(10),
            grant_user: None,
            grant_all: false,
        },
        &state,
        Some(1000),
    );
    assert_eq!(resp, ControlResponse::UnlockOk);
    assert!(
        state
            .unlock
            .check("locked-w", Some("session-1"), 1000)
            .is_ok()
    );

    let resp = process_control(
        &ControlRequest::Lock {
            target: Some("locked-w".to_string()),
            scope: Some("session-1".to_string()),
        },
        &state,
        Some(1000),
    );
    assert_eq!(resp, ControlResponse::LockOk);
    assert!(
        state
            .unlock
            .check("locked-w", Some("session-1"), 1000)
            .is_err()
    );

    let resp = process_control(
        &ControlRequest::Store {
            secret_id: "gws/state".to_string(),
            material: "data".into(),
        },
        &state,
        Some(1000),
    );
    match resp {
        ControlResponse::Error { code, message } => {
            assert_eq!(code, ErrorCode::Unauthorized);
            assert!(message.contains("not an env credential"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}
