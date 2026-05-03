//! Resource slot naming helpers.
//!
//! A backend resource always has three slots:
//!
//! - `<resource>/env/<VAR>`
//! - `<resource>/files/<relative-path>`
//! - `<resource>/libsecret/<relative-id>`
//!
//! Policy is slot-scoped via `env:<resource>`, `files:<resource>`, and
//! `libsecret:<resource>`.

pub const ENV_SLOT: &str = "env";
pub const FILES_SLOT: &str = "files";
pub const LIBSECRET_SLOT: &str = "libsecret";

pub fn env_prefix(resource_id: &str) -> String {
    format!("{resource_id}/{ENV_SLOT}")
}

pub fn files_prefix(resource_id: &str) -> String {
    format!("{resource_id}/{FILES_SLOT}")
}

pub fn libsecret_prefix(resource_id: &str) -> String {
    format!("{resource_id}/{LIBSECRET_SLOT}")
}

pub fn env_resource(resource_id: &str) -> String {
    format!("env:{resource_id}")
}

pub fn files_resource(resource_id: &str) -> String {
    format!("files:{resource_id}")
}

pub fn libsecret_resource(resource_id: &str) -> String {
    format!("libsecret:{resource_id}")
}

pub fn relative_to_prefix<'a>(id: &'a str, prefix: &str) -> Option<&'a str> {
    id.strip_prefix(prefix)?.strip_prefix('/')
}

pub fn has_traversal(path: &str) -> bool {
    path.is_empty()
        || path.starts_with('/')
        || path.contains('\0')
        || path
            .split('/')
            .any(|segment| segment == "." || segment == ".." || segment.is_empty())
}

pub fn validate_env_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("environment variable name is empty");
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err("environment variable name is empty");
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return Err("environment variable name must start with '_' or an ASCII letter");
    }
    if !chars.all(|c| c == '_' || c.is_ascii_alphanumeric()) {
        return Err("environment variable name contains invalid characters");
    }
    if is_reserved_env_name(name) {
        return Err("environment variable name is reserved by underwrap hardening");
    }
    Ok(())
}

pub fn is_reserved_env_name(name: &str) -> bool {
    name.starts_with("LD_")
        || matches!(
            name,
            "LD_PRELOAD"
                | "LD_LIBRARY_PATH"
                | "LD_AUDIT"
                | "PATH"
                | "HOME"
                | "USER"
                | "LOGNAME"
                | "SHELL"
                | "DBUS_SESSION_BUS_ADDRESS"
                | "SSH_AUTH_SOCK"
        )
        || name.starts_with("UNDERWRAP_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_are_stable() {
        assert_eq!(env_prefix("github/token"), "github/token/env");
        assert_eq!(files_prefix("gws/castlabs"), "gws/castlabs/files");
        assert_eq!(libsecret_prefix("gh"), "gh/libsecret");
    }

    #[test]
    fn env_validation_rejects_dangerous_names() {
        assert!(validate_env_name("GH_TOKEN").is_ok());
        assert!(validate_env_name("_TOKEN1").is_ok());
        assert!(validate_env_name("LD_PRELOAD").is_err());
        assert!(validate_env_name("PATH").is_err());
        assert!(validate_env_name("UNDERWRAP_SECRETSD").is_err());
        assert!(validate_env_name("1TOKEN").is_err());
        assert!(validate_env_name("BAD-NAME").is_err());
    }
}
