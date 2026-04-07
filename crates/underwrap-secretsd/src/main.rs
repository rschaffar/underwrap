//! underwrap-secretsd — D-Bus Secret Service provider for libsecret mode.
//!
//! Runs inside a PID namespace alongside the target process. Provides
//! `org.freedesktop.secrets` on a private D-Bus session bus so that
//! tools using libsecret (e.g., `gh`) can store and retrieve credentials.
//!
//! All secret storage goes through the daemon via an authenticated
//! socketpair channel (fd 4), NOT to local files.

use anyhow::{Context, Result, bail};
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::sync::Arc;
use tokio::sync::RwLock;

mod channel;
mod dbus_service;

const WELL_KNOWN_NAME: &str = "org.freedesktop.secrets";
const BASE_PATH: &str = "/org/freedesktop/secrets";

#[derive(Clone)]
struct Args {
    bus_address: String,
    channel_fd: i32,
    ready_fd: Option<i32>,
}

fn parse_args() -> Result<Args> {
    let args: Vec<String> = std::env::args().collect();
    let mut bus_address = None;
    let mut channel_fd = None;
    let mut ready_fd = None;
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
            "--channel-fd" => {
                i += 1;
                channel_fd = Some(
                    args.get(i)
                        .context("--channel-fd requires a value")?
                        .parse()
                        .context("--channel-fd must be a number")?,
                );
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
            other => bail!("unknown argument: {other}"),
        }
        i += 1;
    }
    Ok(Args {
        bus_address: bus_address.context("--bus-address is required")?,
        channel_fd: channel_fd.context("--channel-fd is required")?,
        ready_fd,
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Set non-dumpable BEFORE reading any secrets from the channel.
    // This blocks /proc/<pid>/fd/ access from same-UID processes.
    unsafe {
        libc::prctl(libc::PR_SET_DUMPABLE, 0);
    }

    let args = parse_args()?;

    // Open the channel to the daemon.
    let channel_stream = unsafe { StdUnixStream::from_raw_fd(args.channel_fd) };
    let channel = Arc::new(channel::DaemonChannel::new(channel_stream));

    // Load initial state from the daemon (read _aliases and collections).
    let store = Arc::new(RwLock::new(
        channel::load_initial_state(&channel).context("loading initial state from daemon")?,
    ));

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
    let service = dbus_service::SecretService::new(store.clone(), channel.clone());

    connection
        .object_server()
        .at(BASE_PATH, service)
        .await
        .context("registering Secret Service")?;

    // Register the default collection.
    {
        let state = store.read().await;
        for col_id in state.list_collections() {
            let col = dbus_service::CollectionInterface::new(
                col_id.to_string(),
                store.clone(),
                channel.clone(),
            );
            let path = format!("{BASE_PATH}/collection/{col_id}");
            if let Ok(path) = zbus::zvariant::ObjectPath::try_from(path.clone()) {
                let _ = connection.object_server().at(path, col).await;
            }

            // Register items.
            for item_id in state.list_items(col_id) {
                let item = dbus_service::ItemInterface::new(
                    item_id.to_string(),
                    col_id.to_string(),
                    store.clone(),
                    channel.clone(),
                );
                let item_path = format!("{BASE_PATH}/collection/{col_id}/{item_id}");
                if let Ok(path) = zbus::zvariant::ObjectPath::try_from(item_path) {
                    let _ = connection.object_server().at(path, item).await;
                }
            }
        }

        // Register aliases.
        for (alias, col_id) in &state.aliases {
            let col = dbus_service::CollectionInterface::new(
                col_id.clone(),
                store.clone(),
                channel.clone(),
            );
            let alias_path = format!("{BASE_PATH}/aliases/{alias}");
            if let Ok(path) = zbus::zvariant::ObjectPath::try_from(alias_path) {
                let _ = connection.object_server().at(path, col).await;
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
        unsafe {
            libc::write(ready_fd, b"R".as_ptr() as *const libc::c_void, 1);
            libc::close(ready_fd);
        }
    }

    // Run forever (until the namespace dies and we get SIGKILL).
    std::future::pending::<()>().await;

    Ok(())
}
