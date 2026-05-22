use std::collections::HashMap;

use underwrap_protocol::exec::ExecBundle;

pub(crate) const DEFAULT_PASSTHROUGH: &[&str] = &[
    "HOME",
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
    "XDG_RUNTIME_DIR",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
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

pub(crate) fn build_target_env(
    host_env: &HashMap<String, String>,
    exec_bundle: &ExecBundle,
) -> Result<HashMap<String, String>, String> {
    let mut target_env = HashMap::new();

    for var in DEFAULT_PASSTHROUGH {
        if let Some(value) = host_env.get(*var) {
            target_env.insert((*var).to_string(), value.clone());
        }
    }

    for var in &exec_bundle.env_passthrough {
        if let Some(value) = host_env.get(var) {
            target_env.insert(var.clone(), value.clone());
        }
    }

    for (key, value) in &exec_bundle.env_set {
        target_env.insert(key.clone(), value.clone());
    }

    for (key, value) in &exec_bundle.env {
        let value = value.as_utf8().map_err(|e| {
            format!("env binding '{key}' is not valid UTF-8 and cannot be exported: {e}")
        })?;
        target_env.insert(key.clone(), value.to_string());
    }

    Ok(target_env)
}

#[cfg(test)]
mod tests;
