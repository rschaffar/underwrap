//! underwrap-secretsd — D-Bus Secret Service provider for libsecret mode.
//!
//! Runs inside a PID namespace alongside the target process. Provides
//! `org.freedesktop.secrets` on a private D-Bus session bus so that
//! tools using libsecret (e.g., `gh`) can store and retrieve credentials.
//!
//! The whole libsecret keyring for this run is loaded once at startup
//! from a one-shot pipe (`--init-fd`) and kept in memory. Every D-Bus
//! mutation is persisted by atomically rewriting `--state-path` to the
//! current JCS-canonical bytes of the section; `underwrap-ns` reads
//! the state file after waitpid to compose the end-of-run blob.
//!
//! No socket to the daemon. The kernel SIGKILLs the namespace siblings
//! the moment PID 1 (the target) exits, so there is no opportunity for
//! a graceful teardown handshake — every mutation must already be on
//! disk by the time the D-Bus ack returns.

use anyhow::{Context, Result, bail};
use std::io::Read;
use std::os::unix::io::FromRawFd;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;

mod dbus_service;
mod section_store;
mod state;

use underwrap_protocol::resource_blob::LibsecretSection;

const WELL_KNOWN_NAME: &str = "org.freedesktop.secrets";
const BASE_PATH: &str = "/org/freedesktop/secrets";
const METADATA_TRACE_ENV: &str = "UNDERWRAP_DEBUG_SECRETSD_TRACE_METADATA";

pub(crate) fn metadata_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os(METADATA_TRACE_ENV).is_some())
}

#[derive(Clone, Debug)]
struct Args {
    bus_address: String,
    init_fd: i32,
    state_path: PathBuf,
    ready_fd: Option<i32>,
    /// Optional override for the persisted-state size cap. `None` keeps the
    /// built-in `state::DEFAULT_MAX_STATE_BYTES`.
    max_state_bytes: Option<u64>,
}

fn parse_args_from(args: &[String]) -> Result<Args> {
    let mut bus_address = None;
    let mut init_fd = None;
    let mut state_path = None;
    let mut ready_fd = None;
    let mut max_state_bytes = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--bus-address" => {
                i += 1;
                bus_address = Some(
                    args.get(i)
                        .context("--bus-address requires a value")?
                        .clone(),
                );
            }
            "--init-fd" => {
                i += 1;
                init_fd = Some(
                    args.get(i)
                        .context("--init-fd requires a value")?
                        .parse()
                        .context("--init-fd must be a number")?,
                );
            }
            "--state-path" => {
                i += 1;
                state_path = Some(PathBuf::from(
                    args.get(i)
                        .context("--state-path requires a value")?
                        .clone(),
                ));
            }
            "--ready-fd" => {
                i += 1;
                ready_fd = Some(
                    args.get(i)
                        .context("--ready-fd requires a value")?
                        .parse()
                        .context("--ready-fd must be a number")?,
                );
            }
            "--max-state-bytes" => {
                i += 1;
                max_state_bytes = Some(
                    args.get(i)
                        .context("--max-state-bytes requires a value")?
                        .parse()
                        .context("--max-state-bytes must be a number")?,
                );
            }
            other => bail!("unknown argument: {other}"),
        }
        i += 1;
    }
    Ok(Args {
        bus_address: bus_address.context("--bus-address is required")?,
        init_fd: init_fd.context("--init-fd is required")?,
        state_path: state_path.context("--state-path is required")?,
        ready_fd,
        max_state_bytes,
    })
}

fn parse_args() -> Result<Args> {
    let args: Vec<String> = std::env::args().collect();
    parse_args_from(&args)
}

