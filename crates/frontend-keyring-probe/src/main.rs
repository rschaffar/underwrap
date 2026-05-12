use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Type, Value};
use zbus::{Connection, Proxy};

const SERVICE_NAME: &str = "org.freedesktop.secrets";
const SERVICE_PATH: &str = "/org/freedesktop/secrets";
const SERVICE_IFACE: &str = "org.freedesktop.Secret.Service";
const COLLECTION_IFACE: &str = "org.freedesktop.Secret.Collection";
const ITEM_IFACE: &str = "org.freedesktop.Secret.Item";
const DEFAULT_ALIAS_PATH: &str = "/org/freedesktop/secrets/aliases/default";

#[derive(Parser, Debug)]
#[command(
    name = "underwrap-keyring-probe",
    about = "Probe Secret Service compatibility using go-keyring-style operations"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Store a secret using the same collection/attribute pattern as go-keyring.
    Set(SecretCommand),
    /// Retrieve a secret using the same search pattern as go-keyring.
    Get(GetCommand),
    /// Write then immediately read back the user-scoped slot.
    Roundtrip(SecretCommand),
    /// Reproduce gh's secure-login activation shape: user slot then active slot.
    GhLoginSequence(SecretCommand),
}

#[derive(Args, Debug)]
struct SecretCommand {
    #[arg(long)]
    service: String,
    #[arg(long)]
    user: String,
    #[arg(long)]
    secret: Option<String>,
    #[arg(long)]
    secret_stdin: bool,
}

#[derive(Args, Debug)]
struct GetCommand {
    #[arg(long)]
    service: String,
    #[arg(long)]
    user: String,
}

