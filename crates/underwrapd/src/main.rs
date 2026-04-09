use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "underwrapd", about = "underwrap secret broker daemon")]
struct Cli {
    /// Path to the daemon configuration file.
    #[arg(short, long, default_value = "/etc/underwrap/config.yaml")]
    config: PathBuf,
}

fn parse_daemon_config(config_text: &str) -> Result<underwrap_protocol::config::DaemonConfig> {
    serde_yaml::from_str(config_text).context("parsing config")
}

#[tokio::main]
async fn main() -> Result<()> {
    // Harden: non-dumpable + no core dumps. The daemon holds secrets in memory.
    let ret = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) };
    if ret != 0 {
        eprintln!(
            "underwrapd: prctl(PR_SET_DUMPABLE, 0) failed: {}",
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
            "underwrapd: setrlimit(RLIMIT_CORE, 0) failed: {}",
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
    let config = parse_daemon_config(&config_text)?;

    info!(
        wrappers = config.wrappers.len(),
        credentials = config.credentials.len(),
        "loaded configuration"
    );

    underwrapd::server::run(config, None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_parses_config_flag() {
        let cli = Cli::parse_from(["underwrapd", "--config", "/tmp/test-config.yaml"]);
        assert_eq!(cli.config, PathBuf::from("/tmp/test-config.yaml"));
    }

    #[test]
    fn parse_daemon_config_parses_minimal_yaml() {
        let config = parse_daemon_config(
            r#"
credentials: {}
wrappers: {}
"#,
        )
        .unwrap();
        assert!(config.credentials.is_empty());
        assert!(config.wrappers.is_empty());
    }
}
