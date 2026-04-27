use underwrap_protocol::config::{AccessMode, CredentialMode, WrapperDef};
use underwrap_protocol::message::ReadDirectoryBinding;

pub(crate) fn wrapper_libsecret_binding<'a>(
    wrapper: &'a WrapperDef,
    credentials: &'a std::collections::HashMap<String, underwrap_protocol::config::CredentialDef>,
) -> Option<(&'a str, AccessMode)> {
    wrapper.use_map.iter().find_map(|(cred_ref, usage)| {
        match credentials.get(cred_ref).map(|c| &c.mode) {
            Some(CredentialMode::Libsecret {}) => Some((cred_ref.as_str(), usage.access)),
            _ => None,
        }
    })
}

pub(crate) fn wrapper_env_credentials<'a>(
    wrapper: &'a WrapperDef,
    credentials: &'a std::collections::HashMap<String, underwrap_protocol::config::CredentialDef>,
) -> Vec<(&'a str, &'a str)> {
    let mut out = Vec::new();
    for cred_ref in wrapper.use_map.keys() {
        if let Some(underwrap_protocol::config::CredentialDef {
            mode: CredentialMode::Env { variable },
        }) = credentials.get(cred_ref)
        {
            out.push((cred_ref.as_str(), variable.as_str()));
        }
    }
    out
}

pub(crate) fn wrapper_directory_bindings(
    wrapper: &WrapperDef,
    credentials: &std::collections::HashMap<String, underwrap_protocol::config::CredentialDef>,
) -> Vec<ReadDirectoryBinding> {
    let mut out = Vec::new();
    for (cred_ref, usage) in &wrapper.use_map {
        if let Some(underwrap_protocol::config::CredentialDef {
            mode: CredentialMode::Directory { path },
        }) = credentials.get(cred_ref)
        {
            out.push(ReadDirectoryBinding {
                credential_ref: cred_ref.clone(),
                target_path: path.clone(),
                access: usage.access,
            });
        }
    }
    out.sort_by(|a, b| {
        a.target_path
            .cmp(&b.target_path)
            .then(a.credential_ref.cmp(&b.credential_ref))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use underwrap_protocol::config::{CredentialDef, CredentialUse};

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

    fn wrapper_with_uses(uses: &[(&str, CredentialUse)]) -> WrapperDef {
        WrapperDef {
            target: PathBuf::from("/usr/bin/true"),
            use_map: uses
                .iter()
                .map(|(name, usage)| ((*name).to_string(), usage.clone()))
                .collect(),
            socket: PathBuf::new(),
            group: "test-group".to_string(),
        }
    }

    #[test]
    fn wrapper_binding_helpers_select_expected_credentials() {
        let credentials = HashMap::from([
            ("github/token".to_string(), env_credential("GH_TOKEN")),
            ("libsecret/default".to_string(), libsecret_credential()),
            (
                "gws/state".to_string(),
                directory_credential(".config/gcloud"),
            ),
        ]);
        let wrapper = wrapper_with_uses(&[
            (
                "libsecret/default",
                CredentialUse {
                    access: AccessMode::ReadWrite,
                },
            ),
            (
                "github/token",
                CredentialUse {
                    access: AccessMode::ReadOnly,
                },
            ),
            (
                "gws/state",
                CredentialUse {
                    access: AccessMode::ReadWrite,
                },
            ),
        ]);

        assert_eq!(
            wrapper_libsecret_binding(&wrapper, &credentials),
            Some(("libsecret/default", AccessMode::ReadWrite))
        );

        assert_eq!(
            wrapper_env_credentials(&wrapper, &credentials),
            vec![("github/token", "GH_TOKEN")]
        );

        assert_eq!(
            wrapper_directory_bindings(&wrapper, &credentials),
            vec![ReadDirectoryBinding {
                credential_ref: "gws/state".to_string(),
                target_path: ".config/gcloud".to_string(),
                access: AccessMode::ReadWrite,
            }]
        );
    }
}
