use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use tokio::net::UnixStream;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{BackendMgmtRequest, BackendMgmtResponse};
use underwrap_protocol::policy::{PolicyDocument, PolicyEffect, PolicyRule, WrapperPolicyState};

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
    ShowPolicy {
        store: String,
    },
    WrapperEnable {
        store: String,
        wrapper: String,
    },
    WrapperDisable {
        store: String,
        wrapper: String,
        #[arg(long)]
        reason: Option<String>,
    },
    RuleUpsert {
        store: String,
        id: String,
        wrapper: String,
        resource: String,
        operation: String,
        effect: String,
    },
    RuleDelete {
        store: String,
        id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::ListStores => {
            print_response(send_request(&cli.socket, &BackendMgmtRequest::ListStores).await?)?
        }
        Commands::ListExports => {
            print_response(send_request(&cli.socket, &BackendMgmtRequest::ListExports).await?)?
        }
        Commands::Export {
            store,
            socket,
            mode,
        } => print_response(
            send_request(
                &cli.socket,
                &BackendMgmtRequest::ExportSocket {
                    store: store.clone(),
                    socket: socket.display().to_string(),
                    mode: parse_mode(mode)?,
                },
            )
            .await?,
        )?,
        Commands::Unexport { socket } => print_response(
            send_request(
                &cli.socket,
                &BackendMgmtRequest::UnexportSocket {
                    socket: socket.display().to_string(),
                },
            )
            .await?,
        )?,
        Commands::ShowPolicy { store } => {
            let doc = fetch_policy_document(&cli.socket, store).await?;
            println!("{}", serde_json::to_string_pretty(&doc)?);
        }
        Commands::WrapperEnable { store, wrapper } => {
            let mut doc = fetch_policy_document(&cli.socket, store).await?;
            set_wrapper_state(&mut doc, wrapper, true, None);
            put_policy_document(&cli.socket, store, &doc).await?;
            println!("ok");
        }
        Commands::WrapperDisable {
            store,
            wrapper,
            reason,
        } => {
            let mut doc = fetch_policy_document(&cli.socket, store).await?;
            set_wrapper_state(&mut doc, wrapper, false, reason.clone());
            put_policy_document(&cli.socket, store, &doc).await?;
            println!("ok");
        }
        Commands::RuleUpsert {
            store,
            id,
            wrapper,
            resource,
            operation,
            effect,
        } => {
            let mut doc = fetch_policy_document(&cli.socket, store).await?;
            upsert_rule(
                &mut doc,
                PolicyRule {
                    id: id.clone(),
                    wrapper: wrapper.clone(),
                    resource: resource.clone(),
                    operation: operation.clone(),
                    effect: parse_effect(effect)?,
                },
            );
            put_policy_document(&cli.socket, store, &doc).await?;
            println!("ok");
        }
        Commands::RuleDelete { store, id } => {
            let mut doc = fetch_policy_document(&cli.socket, store).await?;
            doc.rules.retain(|rule| rule.id != *id);
            put_policy_document(&cli.socket, store, &doc).await?;
            println!("ok");
        }
    }

    Ok(())
}

fn print_response(response: BackendMgmtResponse) -> Result<()> {
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
        BackendMgmtResponse::GetPolicyOk { document_json } => {
            if let Some(document_json) = document_json {
                println!("{document_json}");
            } else {
                println!("{{\n  \"wrappers\": [],\n  \"rules\": []\n}}");
            }
        }
        BackendMgmtResponse::PutPolicyOk => println!("ok"),
        BackendMgmtResponse::Error { message, .. } => anyhow::bail!(message),
    }
    Ok(())
}

async fn fetch_policy_document(socket: &Path, store: &str) -> Result<PolicyDocument> {
    let response = send_request(
        socket,
        &BackendMgmtRequest::GetPolicy {
            store: store.to_string(),
        },
    )
    .await?;
    match response {
        BackendMgmtResponse::GetPolicyOk { document_json } => match document_json {
            Some(document_json) => Ok(serde_json::from_str(&document_json)?),
            None => Ok(PolicyDocument::default()),
        },
        BackendMgmtResponse::Error { message, .. } => anyhow::bail!(message),
        other => anyhow::bail!("unexpected response: {other:?}"),
    }
}

