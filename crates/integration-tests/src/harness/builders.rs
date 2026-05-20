//! Builder helpers for common wrapper test configs.

use std::path::PathBuf;

use underwrap_protocol::config::{FilesUse, WrapperDef};

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

/// Simple env-projecting wrapper def for testing.
pub fn wrapper(target: &str, resource_id: &str, _env_var: &str) -> WrapperDef {
    WrapperDef {
        target: PathBuf::from(target),
        logical_name: None,
        resource: resource_id.to_string(),
        env: true,
        files: None,
        libsecret: false,
        socket: PathBuf::new(),
        group: current_group_name(),
    }
}

/// Wrapper def projecting a single resource into its files slot at
/// `path`.
pub fn files_wrapper(target: &str, resource_id: &str, path: &str) -> WrapperDef {
    files_wrapper_paths(target, resource_id, &[path])
}

/// Wrapper def projecting a single resource into its files slot at
/// multiple `paths`.
pub fn files_wrapper_paths(target: &str, resource_id: &str, paths: &[&str]) -> WrapperDef {
    WrapperDef {
        target: PathBuf::from(target),
        logical_name: None,
        resource: resource_id.to_string(),
        env: false,
        files: Some(FilesUse {
            paths: paths.iter().map(|p| (*p).to_string()).collect(),
        }),
        libsecret: false,
        socket: PathBuf::new(),
        group: current_group_name(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_builder_creates_env_projection() {
        let wrapper = wrapper("/usr/bin/gh", "github/token", "GH_TOKEN");
        assert_eq!(wrapper.target, PathBuf::from("/usr/bin/gh"));
        assert_eq!(wrapper.resource, "github/token");
        assert!(wrapper.env);
        assert!(wrapper.files.is_none());
        assert!(!wrapper.libsecret);
    }

    #[test]
    fn files_wrapper_builder_sets_path() {
        let w = files_wrapper("/usr/bin/true", "gws/state", ".config/gws");
        assert_eq!(w.resource, "gws/state");
        assert_eq!(
            w.files.as_ref().unwrap().paths,
            vec![".config/gws".to_string()]
        );
    }

    #[test]
    fn files_wrapper_paths_builder_carries_all_paths() {
        let w = files_wrapper_paths(
            "/usr/bin/true",
            "gws/state",
            &[".config/gws", ".local/share/gws"],
        );
        assert_eq!(
            w.files.unwrap().paths,
            vec![".config/gws".to_string(), ".local/share/gws".to_string()],
        );
    }
}
