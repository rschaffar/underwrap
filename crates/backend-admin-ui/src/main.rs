use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use underwrap_admin_ui::config::{AdminUiConfig, LoadedConfig};
use underwrap_admin_ui::server::{AppState, run};

#[derive(Parser, Debug)]
#[command(
    name = "underwrap-admin-ui",
    about = "Operator UI for the underwrap central backend."
)]
struct Cli {
    /// Path to the admin UI configuration file (YAML).
    #[arg(short, long, default_value = "/etc/underwrap/admin-ui.yaml")]
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

    let text = std::fs::read_to_string(&cli.config)
        .with_context(|| format!("reading admin UI config from {}", cli.config.display()))?;
    let config: AdminUiConfig =
        serde_yaml::from_str(&text).context("parsing admin UI config YAML")?;
    let loaded = LoadedConfig::load(&config)?;
    info!(
        listen = %loaded.listen,
        operators = loaded.operators.len(),
        backends = loaded.backends.len(),
        "admin UI starting"
    );
    let state = Arc::new(AppState::new(loaded));
    run(state, None).await
}