async fn put_policy_document(socket: &Path, store: &str, doc: &PolicyDocument) -> Result<()> {
    let response = send_request(
        socket,
        &BackendMgmtRequest::PutPolicy {
            store: store.to_string(),
            document_json: serde_json::to_string_pretty(doc)?,
        },
    )
    .await?;
    match response {
        BackendMgmtResponse::PutPolicyOk => Ok(()),
        BackendMgmtResponse::Error { message, .. } => anyhow::bail!(message),
        other => anyhow::bail!("unexpected response: {other:?}"),
    }
}

fn set_wrapper_state(
    doc: &mut PolicyDocument,
    wrapper: &str,
    enabled: bool,
    reason: Option<String>,
) {
    if let Some(state) = doc
        .wrappers
        .iter_mut()
        .find(|state| state.wrapper == wrapper)
    {
        state.enabled = enabled;
        state.reason = reason;
    } else {
        doc.wrappers.push(WrapperPolicyState {
            wrapper: wrapper.to_string(),
            enabled,
            reason,
        });
    }
    doc.wrappers.sort_by(|a, b| a.wrapper.cmp(&b.wrapper));
}

fn upsert_rule(doc: &mut PolicyDocument, rule: PolicyRule) {
    if let Some(existing) = doc.rules.iter_mut().find(|existing| existing.id == rule.id) {
        *existing = rule;
    } else {
        doc.rules.push(rule);
    }
    doc.rules.sort_by(|a, b| a.id.cmp(&b.id));
}

fn parse_effect(effect: &str) -> Result<PolicyEffect> {
    match effect {
        "allow" => Ok(PolicyEffect::Allow),
        "deny" => Ok(PolicyEffect::Deny),
        other => anyhow::bail!("invalid effect {other:?}, expected 'allow' or 'deny'"),
    }
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
    fn cli_parses_policy_commands() {
        let cli = Cli::parse_from(["underwrap-backend-ctl", "show-policy", "work"]);
        assert!(matches!(cli.command, Commands::ShowPolicy { ref store } if store == "work"));

        let cli = Cli::parse_from([
            "underwrap-backend-ctl",
            "wrapper-disable",
            "work",
            "gh-comment",
            "--reason",
            "maintenance",
        ]);
        match cli.command {
            Commands::WrapperDisable {
                store,
                wrapper,
                reason,
            } => {
                assert_eq!(store, "work");
                assert_eq!(wrapper, "gh-comment");
                assert_eq!(reason.as_deref(), Some("maintenance"));
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

    #[test]
    fn parse_effect_accepts_allow_and_deny() {
        assert_eq!(parse_effect("allow").unwrap(), PolicyEffect::Allow);
        assert_eq!(parse_effect("deny").unwrap(), PolicyEffect::Deny);
        assert!(parse_effect("maybe").is_err());
    }

    #[test]
    fn wrapper_state_and_rule_helpers_modify_document() {
        let mut doc = PolicyDocument::default();
        set_wrapper_state(&mut doc, "gh-comment", false, Some("revoked".to_string()));
        assert_eq!(doc.wrappers.len(), 1);
        assert!(!doc.wrappers[0].enabled);

        upsert_rule(
            &mut doc,
            PolicyRule {
                id: "r1".to_string(),
                wrapper: "gh-comment".to_string(),
                resource: "secret:github/token".to_string(),
                operation: "secret.read".to_string(),
                effect: PolicyEffect::Allow,
            },
        );
        assert_eq!(doc.rules.len(), 1);

        upsert_rule(
            &mut doc,
            PolicyRule {
                id: "r1".to_string(),
                wrapper: "gh-comment".to_string(),
                resource: "secret:github/token".to_string(),
                operation: "secret.read".to_string(),
                effect: PolicyEffect::Deny,
            },
        );
        assert_eq!(doc.rules.len(), 1);
        assert_eq!(doc.rules[0].effect, PolicyEffect::Deny);
    }
}
