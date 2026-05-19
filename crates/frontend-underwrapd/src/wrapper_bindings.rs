use underwrap_protocol::config::{FilesUse, ResourceUse, WrapperDef};
use underwrap_protocol::message::ReadDirectoryBinding;

pub(crate) fn wrapper_libsecret_resource(wrapper: &WrapperDef) -> Option<&str> {
    wrapper
        .resources
        .iter()
        .find_map(|(resource_id, usage)| usage.libsecret.then_some(resource_id.as_str()))
}

pub(crate) fn wrapper_env_resources(wrapper: &WrapperDef) -> Vec<&str> {
    let mut out: Vec<&str> = wrapper
        .resources
        .iter()
        .filter_map(|(resource_id, usage)| usage.env.then_some(resource_id.as_str()))
        .collect();
    out.sort();
    out
}

pub(crate) fn wrapper_file_bindings(wrapper: &WrapperDef) -> Vec<FileProjection> {
    let mut out = Vec::new();
    for (resource_id, usage) in &wrapper.resources {
        if let Some(FilesUse { path }) = &usage.files {
            out.push(FileProjection {
                resource_id: resource_id.clone(),
                target_path: path.clone(),
            });
        }
    }
    out.sort_by(|a, b| {
        a.target_path
            .cmp(&b.target_path)
            .then(a.resource_id.cmp(&b.resource_id))
    });
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileProjection {
    pub resource_id: String,
    pub target_path: String,
}

impl FileProjection {
    pub(crate) fn into_read_binding(
        self,
        access: underwrap_protocol::config::AccessMode,
    ) -> ReadDirectoryBinding {
        ReadDirectoryBinding {
            resource_id: self.resource_id,
            target_path: self.target_path,
            access,
        }
    }
}

pub(crate) fn resource_has_projection(usage: &ResourceUse) -> bool {
    usage.env || usage.files.is_some() || usage.libsecret
}

/// Every resource id the wrapper declares across any slot, sorted and
/// deduped. Used by the grant-evaluation gate to consult one grant per
/// resource before the read flow proceeds.
pub(crate) fn wrapper_all_resources(wrapper: &WrapperDef) -> Vec<String> {
    let mut out: Vec<String> = wrapper
        .resources
        .iter()
        .filter(|(_, usage)| resource_has_projection(usage))
        .map(|(id, _)| id.clone())
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use underwrap_protocol::config::{FilesUse, ResourceUse};

    fn wrapper_with_resources(resources: &[(&str, ResourceUse)]) -> WrapperDef {
        WrapperDef {
            target: PathBuf::from("/usr/bin/true"),
            logical_name: None,
            resources: resources
                .iter()
                .map(|(name, usage)| ((*name).to_string(), usage.clone()))
                .collect::<HashMap<_, _>>(),
            socket: PathBuf::new(),
            group: "test-group".to_string(),
        }
    }

    #[test]
    fn wrapper_binding_helpers_select_expected_resources() {
        let wrapper = wrapper_with_resources(&[
            (
                "github/token",
                ResourceUse {
                    env: true,
                    files: None,
                    libsecret: false,
                },
            ),
            (
                "libsecret/default",
                ResourceUse {
                    env: false,
                    files: None,
                    libsecret: true,
                },
            ),
            (
                "gws/state",
                ResourceUse {
                    env: false,
                    files: Some(FilesUse {
                        path: ".config/gcloud".to_string(),
                    }),
                    libsecret: false,
                },
            ),
        ]);

        assert_eq!(
            wrapper_libsecret_resource(&wrapper),
            Some("libsecret/default")
        );
        assert_eq!(wrapper_env_resources(&wrapper), vec!["github/token"]);
        assert_eq!(
            wrapper_file_bindings(&wrapper),
            vec![FileProjection {
                resource_id: "gws/state".to_string(),
                target_path: ".config/gcloud".to_string(),
            }]
        );
    }
}
