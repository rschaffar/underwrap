use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use underwrap_backendd::{operators, server};

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
    /// Offline recovery: append (or refuse-on-conflict) an admin
    /// operator directly to the admin-home store. Use when nobody can
    /// log in to the running daemon. This subcommand intentionally
    /// does NOT touch the bootstrap-completed sentinel: it's the
    /// escape hatch for the locked-out case, not a replacement for
    /// the online bootstrap flow.
    ///
    /// Requires physical/file-system access to the store. The password
    /// is read from stdin to keep it out of the process argv (which
    /// would otherwise leak to `/proc/<pid>/cmdline`).
    RecoverOperator {
        /// Username to add. Will be normalised (trimmed + lowercased).
        #[arg(long)]
        username: String,
        /// Which store in the backend config to write to. Defaults to
        /// the admin-home store resolved from config.
        #[arg(long)]
        store: Option<String>,
    },
}

fn parse_backend_config(config_text: &str) -> Result<server::BackendConfig> {
    // Reject the pre-230 inline `stores:` block up front so an operator
    // upgrading an older config gets a clear migration hint instead of a
    // confusing "unknown field" or, worse, a silently-ignored map.
    let raw: serde_yaml::Value =
        serde_yaml::from_str(config_text).context("parsing config as YAML")?;
    if let serde_yaml::Value::Mapping(map) = &raw
        && map.contains_key(serde_yaml::Value::String("stores".into()))
    {
        anyhow::bail!(
            "the inline `stores:` map in backend.yaml was removed in the stores.d \
             refactor. Move each store into its own \
             `<name>.yaml` under `stores_dynamic_dir` \
             (or `stores_declarative_dir`, default `/etc/underwrap-backend/stores.d/`). \
             Each file contains the storage config (`backend: plain|encrypted`, `dir`, …) \
             plus optional `description` / `created_at` metadata."
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
                has_service = config.service.is_some(),
                has_admin = config.admin.is_some(),
                "starting backend daemon"
            );
            server::run(config, None).await
        }
        Command::Admin(admin) => run_admin(admin, config),
    }
}

fn run_admin(cmd: AdminCommand, config: server::BackendConfig) -> Result<()> {
    let definitions =
        server::load_store_definitions(&config.declarative_dir(), &config.dynamic_dir())?;
    let stores = open_stores_for_admin(&definitions)?;
    let configured_home = config
        .admin
        .as_ref()
        .and_then(|a| a.admin_home_store.as_deref());
    match cmd {
        AdminCommand::RecoverOperator { username, store } => {
            let target = match store.as_deref().or(configured_home) {
                Some(name) => {
                    let s = stores
                        .get(name)
                        .with_context(|| format!("--store '{name}' is not declared in stores"))?;
                    (name.to_string(), s.clone())
                }
                None => {
                    let (n, s) = operators::resolve_admin_home_store(&stores, None)?;
                    (n.to_string(), s)
                }
            };
            let mut password = String::new();
            std::io::stdin()
                .read_line(&mut password)
                .context("reading password from stdin")?;
            let password = password.trim_end_matches(['\r', '\n']).to_string();
            recover_operator(target.1.as_ref(), &username, &password)?;
            info!(store = %target.0, username = %username, "recover-operator: admin added");
            Ok(())
        }
    }
}

fn open_stores_for_admin(
    definitions: &HashMap<String, server::LoadedStoreDef>,
) -> Result<HashMap<String, Arc<dyn underwrap_store::StorageBackend>>> {
    let mut out = HashMap::new();
    for (name, def) in definitions {
        let store: Arc<dyn underwrap_store::StorageBackend> =
            Arc::from(underwrap_store::open(&def.definition.storage)?);
        out.insert(name.clone(), store);
    }
    Ok(out)
}

