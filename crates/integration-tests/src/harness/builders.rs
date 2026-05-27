//! Builder helpers for common wrapper test configs.

use std::path::PathBuf;

use underwrap_protocol::config::{FilesUse, WrapperDef};

/// Get the current process's primary group name. The harness uses this
/// as the single instance group so the daemon's `egid == instance gid`
/// gate passes for the in-process test connections.
pub(crate) fn current_group_name() -> String {
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
        resource: resource_id.to_string(),
        env: true,
        files: None,
        libsecret: false,
        env_passthrough: Vec::new(),
        env_set: std::collections::HashMap::new(),
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
        resource: resource_id.to_string(),
        env: false,
        files: Some(FilesUse {
            paths: paths.iter().map(|p| (*p).to_string()).collect(),
        }),
        libsecret: false,
        env_passthrough: Vec::new(),
        env_set: std::collections::HashMap::new(),
    }
}

#[cfg(test)]
mod tests;
