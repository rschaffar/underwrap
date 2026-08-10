use std::collections::HashMap;
use std::path::Path;

use underwrap_protocol::exec::ExecBundle;
use underwrap_protocol::resource::validate_env_name;
use underwrap_protocol::resource_blob::ResourceBlob;
use underwrap_protocol::target_env::{
    CONFIGURABLE_FORCED_TARGET_ENV_NAMES, is_forced_target_env_name,
};

pub(crate) const DEFAULT_PASSTHROUGH: &[&str] = &[
    "USER",
    "LOGNAME",
    "SHELL",
    "TERM",
    "COLORTERM",
    "LANG",
    "LC_ALL",
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_TIME",
    "TZ",
    "TMPDIR",
    "SSH_AUTH_SOCK",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
];

fn joined_path(base: &str, tail: &str) -> Result<String, String> {
    Path::new(base)
        .join(tail)
        .to_str()
        .map(str::to_string)
        .ok_or_else(|| format!("derived path under {:?} is not valid UTF-8", base))
}

/// Validate the environment exactly as it will be passed to the target.
///
/// All sources must be composed first so precedence and forced paths stay
/// unchanged while no final key/value can make process setup fail after
/// projection planning has completed.
fn validate_final_target_env(target_env: &HashMap<String, String>) -> Result<(), String> {
    for (key, value) in target_env {
        if key.contains('=') || key.contains('\0') {
            return Err(format!(
                "target environment key {key:?} contains '=' or NUL"
            ));
        }
        if value.contains('\0') {
            return Err(format!(
                "target environment value for key {key:?} contains NUL"
            ));
        }
    }
    Ok(())
}

pub(crate) fn build_target_env(
    host_env: &HashMap<String, String>,
    exec_bundle: &ExecBundle,
    checkout_blob: &ResourceBlob,
    passwd_home: &Path,
    xdg_runtime_dir: &str,
) -> Result<HashMap<String, String>, String> {
    let mut target_env = HashMap::new();

    for var in DEFAULT_PASSTHROUGH {
        if let Some(value) = host_env.get(*var) {
            target_env.insert((*var).to_string(), value.clone());
        }
    }

    let passwd_home = passwd_home
        .to_str()
        .ok_or_else(|| format!("passwd home {} is not valid UTF-8", passwd_home.display()))?;
    let effective_home = exec_bundle
        .env_set
        .get("HOME")
        .cloned()
        .unwrap_or_else(|| passwd_home.to_string());
    if !Path::new(&effective_home).is_absolute() {
        return Err(format!(
            "effective HOME must be an absolute path, got {:?}",
            effective_home
        ));
    }
    if !Path::new(xdg_runtime_dir).is_absolute() {
        return Err(format!(
            "XDG_RUNTIME_DIR must be absolute, got {:?}",
            xdg_runtime_dir
        ));
    }

    target_env.insert("HOME".to_string(), effective_home.clone());
    target_env.insert("XDG_RUNTIME_DIR".to_string(), xdg_runtime_dir.to_string());
    target_env.insert(
        "XDG_CONFIG_HOME".to_string(),
        joined_path(&effective_home, ".config")?,
    );
    target_env.insert(
        "XDG_DATA_HOME".to_string(),
        joined_path(&effective_home, ".local/share")?,
    );
    target_env.insert(
        "XDG_CACHE_HOME".to_string(),
        joined_path(&effective_home, ".cache")?,
    );
    target_env.insert(
        "XDG_STATE_HOME".to_string(),
        joined_path(&effective_home, ".local/state")?,
    );

    for var in &exec_bundle.env_passthrough {
        if is_forced_target_env_name(var) {
            continue;
        }
        if let Some(value) = host_env.get(var) {
            target_env.insert(var.clone(), value.clone());
        }
    }

    for name in CONFIGURABLE_FORCED_TARGET_ENV_NAMES {
        if let Some(value) = exec_bundle.env_set.get(*name)
            && !Path::new(value).is_absolute()
        {
            return Err(format!(
                "env_set {name} must be an absolute path, got {:?}",
                value
            ));
        }
    }

    for (key, value) in &exec_bundle.env_set {
        if key == "XDG_RUNTIME_DIR" {
            continue;
        }
        target_env.insert(key.clone(), value.clone());
    }

    if exec_bundle.resource.env_projection {
        for (key, value) in &checkout_blob.env {
            validate_env_name(key).map_err(|reason| {
                format!("Checkout env binding '{key}' is invalid for projection: {reason}")
            })?;
            let value = value.as_utf8().map_err(|e| {
                format!(
                    "Checkout env binding '{key}' is not valid UTF-8 and cannot be exported: {e}"
                )
            })?;
            target_env.insert(key.clone(), value.to_string());
        }
    }

    validate_final_target_env(&target_env)?;
    Ok(target_env)
}

#[cfg(test)]
mod tests;
