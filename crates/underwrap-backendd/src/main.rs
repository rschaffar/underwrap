use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use tracing::info;

mod derive;
mod server;

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
    let config: server::BackendConfig =
        serde_yaml::from_str(&config_text).context("parsing config")?;

    info!(
        socket = %config.socket.display(),
        "starting backend daemon"
    );

    server::run(config).await
}
