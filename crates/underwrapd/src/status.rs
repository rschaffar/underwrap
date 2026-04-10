use anyhow::Result;
use underwrap_protocol::config::{CredentialMode, DaemonConfig};
use underwrap_protocol::message::{
    CredentialStatus, CredentialUsageStatus, WrapperStatus, WrapperUnlockStatus,
};
use underwrap_store::StorageBackend;

use crate::unlock::UnlockState;

pub(crate) fn build_wrapper_status(
    name: &str,
    config: &DaemonConfig,
    unlock: &UnlockState,
    store: &dyn StorageBackend,
) -> Option<WrapperStatus> {
    let wrapper = config.wrappers.get(name)?;
    let mut usage = Vec::new();
    for (cred_ref, binding) in &wrapper.use_map {
        let stored = config
            .credentials
            .get(cred_ref)
            .and_then(|credential| credential_stored(cred_ref, credential, store).ok());
        usage.push(CredentialUsageStatus {
            credential: cred_ref.clone(),
            access: binding.access,
            stored,
        });
    }
    usage.sort_by(|a, b| a.credential.cmp(&b.credential));

    Some(WrapperStatus {
        wrapper: name.to_string(),
        target: wrapper.target.display().to_string(),
        usage,
        unlock: WrapperUnlockStatus {
            mode: wrapper.unlock.mode,
            max_seconds: wrapper.unlock.max_seconds,
            status: unlock.status(name),
        },
    })
}

pub(crate) fn build_credential_status(
    name: &str,
    config: &DaemonConfig,
    store: &dyn StorageBackend,
) -> Option<CredentialStatus> {
    let credential = config.credentials.get(name)?;

    let (stored, storage_error) = match credential_stored(name, credential, store) {
        Ok(stored) => (stored, None),
        Err(e) => (false, Some(format!("{e}"))),
    };

    Some(CredentialStatus {
        credential: name.to_string(),
        stored,
        storage_error,
    })
}

pub(crate) fn credential_stored(
    cred_ref: &str,
    credential: &underwrap_protocol::config::CredentialDef,
    store: &dyn StorageBackend,
) -> Result<bool> {
    match &credential.mode {
        CredentialMode::Env { .. } => Ok(store.read(cred_ref)?.is_some()),
        CredentialMode::Libsecret {} => Ok(!store.list(cred_ref)?.is_empty()),
        CredentialMode::Directory { .. } => {
            Ok(!store.list(&format!("{cred_ref}/files"))?.is_empty())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use underwrap_protocol::config::{
        AccessMode, CredentialDef, CredentialUse, UnlockPolicy, WrapperDef,
    };
    use underwrap_protocol::message::SecretBytes;

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

    impl StorageBackend for MemStore {
        fn read(&self, secret_id: &str) -> Result<Option<SecretBytes>> {
            Ok(self
                .data
                .lock()
                .unwrap()
                .get(secret_id)
                .cloned()
                .map(Into::into))
        }

        fn write(&self, secret_id: &str, material: &[u8]) -> Result<()> {
            self.data
                .lock()
                .unwrap()
                .insert(secret_id.to_string(), material.to_vec());
            Ok(())
        }

        fn list(&self, prefix: &str) -> Result<Vec<String>> {
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

        fn delete(&self, secret_id: &str) -> Result<bool> {
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

    fn libsecret_credential() -> CredentialDef {
        CredentialDef {
            mode: CredentialMode::Libsecret {},
        }
    }

    fn directory_credential(path: &str) -> CredentialDef {
        CredentialDef {
            mode: CredentialMode::Directory {
                path: path.to_string(),
            },
        }
    }

    #[test]
    fn credential_stored_checks_env_libsecret_and_directory_namespaces() {
        let store = MemStore::with_entries(&[
            ("github/token", b"ghp_test"),
            ("libsecret/default/item_1", b"secret"),
            ("gws/state/files/access_tokens.json", b"{}"),
        ]);

        assert!(credential_stored("github/token", &env_credential("GH_TOKEN"), &store).unwrap());
        assert!(credential_stored("libsecret/default", &libsecret_credential(), &store).unwrap());
        assert!(
            credential_stored("gws/state", &directory_credential(".config/gcloud"), &store)
                .unwrap()
        );

        assert!(!credential_stored("missing/env", &env_credential("MISSING"), &store).unwrap());
        assert!(!credential_stored("missing/libsecret", &libsecret_credential(), &store).unwrap());
        assert!(
            !credential_stored(
                "missing/dir",
                &directory_credential(".config/missing"),
                &store
            )
            .unwrap()
        );
    }

    #[test]
    fn build_status_helpers_report_wrapper_and_credential_state() {
        let config = DaemonConfig {
            sockets: Default::default(),
            backend: underwrap_protocol::config::BackendSocketConfig {
                socket: PathBuf::from("/tmp/backend.sock"),
            },
            allowed_users: Vec::new(),
            credentials: HashMap::from([("github/token".to_string(), env_credential("GH_TOKEN"))]),
            wrappers: HashMap::from([(
                "gh-authorized".to_string(),
                WrapperDef {
                    target: PathBuf::from("/usr/bin/gh"),
                    use_map: HashMap::from([(
                        "github/token".to_string(),
                        CredentialUse {
                            access: AccessMode::ReadOnly,
                        },
                    )]),
                    unlock: UnlockPolicy::default(),
                    socket: PathBuf::from("/tmp/wrapper.sock"),
                    group: "test-group".to_string(),
                    unlock_key: None,
                },
            )]),
        };
        let unlock = UnlockState::new();
        let store = MemStore::with_entries(&[("github/token", b"ghp_test")]);

        let wrapper_status =
            build_wrapper_status("gh-authorized", &config, &unlock, &store).unwrap();
        assert_eq!(wrapper_status.wrapper, "gh-authorized");
        assert_eq!(wrapper_status.usage.len(), 1);
        assert_eq!(wrapper_status.usage[0].stored, Some(true));

        let credential_status = build_credential_status("github/token", &config, &store).unwrap();
        assert_eq!(credential_status.credential, "github/token");
        assert!(credential_status.stored);
        assert!(credential_status.storage_error.is_none());
    }
}
