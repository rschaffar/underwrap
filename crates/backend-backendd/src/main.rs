use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use tracing::info;

use underwrap_backendd::{BackendConfig, server, users};

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

fn parse_backend_config(config_text: &str) -> Result<BackendConfig> {
    // Reject the legacy inline `stores:` block up front so an operator
    // gets a clear migration hint instead of a confusing "unknown field"
    // or, worse, a silently-ignored map.
    let raw: serde_yaml::Value =
        serde_yaml::from_str(config_text).context("parsing config as YAML")?;
    if let serde_yaml::Value::Mapping(map) = &raw
        && map.contains_key(serde_yaml::Value::String("stores".into()))
    {
        anyhow::bail!(
            "the inline `stores:` map in backend.yaml is no longer supported. \
             Stores now live in the backend-managed store registry file \
             (`stores_file`, default /var/lib/underwrap-backend/stores.json)."
        );
    }
    serde_yaml::from_value(raw).context("parsing config")
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
    let users_file = config.users_file();
    match cmd {
        AdminCommand::RecoverUser { username } => {
            let mut password = String::new();
            std::io::stdin()
                .read_line(&mut password)
                .context("reading password from stdin")?;
            let password = password.trim_end_matches(['\r', '\n']).to_string();
            recover_user(&users_file, &username, &password)?;
            info!(path = %users_file.display(), username = %username, "recover-user: user added");
            Ok(())
        }
    }
}

/// Add a user to the on-disk collection. Refuses if a user with the
/// same username already exists. Doesn't touch the
/// bootstrap-completed sentinel — recovery is intentionally not a
/// substitute for the online bootstrap (which is what locks the
/// endpoint going forward).
fn recover_user(path: &Path, username: &str, password: &str) -> Result<()> {
    if password.is_empty() {
        anyhow::bail!("recover-user: password must not be empty");
    }
    let normalised = users::normalise_username(username);
    if normalised.is_empty() {
        anyhow::bail!("recover-user: username must not be empty");
    }
    let mut collection = users::load(path)?;
    if collection.find_by_username(&normalised).is_some() {
        anyhow::bail!(
            "recover-user: user '{}' already exists; refusing to overwrite",
            normalised
        );
    }
    let hash = underwrap_backendd::auth::hash_password(password)?;
    let now = users::now_epoch_secs();
    collection.add(users::UserRecord {
        id: users::new_user_id(),
        username: normalised,
        password_hash: hash,
        created_at: now,
        password_changed_at: now,
    })?;
    users::save(path, &collection)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_parses_config_flag() {
        let cli = Cli::parse_from(["underwrap-backendd", "--config", "/tmp/backend.yaml"]);
        assert_eq!(cli.config, PathBuf::from("/tmp/backend.yaml"));
        assert!(cli.command.is_none());
    }

    #[test]
    fn parse_backend_config_parses_minimal_yaml() {
        // Empty YAML works because the backend-managed store registry
        // path and https+admin blocks are optional.
        let config = parse_backend_config("{}\n").unwrap();
        assert!(config.stores_file.is_none());
        assert!(config.client_instances_file.is_none());
        assert!(config.https.is_none());
        assert!(config.admin.is_none());
    }

    #[test]
    fn parse_backend_config_rejects_legacy_inline_stores_with_migration_hint() {
        let err = parse_backend_config(
            r#"
stores:
  default:
    backend: plain
    dir: /var/lib/underwrap-backend/secrets
"#,
        )
        .expect_err("inline `stores:` must be rejected after the store-registry refactor");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("stores_file") && msg.contains("no longer supported"),
            "error message must guide the operator to the new layout, got: {msg}",
        );
    }

    #[test]
    fn parse_backend_config_accepts_dir_overrides() {
        let config = parse_backend_config(
            r#"
stores_file: /var/u/stores.json
client_instances_file: /var/u/client-instances.json
users_file: /var/u/users.json
"#,
        )
        .unwrap();
        assert_eq!(
            config.stores_file(),
            std::path::PathBuf::from("/var/u/stores.json"),
        );
        assert_eq!(
            config.client_instances_file(),
            std::path::PathBuf::from("/var/u/client-instances.json"),
        );
        assert_eq!(
            config.users_file(),
            std::path::PathBuf::from("/var/u/users.json"),
        );
    }

    #[test]
    fn cli_parses_recover_user_subcommand() {
        let cli = Cli::parse_from([
            "underwrap-backendd",
            "admin",
            "recover-user",
            "--username",
            "Alice",
        ]);
        match cli.command {
            Some(Command::Admin(AdminCommand::RecoverUser { username })) => {
                assert_eq!(username, "Alice");
            }
            _ => panic!("expected recover-user subcommand, got {:?}", cli.command),
        }
    }

    #[test]
    fn recover_user_writes_user_to_empty_state_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let users_file = dir.path().join("users.json");

        recover_user(&users_file, "Alice", "hunter2").unwrap();

        // Read it back: user landed in the state file with a
        // verifiable password.
        let coll = users::load(&users_file).unwrap();
        let u = coll.find_by_username("alice").unwrap();
        assert!(underwrap_backendd::auth::verify_password(
            &u.password_hash,
            "hunter2"
        ));
        // Sentinel is NOT set — recovery deliberately stays "offline".
        assert!(!users::bootstrap_completed(&users_file).unwrap());
    }

    #[test]
    fn recover_user_refuses_existing_username() {
        let dir = tempfile::TempDir::new().unwrap();
        let users_file = dir.path().join("users.json");
        recover_user(&users_file, "alice", "p1").unwrap();
        let err = recover_user(&users_file, "Alice", "p2").unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn recover_user_rejects_empty_password() {
        let dir = tempfile::TempDir::new().unwrap();
        let users_file = dir.path().join("users.json");
        let err = recover_user(&users_file, "alice", "").unwrap_err();
        assert!(err.to_string().contains("password"));
    }
}
