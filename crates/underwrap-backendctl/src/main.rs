use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use tokio::net::UnixStream;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{BackendMgmtRequest, BackendMgmtResponse};

#[derive(Parser, Debug)]
#[command(
    name = "underwrap-backend-ctl",
    about = "underwrap backend management tool"
)]
struct Cli {
    #[arg(long, default_value = "/run/underwrap-backend/mgmt.sock")]
    socket: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    ListStores,
    ListExports,
    Export {
        store: String,
        socket: PathBuf,
        #[arg(long, default_value = "0660")]
        mode: String,
    },
    Unexport {
        socket: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let request = match &cli.command {
        Commands::ListStores => BackendMgmtRequest::ListStores,
        Commands::ListExports => BackendMgmtRequest::ListExports,
        Commands::Export {
            store,
            socket,
            mode,
        } => BackendMgmtRequest::ExportSocket {
            store: store.clone(),
            socket: socket.display().to_string(),
            mode: parse_mode(mode)?,
        },
        Commands::Unexport { socket } => BackendMgmtRequest::UnexportSocket {
            socket: socket.display().to_string(),
        },
    };

    let response = send_request(&cli.socket, &request).await?;
    match response {
        BackendMgmtResponse::ListStoresOk { stores } => {
            for store in stores {
                println!("{store}");
            }
        }
        BackendMgmtResponse::ListExportsOk { exports } => {
            for export in exports {
                println!("{} {} {:#o}", export.store, export.socket, export.mode);
            }
        }
        BackendMgmtResponse::ExportSocketOk => println!("ok"),
        BackendMgmtResponse::UnexportSocketOk { existed } => println!("{existed}"),
        BackendMgmtResponse::Error { message, .. } => anyhow::bail!(message),
    }

    Ok(())
}

fn parse_mode(mode: &str) -> Result<u32> {
    let trimmed = mode
        .strip_prefix("0o")
        .or_else(|| mode.strip_prefix('0'))
        .unwrap_or(mode);
    u32::from_str_radix(trimmed, 8).with_context(|| format!("invalid mode {mode:?}"))
}

async fn send_request(socket: &Path, request: &BackendMgmtRequest) -> Result<BackendMgmtResponse> {
    let mut stream = UnixStream::connect(socket)
        .await
        .with_context(|| format!("connecting to {}", socket.display()))?;
    write_frame_async(&mut stream, request).await?;
    let response: BackendMgmtResponse = read_frame_async(&mut stream).await?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_parses_export_command() {
        let cli = Cli::parse_from([
            "underwrap-backend-ctl",
            "--socket",
            "/run/mgmt.sock",
            "export",
            "work",
            "/run/work.sock",
            "--mode",
            "0660",
        ]);
        assert_eq!(cli.socket, PathBuf::from("/run/mgmt.sock"));
        match cli.command {
            Commands::Export {
                store,
                socket,
                mode,
            } => {
                assert_eq!(store, "work");
                assert_eq!(socket, PathBuf::from("/run/work.sock"));
                assert_eq!(mode, "0660");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_mode_accepts_octal_strings() {
        assert_eq!(parse_mode("0660").unwrap(), 0o660);
        assert_eq!(parse_mode("0o600").unwrap(), 0o600);
        assert!(parse_mode("not-a-mode").is_err());
    }
}
