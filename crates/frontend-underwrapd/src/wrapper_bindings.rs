//! Wrapper-projection helpers.
//!
//! Each `WrapperDef` declares one resource plus the subset of the three
//! projection slots (`env`, `files`, `libsecret`) that resource should
//! populate. These helpers are thin views over those flat fields.

use underwrap_protocol::config::WrapperDef;

/// Target-home-relative bind-mount paths for the wrapper's resource,
/// or empty when the wrapper does not project a `files` slot.
pub(crate) fn wrapper_file_paths(wrapper: &WrapperDef) -> Vec<String> {
    wrapper
        .files
        .as_ref()
        .map(|f| f.paths.clone())
        .unwrap_or_default()
}

/// Whether the wrapper enables at least one projection slot for its
/// resource. A wrapper without any slot would do nothing useful at run
/// time and is rejected by [`crate::startup::validate_config`].
pub(crate) fn wrapper_has_projection(wrapper: &WrapperDef) -> bool {
    wrapper.env || wrapper.files.is_some() || wrapper.libsecret
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use underwrap_protocol::config::FilesUse;

    fn base() -> WrapperDef {
        WrapperDef {
            target: PathBuf::from("/usr/bin/true"),
            logical_name: None,
            resource: "x/y".to_string(),
            env: false,
            files: None,
            libsecret: false,
            socket: PathBuf::new(),
            group: "g".to_string(),
        }
    }

    #[test]
    fn file_paths_empty_when_no_files_slot() {
        let w = base();
        assert!(wrapper_file_paths(&w).is_empty());
    }

    #[test]
    fn file_paths_returned_when_files_slot_present() {
        let mut w = base();
        w.files = Some(FilesUse {
            paths: vec![".config/gws".to_string()],
        });
        assert_eq!(wrapper_file_paths(&w), vec![".config/gws".to_string()]);
    }

    #[test]
    fn has_projection_reflects_any_enabled_slot() {
        let mut w = base();
        assert!(!wrapper_has_projection(&w));
        w.env = true;
        assert!(wrapper_has_projection(&w));
        w.env = false;
        w.libsecret = true;
        assert!(wrapper_has_projection(&w));
        w.libsecret = false;
        w.files = Some(FilesUse {
            paths: vec![".x".to_string()],
        });
        assert!(wrapper_has_projection(&w));
    }
}
