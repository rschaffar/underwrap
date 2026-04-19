//! Builder helpers for common wrapper and credential test configs.

use std::collections::HashMap;
use std::path::PathBuf;

use underwrap_protocol::config::{
    AccessMode, CredentialDef, CredentialMode, CredentialUse, WrapperDef,
};

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

/// Create a simple wrapper def for testing.
pub fn wrapper(target: &str, secret: &str, _env_var: &str) -> WrapperDef {
    let mut use_map = HashMap::new();
    use_map.insert(
        secret.to_string(),
        CredentialUse {
            access: AccessMode::ReadOnly,
        },
    );
    WrapperDef {
        target: PathBuf::from(target),
        use_map,
        socket: PathBuf::new(),
        group: current_group_name(),
    }
}

/// Create a simple env credential def for testing.
pub fn secret(_mutable: bool) -> CredentialDef {
    CredentialDef {
        mode: CredentialMode::Env {
            variable: "UNDERWRAP_SECRET".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_builder_creates_read_only_usage() {
        let wrapper = wrapper("/usr/bin/gh", "github/token", "GH_TOKEN");
        assert_eq!(wrapper.target, PathBuf::from("/usr/bin/gh"));
        assert_eq!(wrapper.use_map.len(), 1);
        assert_eq!(
            wrapper.use_map.get("github/token").unwrap().access,
            AccessMode::ReadOnly
        );
    }

    #[test]
    fn secret_builder_creates_env_credential() {
        let credential = secret(false);
        match credential.mode {
            CredentialMode::Env { variable } => assert_eq!(variable, "UNDERWRAP_SECRET"),
            other => panic!("expected env credential, got {other:?}"),
        }
    }
}
