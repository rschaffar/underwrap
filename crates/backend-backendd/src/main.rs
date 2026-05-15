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
    /// Import operator records from a legacy Next.js admin-ui YAML
    /// config and its sidecar `operators.json` state file, writing the
    /// resulting collection into the admin-home store. Refuses if any
    /// operator records already exist.
    ImportOperatorsFromYaml {
        /// Path to the legacy admin-ui.yaml (with `operators: { name:
        /// { password_hash_file } }`).
        #[arg(long)]
        yaml: PathBuf,
        /// Optional path to the legacy operators.json state file
        /// (`{ version, operators: { name: { password_hash } } }`).
        #[arg(long)]
        state_json: Option<PathBuf>,
        /// Which store in the backend config receives the records.
        /// Defaults to the admin-home store resolved from config.
        #[arg(long)]
        store: Option<String>,
    },
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

    match cli.command.unwrap_or(Command::Run) {
        Command::Run => {
            info!(
                stores = config.stores.len(),
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
    let stores = open_stores_for_admin(&config.stores)?;
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
        AdminCommand::ImportOperatorsFromYaml {
            yaml,
            state_json,
            store,
        } => {
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
            let existing = operators::load(target.1.as_ref())?;
            anyhow::ensure!(
                existing.is_empty(),
                "store '{}' already has {} operator record(s); refusing to import",
                target.0,
                existing.len(),
            );
            let collection = import_operators_from_yaml_files(&yaml, state_json.as_deref())?;
            let count = collection.len();
            operators::save(target.1.as_ref(), &collection)?;
            info!(
                store = %target.0,
                count = count,
                "imported operators into admin-home store"
            );
            Ok(())
        }
    }
}

fn open_stores_for_admin(
    configs: &HashMap<String, underwrap_protocol::config::StorageConfig>,
) -> Result<HashMap<String, Arc<dyn underwrap_store::StorageBackend>>> {
    let mut out = HashMap::new();
    for (name, cfg) in configs {
        let store: Arc<dyn underwrap_store::StorageBackend> =
            Arc::from(underwrap_store::open(cfg)?);
        out.insert(name.clone(), store);
    }
    Ok(out)
}

#[derive(serde::Deserialize)]
struct LegacyYaml {
    #[serde(default)]
    operators: HashMap<String, LegacyOperator>,
}

#[derive(serde::Deserialize)]
struct LegacyOperator {
    password_hash_file: Option<PathBuf>,
}

#[derive(serde::Deserialize)]
struct LegacyStateJson {
    #[serde(default)]
    operators: HashMap<String, LegacyStateOperator>,
}

#[derive(serde::Deserialize)]
struct LegacyStateOperator {
    password_hash: Option<String>,
}

fn import_operators_from_yaml_files(
    yaml_path: &std::path::Path,
    state_json: Option<&std::path::Path>,
) -> Result<operators::OperatorCollection> {
    let yaml_text = std::fs::read_to_string(yaml_path)
        .with_context(|| format!("reading legacy admin-ui yaml at {}", yaml_path.display()))?;
    let parsed: LegacyYaml = serde_yaml::from_str(&yaml_text).context("parsing legacy yaml")?;

    let mut collection = operators::OperatorCollection::default();
    let now = operators::now_epoch_secs();

    // Config-declared operators come first (precedence-preserving over
    // any state-file entries with the same username).
    for (name, op) in parsed.operators {
        let hash_path = op
            .password_hash_file
            .with_context(|| format!("legacy operator '{name}' missing password_hash_file"))?;
        let resolved = if hash_path.is_absolute() {
            hash_path
        } else {
            yaml_path
                .parent()
                .map(|p| p.join(&hash_path))
                .unwrap_or(hash_path)
        };
        let hash = std::fs::read_to_string(&resolved)
            .with_context(|| {
                format!(
                    "reading password hash for operator '{name}' at {}",
                    resolved.display()
                )
            })?
            .trim()
            .to_string();
        collection.add(operators::OperatorRecord {
            id: operators::new_operator_id(),
            username: name,
            password_hash: hash,
            role: operators::OperatorRole::Admin,
            granted_stores: Vec::new(),
            created_at: now,
            password_changed_at: now,
        })?;
    }

    // Then state-file operators that don't collide with config ones.
    if let Some(path) = state_json {
        let raw = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(collection),
            Err(e) => {
                return Err(anyhow::Error::from(e).context(format!(
                    "reading legacy operators.json at {}",
                    path.display()
                )));
            }
        };
        let parsed: LegacyStateJson =
            serde_json::from_str(&raw).context("parsing legacy operators.json")?;
        for (name, op) in parsed.operators {
            let normalised = operators::normalise_username(&name);
            if collection.find_by_username(&normalised).is_some() {
                continue;
            }
            let hash = op
                .password_hash
                .with_context(|| format!("legacy state operator '{name}' missing password_hash"))?
                .trim()
                .to_string();
            collection.add(operators::OperatorRecord {
                id: operators::new_operator_id(),
                username: name,
                password_hash: hash,
                role: operators::OperatorRole::Admin,
                granted_stores: Vec::new(),
                created_at: now,
                password_changed_at: now,
            })?;
        }
    }

    Ok(collection)
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
    fn cli_parses_admin_import_subcommand() {
        let cli = Cli::parse_from([
            "underwrap-backendd",
            "admin",
            "import-operators-from-yaml",
            "--yaml",
            "/tmp/admin-ui.yaml",
            "--store",
            "robert",
        ]);
        match cli.command {
            Some(Command::Admin(AdminCommand::ImportOperatorsFromYaml {
                yaml,
                store,
                state_json,
            })) => {
                assert_eq!(yaml, PathBuf::from("/tmp/admin-ui.yaml"));
                assert_eq!(store.as_deref(), Some("robert"));
                assert!(state_json.is_none());
            }
            _ => panic!("expected admin import subcommand, got {:?}", cli.command),
        }
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

    #[test]
    fn import_helper_reads_yaml_and_state() {
        let dir = tempfile::TempDir::new().unwrap();
        let hash_path = dir.path().join("alice.hash");
        std::fs::write(
            &hash_path,
            "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n",
        )
        .unwrap();
        let yaml_path = dir.path().join("admin-ui.yaml");
        std::fs::write(
            &yaml_path,
            format!(
                "operators:\n  alice:\n    password_hash_file: {}\n",
                hash_path.display()
            ),
        )
        .unwrap();

        let state_path = dir.path().join("operators.json");
        std::fs::write(
            &state_path,
            r#"{"version":1,"operators":{"bob":{"password_hash":"$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy"}}}"#,
        )
        .unwrap();

        let coll = import_operators_from_yaml_files(&yaml_path, Some(&state_path)).unwrap();
        assert_eq!(coll.len(), 2);
        assert!(coll.find_by_username("alice").is_some());
        assert!(coll.find_by_username("bob").is_some());
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

    #[test]
    fn import_helper_state_does_not_clobber_yaml_operator() {
        let dir = tempfile::TempDir::new().unwrap();
        let hash_path = dir.path().join("alice.hash");
        let yaml_hash = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        std::fs::write(&hash_path, yaml_hash).unwrap();
        let yaml_path = dir.path().join("admin-ui.yaml");
        std::fs::write(
            &yaml_path,
            format!(
                "operators:\n  alice:\n    password_hash_file: {}\n",
                hash_path.display()
            ),
        )
        .unwrap();
        let state_path = dir.path().join("operators.json");
        std::fs::write(
            &state_path,
            r#"{"version":1,"operators":{"Alice":{"password_hash":"$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}}}"#,
        )
        .unwrap();

        let coll = import_operators_from_yaml_files(&yaml_path, Some(&state_path)).unwrap();
        assert_eq!(coll.len(), 1);
        let alice = coll.find_by_username("alice").unwrap();
        assert_eq!(alice.password_hash, yaml_hash);
    }
}
