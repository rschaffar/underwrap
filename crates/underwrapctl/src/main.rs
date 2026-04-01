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
    /// Store a secret (reads material from stdin).
    Store {
        /// Secret id, e.g. "github/token".
        secret_id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let request = match &cli.command {
        Commands::Status { target } => ControlRequest::Status {
            target: target.clone(),
        },
        Commands::Store { secret_id } => {
            let material = read_stdin().await?;
            ControlRequest::Store {
                secret_id: secret_id.clone(),
                material,
            }
        }
    };

    let response: ControlResponse = send_control(&cli.socket, &request).await?;

    match response {
        ControlResponse::Ok { info } => {
            if let Some(info) = info {
                println!("{}", serde_json::to_string_pretty(&info)?);
            } else {
                println!("ok");
            }
        }
        ControlResponse::Error { code, message } => {
            eprintln!("error ({code:?}): {message}");
            std::process::exit(1);
        }
    }

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
    // Trim a single trailing newline (common from echo/heredoc), but
    // preserve all other content including embedded newlines.
    if buf.ends_with('\n') {
        buf.pop();
        if buf.ends_with('\r') {
            buf.pop();
        }
    }
    Ok(buf)
}
