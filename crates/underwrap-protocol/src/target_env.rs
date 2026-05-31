//! Shared target-environment hardening policy.
//!
//! These names are treated specially by the wrapper chain because they
//! steer where the target discovers configuration, caches, state, and
//! runtime sockets. Callers must not control them through ambient
//! passthrough. `HOME` plus the base XDG dirs may be intentionally
//! overridden from trusted wrapper config; `XDG_RUNTIME_DIR` may not.

/// Environment variables underwrap never accepts from caller
/// passthrough when constructing the target environment.
pub const FORCED_TARGET_ENV_NAMES: &[&str] = &[
    "HOME",
    "XDG_RUNTIME_DIR",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
];

/// Subset of [`FORCED_TARGET_ENV_NAMES`] that a wrapper may still
/// override intentionally via trusted `envSet` configuration.
pub const CONFIGURABLE_FORCED_TARGET_ENV_NAMES: &[&str] = &[
    "HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
];

pub fn is_forced_target_env_name(name: &str) -> bool {
    FORCED_TARGET_ENV_NAMES.contains(&name)
}

pub fn is_home_or_base_xdg_dir_env_name(name: &str) -> bool {
    CONFIGURABLE_FORCED_TARGET_ENV_NAMES.contains(&name)
}

#[cfg(test)]
mod tests;
