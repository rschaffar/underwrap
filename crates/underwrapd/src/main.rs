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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    let config_text = std::fs::read_to_string(&cli.config)
        .with_context(|| format!("reading config from {}", cli.config.display()))?;
    let config: underwrap_protocol::config::DaemonConfig =
        serde_yaml::from_str(&config_text).context("parsing config")?;

    info!(
        wrappers = config.wrappers.len(),
        secrets = config.secrets.len(),
        "loaded configuration"
    );

    underwrapd::server::run(config, None).await
}
