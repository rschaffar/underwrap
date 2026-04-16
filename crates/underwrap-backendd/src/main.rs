use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use tracing::info;

use underwrap_backendd::server;

#[derive(Parser, Debug)]
#[command(
    name = "underwrap-backendd",
    about = "underwrap backend daemon — secret storage and derivation server"
)]
struct Cli {
    /// Path to the backend configuration file.
    #[arg(short, long, default_value = "/etc/underwrap/backend.yaml")]
    config: PathBuf,
}

fn parse_backend_config(config_text: &str) -> Result<server::BackendConfig> {
    serde_yaml::from_str(config_text).context("parsing config")
}

#[tokio::main]
async fn main() -> Result<()> {
    // Harden: non-dumpable + no core dumps. The backend daemon holds secrets.
    let ret = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) };
    if ret != 0 {
        eprintln!(
            "underwrap-backendd: prctl(PR_SET_DUMPABLE, 0) failed: {}",
            std::io::Error::last_os_error()
        );
        std::process::exit(1);
    }
    let rlim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    let ret = unsafe { libc::setrlimit(libc::RLIMIT_CORE, &rlim) };
    if ret != 0 {
        eprintln!(
            "underwrap-backendd: setrlimit(RLIMIT_CORE, 0) failed: {}",
            std::io::Error::last_os_error()
        );
        std::process::exit(1);
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    let config_text = std::fs::read_to_string(&cli.config)
        .with_context(|| format!("reading config from {}", cli.config.display()))?;
    let config = parse_backend_config(&config_text)?;

    info!(
        stores = config.stores.len(),
        has_service = config.service.is_some(),
        has_admin = config.admin.is_some(),
        "starting backend daemon"
    );

    server::run(config, None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_parses_config_flag() {
        let cli = Cli::parse_from(["underwrap-backendd", "--config", "/tmp/backend.yaml"]);
        assert_eq!(cli.config, PathBuf::from("/tmp/backend.yaml"));
    }

    #[test]
    fn parse_backend_config_parses_minimal_yaml() {
        let config = parse_backend_config(
            r#"
stores:
  default:
    backend: plain
    dir: /var/lib/underwrap-backend/secrets
"#,
        )
        .unwrap();
        assert!(config.stores.contains_key("default"));
        assert!(config.service.is_none());
        assert!(config.admin.is_none());
    }
}
