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
mod tests;
