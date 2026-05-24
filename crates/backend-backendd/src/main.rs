use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::info;

use underwrap_backendd::{BackendConfig, cli::recover_user, server};

#[derive(Parser, Debug)]
#[command(
    name = "underwrap-backendd",
    about = "underwrap backend daemon — secret storage and derivation server"
)]
struct Cli {
    /// Path to the backend configuration file.
    #[arg(short, long, default_value = "/etc/underwrap/backend.yaml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the daemon. Default when no subcommand is given.
    Run,
    /// Administrative one-shot operations against the backend store.
    /// These read/write directly on disk and do not require the
    /// running daemon. Intended to be invoked as the
    /// `underwrap-backend` system user.
    #[command(subcommand)]
    Admin(AdminCommand),
}

#[derive(Subcommand, Debug)]
enum AdminCommand {
    /// Offline recovery: append (or refuse-on-conflict) a user
    /// directly to the backend-managed user-state file. Use when
    /// nobody can log in to the running daemon. This subcommand
    /// intentionally does NOT touch the bootstrap-completed sentinel:
    /// it's the escape hatch for the locked-out case, not a
    /// replacement for the online bootstrap flow.
    ///
    /// Requires physical/file-system access to the backend state dir.
    /// The password is read from stdin to keep it out of the process
    /// argv (which would otherwise leak to `/proc/<pid>/cmdline`).
    RecoverUser {
        /// Username to add. Will be normalised (trimmed + lowercased).
        #[arg(long)]
        username: String,
    },
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
    let config = underwrap_backendd::cli::parse_backend_config(&config_text)?;

    match cli.command.unwrap_or(Command::Run) {
        Command::Run => {
            info!(
                has_https = config.https.is_some(),
                has_admin = config.admin.is_some(),
                "starting backend daemon"
            );
            server::run(config, None).await
        }
        Command::Admin(admin) => run_admin(admin, config),
    }
}

fn run_admin(cmd: AdminCommand, config: BackendConfig) -> Result<()> {
    let users_dir = config.users_dir.clone();
    match cmd {
        AdminCommand::RecoverUser { username } => {
            let mut password = String::new();
            std::io::stdin()
                .read_line(&mut password)
                .context("reading password from stdin")?;
            let password = password.trim_end_matches(['\r', '\n']).to_string();
            recover_user(&users_dir, &username, &password)?;
            info!(dir = %users_dir.display(), username = %username, "recover-user: user added");
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
