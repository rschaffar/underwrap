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

fn build_request(command: &Commands, store_material: Option<String>) -> Result<ControlRequest> {
    Ok(match command {
        Commands::Status { target } => match target {
            Some(target) => ControlRequest::StatusTarget {
                target: target.clone(),
            },
            None => ControlRequest::StatusAll,
        },
        Commands::Store { secret_id } => ControlRequest::Store {
            secret_id: secret_id.clone(),
            material: store_material.context("store material missing")?.into(),
        },
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
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let store_material = match &cli.command {
        Commands::Store { .. } => Some(read_stdin().await?),
        _ => None,
    };
    let request = build_request(&cli.command, store_material)?;

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

fn trim_stdin_trailing_newline(buf: &mut String) {
    if buf.ends_with('\n') {
        buf.pop();
        if buf.ends_with('\r') {
            buf.pop();
        }
    }
}

async fn read_stdin() -> Result<String> {
    use tokio::io::AsyncReadExt;
    let mut stdin = tokio::io::stdin();
    let mut buf = String::new();
    stdin.read_to_string(&mut buf).await?;
    trim_stdin_trailing_newline(&mut buf);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_status_variants() {
        assert_eq!(
            build_request(&Commands::Status { target: None }, None).unwrap(),
            ControlRequest::StatusAll
        );
        assert_eq!(
            build_request(
                &Commands::Status {
                    target: Some("gh-authorized".to_string())
                },
                None,
            )
            .unwrap(),
            ControlRequest::StatusTarget {
                target: "gh-authorized".to_string()
            }
        );
    }

    #[test]
    fn build_request_store_requires_material() {
        let err = build_request(
            &Commands::Store {
                secret_id: "github/token".to_string(),
            },
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("store material missing"));

        assert_eq!(
            build_request(
                &Commands::Store {
                    secret_id: "github/token".to_string(),
                },
                Some("ghp_test".to_string()),
            )
            .unwrap(),
            ControlRequest::Store {
                secret_id: "github/token".to_string(),
                material: "ghp_test".into(),
            }
        );
    }

    #[test]
    fn build_request_unlock_and_lock() {
        assert_eq!(
            build_request(
                &Commands::Unlock {
                    target: "locked-w".to_string(),
                    seconds: Some(60),
                    scope: Some("session-1".to_string()),
                    grant_user: Some("alice".to_string()),
                    grant_all: false,
                },
                None,
            )
            .unwrap(),
            ControlRequest::Unlock {
                target: "locked-w".to_string(),
                scope: Some("session-1".to_string()),
                seconds: Some(60),
                grant_user: Some("alice".to_string()),
                grant_all: false,
            }
        );

        assert_eq!(
            build_request(
                &Commands::Lock {
                    target: Some("locked-w".to_string()),
                    scope: Some("session-1".to_string()),
                },
                None,
            )
            .unwrap(),
            ControlRequest::Lock {
                target: Some("locked-w".to_string()),
                scope: Some("session-1".to_string()),
            }
        );
    }

    #[test]
    fn trim_stdin_trailing_newline_removes_single_line_ending_only() {
        let mut unix = "secret\n".to_string();
        trim_stdin_trailing_newline(&mut unix);
        assert_eq!(unix, "secret");

        let mut windows = "secret\r\n".to_string();
        trim_stdin_trailing_newline(&mut windows);
        assert_eq!(windows, "secret");

        let mut untouched = "secret\nnext".to_string();
        trim_stdin_trailing_newline(&mut untouched);
        assert_eq!(untouched, "secret\nnext");
    }
}