/// Add an admin operator to the on-disk collection. Refuses if an
/// operator with the same username already exists. Doesn't touch the
/// bootstrap-completed sentinel — recovery is intentionally not a
/// substitute for the online bootstrap (which is what locks the
/// endpoint going forward).
fn recover_operator(
    store: &dyn underwrap_store::StorageBackend,
    username: &str,
    password: &str,
) -> Result<()> {
    if password.is_empty() {
        anyhow::bail!("recover-operator: password must not be empty");
    }
    let normalised = operators::normalise_username(username);
    if normalised.is_empty() {
        anyhow::bail!("recover-operator: username must not be empty");
    }
    let mut collection = operators::load(store)?;
    if collection.find_by_username(&normalised).is_some() {
        anyhow::bail!(
            "recover-operator: operator '{}' already exists; refusing to overwrite",
            normalised
        );
    }
    let hash = underwrap_backendd::auth::hash_password(password)?;
    let now = operators::now_epoch_secs();
    collection.add(operators::OperatorRecord {
        id: operators::new_operator_id(),
        username: normalised,
        password_hash: hash,
        role: operators::OperatorRole::Admin,
        granted_stores: Vec::new(),
        created_at: now,
        password_changed_at: now,
    })?;
    operators::save(store, &collection)?;
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
        // Empty YAML works because both store dirs and service+admin
        // are optional. Tests that need stores write per-store YAML
        // files under a temp `stores.d/` instead.
        let config = parse_backend_config("{}\n").unwrap();
        assert!(config.stores_declarative_dir.is_none());
        assert!(config.stores_dynamic_dir.is_none());
        assert!(config.service.is_none());
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
        .expect_err("inline `stores:` must be rejected after the stores.d refactor");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("stores.d") && msg.contains("removed"),
            "error message must guide the operator to the new layout, got: {msg}",
        );
    }

    #[test]
    fn parse_backend_config_accepts_dir_overrides() {
        let config = parse_backend_config(
            "stores_declarative_dir: /etc/u/decl\n\
             stores_dynamic_dir: /var/u/dyn\n",
        )
        .unwrap();
        assert_eq!(
            config.declarative_dir(),
            std::path::PathBuf::from("/etc/u/decl"),
        );
        assert_eq!(config.dynamic_dir(), std::path::PathBuf::from("/var/u/dyn"),);
    }

    #[test]
    fn cli_parses_recover_operator_subcommand() {
        let cli = Cli::parse_from([
            "underwrap-backendd",
            "admin",
            "recover-operator",
            "--username",
            "Alice",
            "--store",
            "home",
        ]);
        match cli.command {
            Some(Command::Admin(AdminCommand::RecoverOperator { username, store })) => {
                assert_eq!(username, "Alice");
                assert_eq!(store.as_deref(), Some("home"));
            }
            _ => panic!(
                "expected recover-operator subcommand, got {:?}",
                cli.command
            ),
        }
    }

    #[test]
    fn recover_operator_writes_admin_to_empty_store() {
        // Direct disk operation: no running daemon.
        let dir = tempfile::TempDir::new().unwrap();
        let store_dir = dir.path().join("store");
        std::fs::create_dir_all(&store_dir).unwrap();
        let cfg = underwrap_protocol::config::StorageConfig::Plain {
            dir: store_dir.clone(),
        };
        let backend: Arc<dyn underwrap_store::StorageBackend> =
            Arc::from(underwrap_store::open(&cfg).unwrap());

        recover_operator(backend.as_ref(), "Alice", "hunter2").unwrap();

        // Read it back: operator landed in the store with admin role
        // and a verifiable password.
        let coll = operators::load(backend.as_ref()).unwrap();
        let op = coll.find_by_username("alice").unwrap();
        assert_eq!(op.role, operators::OperatorRole::Admin);
        assert!(underwrap_backendd::auth::verify_password(
            &op.password_hash,
            "hunter2"
        ));
        // Sentinel is NOT set — recovery deliberately stays "offline".
        assert!(!operators::bootstrap_completed(backend.as_ref()).unwrap());
    }

    #[test]
    fn recover_operator_refuses_existing_username() {
        let dir = tempfile::TempDir::new().unwrap();
        let store_dir = dir.path().join("store");
        std::fs::create_dir_all(&store_dir).unwrap();
        let cfg = underwrap_protocol::config::StorageConfig::Plain { dir: store_dir };
        let backend: Arc<dyn underwrap_store::StorageBackend> =
            Arc::from(underwrap_store::open(&cfg).unwrap());
        recover_operator(backend.as_ref(), "alice", "p1").unwrap();
        let err = recover_operator(backend.as_ref(), "Alice", "p2").unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn recover_operator_rejects_empty_password() {
        let dir = tempfile::TempDir::new().unwrap();
        let store_dir = dir.path().join("store");
        std::fs::create_dir_all(&store_dir).unwrap();
        let cfg = underwrap_protocol::config::StorageConfig::Plain { dir: store_dir };
        let backend: Arc<dyn underwrap_store::StorageBackend> =
            Arc::from(underwrap_store::open(&cfg).unwrap());
        let err = recover_operator(backend.as_ref(), "alice", "").unwrap_err();
        assert!(err.to_string().contains("password"));
    }
}