#[derive(Debug, Serialize, Deserialize, Type)]
struct Secret {
    session: OwnedObjectPath,
    parameters: Vec<u8>,
    value: Vec<u8>,
    content_type: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("underwrap-keyring-probe: {err:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let conn = Connection::session()
        .await
        .context("connecting to session bus")?;
    match cli.command {
        Command::Set(cmd) => {
            let secret = cmd.resolve_secret()?;
            set_secret(&conn, &cmd.service, &cmd.user, &secret).await?;
        }
        Command::Get(cmd) => {
            let secret = get_secret(&conn, &cmd.service, &cmd.user).await?;
            println!("{secret}");
        }
        Command::Roundtrip(cmd) => {
            let expected = cmd.resolve_secret()?;
            set_secret(&conn, &cmd.service, &cmd.user, &expected).await?;
            let actual = get_secret(&conn, &cmd.service, &cmd.user).await?;
            ensure_secret_match(&expected, &actual, "user slot")?;
            println!("user-slot=ok");
        }
        Command::GhLoginSequence(cmd) => {
            let expected = cmd.resolve_secret()?;
            set_secret(&conn, &cmd.service, &cmd.user, &expected).await?;
            let user_secret = get_secret(&conn, &cmd.service, &cmd.user).await?;
            ensure_secret_match(&expected, &user_secret, "user slot")?;
            set_secret(&conn, &cmd.service, "", &user_secret).await?;
            let active_secret = get_secret(&conn, &cmd.service, "").await?;
            ensure_secret_match(&expected, &active_secret, "active slot")?;
            println!("user-slot=ok");
            println!("active-slot=ok");
        }
    }
    Ok(())
}

impl SecretCommand {
    fn resolve_secret(&self) -> Result<String> {
        match (&self.secret, self.secret_stdin) {
            (Some(_), true) => bail!("specify only one of --secret or --secret-stdin"),
            (None, false) => bail!("one of --secret or --secret-stdin is required"),
            (Some(secret), false) => Ok(secret.clone()),
            (None, true) => read_secret_from_stdin(),
        }
    }
}

fn read_secret_from_stdin() -> Result<String> {
    let mut secret = String::new();
    std::io::stdin()
        .read_to_string(&mut secret)
        .context("reading secret from stdin")?;
    if let Some(stripped) = secret.strip_suffix('\n') {
        secret = stripped.to_string();
        if let Some(stripped) = secret.strip_suffix('\r') {
            secret = stripped.to_string();
        }
    }
    if secret.is_empty() {
        bail!("secret from stdin is empty")
    }
    Ok(secret)
}

async fn set_secret(conn: &Connection, service: &str, user: &str, secret: &str) -> Result<()> {
    let session = open_session(conn).await?;
    let collection_path = OwnedObjectPath::try_from(DEFAULT_ALIAS_PATH.to_string())
        .expect("default alias path is valid");
    unlock(conn, vec![collection_path.clone()]).await?;

    let label = format!("Password for '{user}' on '{service}'");
    let mut properties = HashMap::<String, Value<'_>>::new();
    properties.insert(
        "org.freedesktop.Secret.Item.Label".to_string(),
        Value::from(label.as_str()),
    );
    properties.insert(
        "org.freedesktop.Secret.Item.Attributes".to_string(),
        Value::from(attributes(service, user)),
    );

    let collection = Proxy::new(
        conn,
        SERVICE_NAME,
        collection_path.clone(),
        COLLECTION_IFACE,
    )
    .await
    .context("creating collection proxy")?;
    let _: (OwnedObjectPath, OwnedObjectPath) = collection
        .call(
            "CreateItem",
            &(
                properties,
                Secret {
                    session,
                    parameters: Vec::new(),
                    value: secret.as_bytes().to_vec(),
                    content_type: "text/plain; charset=utf8".to_string(),
                },
                true,
            ),
        )
        .await
        .context("CreateItem")?;
    Ok(())
}

async fn get_secret(conn: &Connection, service: &str, user: &str) -> Result<String> {
    let collection_path = OwnedObjectPath::try_from(DEFAULT_ALIAS_PATH.to_string())
        .expect("default alias path is valid");
    unlock(conn, vec![collection_path.clone()]).await?;

    let collection = Proxy::new(conn, SERVICE_NAME, collection_path, COLLECTION_IFACE)
        .await
        .context("creating collection proxy")?;
    let matches: Vec<OwnedObjectPath> = collection
        .call("SearchItems", &(attributes(service, user)))
        .await
        .context("SearchItems")?;
    let item = matches
        .into_iter()
        .next()
        .with_context(|| format!("no secret found for service={service:?} user={user:?}"))?;

    let session = open_session(conn).await?;
    unlock(conn, vec![item.clone()]).await?;

    let item_proxy = Proxy::new(conn, SERVICE_NAME, item, ITEM_IFACE)
        .await
        .context("creating item proxy")?;
    let secret: Secret = item_proxy
        .call("GetSecret", &(session))
        .await
        .context("GetSecret")?;
    String::from_utf8(secret.value).context("secret was not valid UTF-8")
}

async fn open_session(conn: &Connection) -> Result<OwnedObjectPath> {
    let service = Proxy::new(conn, SERVICE_NAME, SERVICE_PATH, SERVICE_IFACE)
        .await
        .context("creating service proxy")?;
    let (_, session): (OwnedValue, OwnedObjectPath) = service
        .call("OpenSession", &("plain", Value::from("")))
        .await
        .context("OpenSession")?;
    Ok(session)
}

async fn unlock(conn: &Connection, objects: Vec<OwnedObjectPath>) -> Result<()> {
    let service = Proxy::new(conn, SERVICE_NAME, SERVICE_PATH, SERVICE_IFACE)
        .await
        .context("creating service proxy")?;
    let (_unlocked, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) =
        service.call("Unlock", &(objects)).await.context("Unlock")?;
    if prompt.as_str() != "/" {
        bail!("unexpected prompt path from Unlock: {}", prompt.as_str());
    }
    Ok(())
}

fn attributes(service: &str, user: &str) -> HashMap<String, String> {
    HashMap::from([
        ("service".to_string(), service.to_string()),
        ("username".to_string(), user.to_string()),
    ])
}

fn ensure_secret_match(expected: &str, actual: &str, context: &str) -> Result<()> {
    if expected != actual {
        bail!("{context} mismatch: expected {expected:?}, got {actual:?}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_secret_accepts_explicit_secret() {
        let cmd = SecretCommand {
            service: "gh:github.com".to_string(),
            user: "alice".to_string(),
            secret: Some("topsecret".to_string()),
            secret_stdin: false,
        };
        assert_eq!(cmd.resolve_secret().unwrap(), "topsecret");
    }

    #[test]
    fn resolve_secret_rejects_missing_or_conflicting_sources() {
        let missing = SecretCommand {
            service: "svc".to_string(),
            user: "user".to_string(),
            secret: None,
            secret_stdin: false,
        };
        assert!(missing.resolve_secret().is_err());

        let conflicting = SecretCommand {
            service: "svc".to_string(),
            user: "user".to_string(),
            secret: Some("x".to_string()),
            secret_stdin: true,
        };
        assert!(conflicting.resolve_secret().is_err());
    }

    #[test]
    fn attributes_match_go_keyring_shape() {
        let attrs = attributes("gh:github.com", "rschaffar");
        assert_eq!(attrs.get("service"), Some(&"gh:github.com".to_string()));
        assert_eq!(attrs.get("username"), Some(&"rschaffar".to_string()));
    }
}
