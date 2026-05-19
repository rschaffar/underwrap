//! Builder helpers for common wrapper and resource test configs.

use std::collections::HashMap;
use std::path::PathBuf;

use underwrap_protocol::config::{FilesUse, ResourceUse, WrapperDef};

/// Get the current process's primary group name (for test wrappers).
fn current_group_name() -> String {
    let gid = unsafe { libc::getegid() };
    let grp = unsafe { libc::getgrgid(gid) };
    if grp.is_null() {
        return gid.to_string();
    }
    let name = unsafe { std::ffi::CStr::from_ptr((*grp).gr_name) };
    name.to_string_lossy().to_string()
}

/// Create a simple env-projecting wrapper def for testing.
pub fn wrapper(target: &str, resource_id: &str, _env_var: &str) -> WrapperDef {
    WrapperDef {
        target: PathBuf::from(target),
        logical_name: None,
        resources: HashMap::from([(
            resource_id.to_string(),
            ResourceUse {
                env: true,
                files: None,
                libsecret: false,
            },
        )]),
        socket: PathBuf::new(),
        group: current_group_name(),
    }
}

pub fn files_resource(path: &str) -> ResourceUse {
    files_resource_paths(&[path])
}

pub fn files_resource_paths(paths: &[&str]) -> ResourceUse {
    ResourceUse {
        env: false,
        files: Some(FilesUse {
            paths: paths.iter().map(|p| (*p).to_string()).collect(),
        }),
        libsecret: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_builder_creates_env_projection() {
        let wrapper = wrapper("/usr/bin/gh", "github/token", "GH_TOKEN");
        assert_eq!(wrapper.target, PathBuf::from("/usr/bin/gh"));
        assert_eq!(wrapper.resources.len(), 1);
        assert!(wrapper.resources.get("github/token").unwrap().env);
    }

    #[test]
    fn files_resource_builder_sets_path() {
        let usage = files_resource(".config/gws");
        assert_eq!(usage.files.unwrap().paths, vec![".config/gws".to_string()]);
    }

    #[test]
    fn files_resource_paths_builder_carries_all_paths() {
        let usage = files_resource_paths(&[".config/gws", ".local/share/gws"]);
        assert_eq!(
            usage.files.unwrap().paths,
            vec![".config/gws".to_string(), ".local/share/gws".to_string()],
        );
    }
}
