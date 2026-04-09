use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tokio::net::UnixStream;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{ControlRequest, ControlResponse};

#[derive(Parser, Debug)]
#[command(name = "underwrapctl", about = "underwrap control tool")]
struct Cli {
    /// Path to the control socket.
    #[arg(long, default_value = underwrap_protocol::CONTROL_SOCKET_PATH)]
    socket: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show daemon / wrapper status.
    Status {
        /// Optional wrapper or secret name.
        target: Option<String>,
    },
    /// Store a secret (reads material from stdin as UTF-8 text).
    Store {
        /// Secret id, e.g. "github/token".
        secret_id: String,
    },
    /// Unlock a wrapper for a duration.
    Unlock {
        /// Wrapper name.
        target: String,
        /// Duration in seconds. Omit for indefinite.
        #[arg(long)]
        seconds: Option<u64>,
        /// Scope identifier (e.g. shell session).
        #[arg(long)]
        scope: Option<String>,
        /// Root/admin only: grant unlock to a specific allowed username.
        #[arg(long = "for-user")]
        grant_user: Option<String>,
        /// Root/admin only: grant unlock to all allowed users of the instance.
        #[arg(long = "for-all")]
        grant_all: bool,
    },
    /// Lock a wrapper (or everything).
    Lock {
        /// Wrapper name. Omit to lock everything.
        target: Option<String>,
        /// Scope identifier. Omit to lock all scopes.
        #[arg(long)]
        scope: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let request = match &cli.command {
        Commands::Status { target } => match target {
            Some(target) => ControlRequest::StatusTarget {
                target: target.clone(),
            },
            None => ControlRequest::StatusAll,
        },
        Commands::Store { secret_id } => {
            let material = read_stdin().await?;
            ControlRequest::Store {
                secret_id: secret_id.clone(),
                material: material.into(),
            }
        }
        Commands::Unlock {
            target,
            seconds,
            scope,
            grant_user,
            grant_all,
        } => ControlRequest::Unlock {
            target: target.clone(),
            scope: scope.clone(),
            seconds: *seconds,
            grant_user: grant_user.clone(),
            grant_all: *grant_all,
        },
        Commands::Lock { target, scope } => ControlRequest::Lock {
            target: target.clone(),
            scope: scope.clone(),
        },
    };

    let response: ControlResponse = send_control(&cli.socket, &request).await?;

    match response {
        ControlResponse::StatusAllOk {
            wrappers,
            credentials,
        } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "wrappers": wrappers,
                    "credentials": credentials,
                }))?
            );
        }
        ControlResponse::StatusWrapperOk { status } => {
            print_json(&status)?;
        }
        ControlResponse::StatusCredentialOk { status } => {
            print_json(&status)?;
        }
        ControlResponse::StoreOk | ControlResponse::UnlockOk | ControlResponse::LockOk => {
            println!("ok");
        }
        ControlResponse::Error { code, message } => {
            eprintln!("error ({code:?}): {message}");
            std::process::exit(1);
        }
    }

    Ok(())
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

async fn send_control(socket_path: &PathBuf, request: &ControlRequest) -> Result<ControlResponse> {
    let mut stream = UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("connecting to {}", socket_path.display()))?;

    write_frame_async(&mut stream, request).await?;
    let response: ControlResponse = read_frame_async(&mut stream).await?;
    Ok(response)
}

async fn read_stdin() -> Result<String> {
    use tokio::io::AsyncReadExt;
    let mut stdin = tokio::io::stdin();
    let mut buf = String::new();
    stdin.read_to_string(&mut buf).await?;
    if buf.ends_with('\n') {
        buf.pop();
        if buf.ends_with('\r') {
            buf.pop();
        }
    }
    Ok(buf)
}
