use std::path::PathBuf;
use underwrap_protocol::config::{BackendServiceConfig, DaemonConfig};

pub(crate) fn config_with_backend_url(url: &str) -> DaemonConfig {
    DaemonConfig {
        backend: BackendServiceConfig {
            url: url.into(),
            ca_file: None,
            client_identity_file: None,
        },
        instance: "local".into(),
        socket: PathBuf::from("/run/underwrap/local.sock"),
        group: "underwrap-local".into(),
        ns_binary: PathBuf::from("/run/wrappers/bin/underwrap-ns"),
        debug_tool_target: PathBuf::from("/run/current-system/sw/bin/underwrap-debug-tool"),
        target_runtime_dir_base: "/run/user".into(),
        wrappers: Default::default(),
    }
}
