//! Credential derivation for the backend daemon.
//!
//! The backend holds secret material and performs derivations (e.g. OAuth
//! refresh → access token) on behalf of the frontend.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Apply a derivation to stored secret material.
pub async fn derive(mode: &str, secret_id: &str, material: &[u8]) -> Result<String> {
    match mode {
        "none" => Ok(std::str::from_utf8(material)
            .context("stored secret is not valid UTF-8")?
            .to_string()),
        "oauth_refresh_to_access_token" => {
            let creds: AuthorizedUserCredentials =
                serde_json::from_slice(material).context("parsing OAuth credentials JSON")?;
            let token_response = exchange_refresh_token(&creds).await?;
            info!(secret = %secret_id, expires_in = token_response.expires_in, "derived access token");
            Ok(token_response.access_token)
        }
        other => bail!("unknown derivation mode: {other}"),
    }
}

const GOOGLE_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

#[derive(Debug, Deserialize, Serialize)]
struct AuthorizedUserCredentials {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    #[serde(default)]
    #[allow(dead_code)]
    token_uri: Option<String>,
    #[serde(rename = "type", default)]
    #[allow(dead_code)]
    credential_type: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default = "default_expires_in")]
    expires_in: u64,
    #[allow(dead_code)]
    token_type: Option<String>,
}

fn default_expires_in() -> u64 {
    3600
}

async fn exchange_refresh_token(creds: &AuthorizedUserCredentials) -> Result<TokenResponse> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("building HTTP client")?;

    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", &creds.client_id),
        ("client_secret", &creds.client_secret),
        ("refresh_token", &creds.refresh_token),
    ];

    let response = client
        .post(GOOGLE_TOKEN_ENDPOINT)
        .form(&params)
        .send()
        .await
        .context("sending token exchange request")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unreadable>".to_string());
        let safe_body: String = body.chars().take(200).collect();
        warn!(status = %status, "token exchange failed");
        bail!("token exchange failed (HTTP {status}): {safe_body}");
    }

    response
        .json()
        .await
        .context("parsing token exchange response")
}