fn read_init_section(init_fd: i32) -> Result<LibsecretSection> {
    let mut file = unsafe { std::fs::File::from_raw_fd(init_fd) };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .context("reading initial libsecret section from --init-fd")?;
    if bytes.is_empty() {
        return Ok(LibsecretSection::default());
    }
    LibsecretSection::from_canonical_bytes(&bytes)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // secretsd runs inside the wrapper's namespace and inherits the user's
    // terminal stderr, so default to `warn` to keep routine startup and
    // per-call DBus traces off the console. Set `RUST_LOG=underwrap_secretsd=info`
    // (or `debug`) to re-enable them when debugging.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();

    // Set non-dumpable BEFORE reading any secrets from the init pipe.
    // This blocks /proc/<pid>/fd/ access from same-UID processes.
    let ret = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) };
    if ret != 0 {
        bail!(
            "prctl(PR_SET_DUMPABLE, 0) failed: {}",
            std::io::Error::last_os_error()
        );
    }

    let args = parse_args()?;

    if let Some(max) = args.max_state_bytes {
        state::configure_max_state_bytes(max);
    }

    if metadata_trace_enabled() {
        tracing::info!(
            env_var = METADATA_TRACE_ENV,
            "Secret Service metadata tracing enabled"
        );
    }

    // Read the entire libsecret section from the one-shot init pipe,
    // then build the in-memory store. The pipe is closed implicitly
    // when `read_init_section`'s `File` drops.
    let section =
        read_init_section(args.init_fd).context("loading libsecret section from --init-fd")?;
    let store = Arc::new(RwLock::new(section_store::store_from_section(&section)));
    let state_path = Arc::new(args.state_path.clone());

    // Persist the initial state immediately so the ns parent observes a
    // valid file even if no mutations happen during the run. A torn
    // initial write would make the end-of-run read fail.
    state::dump(&*store.read().await, &state_path).context("persisting initial libsecret state")?;

    // Connect to the private D-Bus session bus.
    let connection = zbus::ConnectionBuilder::address(args.bus_address.as_str())?
        .build()
        .await
        .context("connecting to private D-Bus session bus")?;

    tracing::info!(
        "connected to D-Bus, unique name: {}",
        connection.unique_name().map(|n| n.as_str()).unwrap_or("?")
    );

    // Register the Secret Service on the bus.
    let service = dbus_service::SecretService::new(store.clone(), state_path.clone());

    connection
        .object_server()
        .at(BASE_PATH, service)
        .await
        .context("registering Secret Service")?;

    // Register collections, items, aliases.
    {
        let state = store.read().await;
        for col_id in state.list_collections() {
            let col = dbus_service::CollectionInterface::new(
                col_id.to_string(),
                store.clone(),
                state_path.clone(),
            );
            let path = format!("{BASE_PATH}/collection/{col_id}");
            match zbus::zvariant::ObjectPath::try_from(path.clone()) {
                Ok(path) => {
                    if let Err(e) = connection.object_server().at(&path, col).await {
                        tracing::warn!(%path, error = %e, "registering collection failed");
                    }
                }
                Err(e) => tracing::warn!(path, error = %e, "invalid collection object path"),
            }

            for item_id in state.list_items(col_id) {
                let item = dbus_service::ItemInterface::new(
                    item_id.to_string(),
                    col_id.to_string(),
                    store.clone(),
                    state_path.clone(),
                );
                let item_path = format!("{BASE_PATH}/collection/{col_id}/{item_id}");
                match zbus::zvariant::ObjectPath::try_from(item_path.clone()) {
                    Ok(path) => {
                        if let Err(e) = connection.object_server().at(&path, item).await {
                            tracing::warn!(%path, error = %e, "registering item failed");
                        }
                    }
                    Err(e) => tracing::warn!(item_path, error = %e, "invalid item object path"),
                }
            }
        }

        for (alias, col_id) in &state.aliases {
            let col = dbus_service::CollectionInterface::new(
                col_id.clone(),
                store.clone(),
                state_path.clone(),
            );
            let alias_path = format!("{BASE_PATH}/aliases/{alias}");
            match zbus::zvariant::ObjectPath::try_from(alias_path.clone()) {
                Ok(path) => {
                    if let Err(e) = connection.object_server().at(&path, col).await {
                        tracing::warn!(%path, error = %e, "registering alias failed");
                    }
                }
                Err(e) => tracing::warn!(alias_path, error = %e, "invalid alias object path"),
            }
        }
    }

    // Request the well-known name.
    connection
        .request_name(WELL_KNOWN_NAME)
        .await
        .context("requesting org.freedesktop.secrets")?;

    tracing::info!("acquired {WELL_KNOWN_NAME}, ready to serve");

    // Signal readiness to underwrap-ns.
    if let Some(ready_fd) = args.ready_fd {
        use std::io::Write;
        let mut ready_file = unsafe { std::fs::File::from_raw_fd(ready_fd) };
        if let Err(e) = ready_file.write_all(b"R") {
            eprintln!("underwrap-secretsd: failed to signal readiness: {e}");
            std::process::exit(1);
        }
    }

    // Run forever (until the namespace dies and we get SIGKILL).
    std::future::pending::<()>().await;

    Ok(())
}

#[cfg(test)]
mod tests;
