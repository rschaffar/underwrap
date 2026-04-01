//! Credential derivation — transforms stored secrets into derived forms.
//!
//! Phase 4: OAuth refresh → access token derivation.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Instant;
use tracing::{info, warn};

use underwrap_protocol::config::DerivationMode;

/// Cached derived credential.
struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

/// Token cache: maps secret_id → cached access token.
pub struct TokenCache {
    cache: Mutex<std::collections::HashMap<String, CachedToken>>,
}

impl TokenCache {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(std::collections::HashMap::new()),
        }
    }

    fn get(&self, key: &str) -> Option<String> {
        let cache = self.cache.lock().unwrap();
        if let Some(entry) = cache.get(key) {
            if Instant::now() < entry.expires_at {
                return Some(entry.access_token.clone());
            }
        }
        None
    }

    fn put(&self, key: &str, token: &str, expires_in_secs: u64) {
        let mut cache = self.cache.lock().unwrap();
        cache.insert(
            key.to_string(),
            CachedToken {
                access_token: token.to_string(),
                // Expire 60 seconds early to avoid edge cases.
                expires_at: Instant::now()
                    + std::time::Duration::from_secs(expires_in_secs.saturating_sub(60)),
            },
        );
    }
}

impl Default for TokenCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Apply a derivation to stored secret material.
///
/// Returns the derived material (e.g. an access token instead of the raw
/// refresh token JSON).
pub async fn derive(
    mode: &DerivationMode,
    secret_id: &str,
    material: &str,
    cache: &TokenCache,
) -> Result<String> {
    match mode {
        DerivationMode::None => Ok(material.to_string()),
        DerivationMode::OAuthRefreshToAccessToken => {
            // Check cache first.
            if let Some(cached) = cache.get(secret_id) {
                info!(secret = %secret_id, "using cached access token");
                return Ok(cached);
            }

            // Parse the stored authorized-user JSON.
            let creds: AuthorizedUserCredentials =
                serde_json::from_str(material).context("parsing OAuth credentials JSON")?;

            // Exchange refresh token for access token.
            let token_response = exchange_refresh_token(&creds).await?;

            // Cache the result.
            cache.put(
                secret_id,
                &token_response.access_token,
                token_response.expires_in,
            );

            info!(secret = %secret_id, expires_in = token_response.expires_in, "derived access token");
            Ok(token_response.access_token)
        }
    }
}

/// Google authorized-user credentials JSON format.
#[derive(Debug, Deserialize, Serialize)]
struct AuthorizedUserCredentials {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    #[serde(default = "default_token_uri")]
    token_uri: String,
    #[serde(rename = "type", default)]
    credential_type: String,
}

fn default_token_uri() -> String {
    "https://oauth2.googleapis.com/token".to_string()
}

/// Token exchange response.
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

/// Perform the OAuth2 token exchange via HTTP POST.
async fn exchange_refresh_token(creds: &AuthorizedUserCredentials) -> Result<TokenResponse> {
    let client = reqwest::Client::new();

    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", &creds.client_id),
        ("client_secret", &creds.client_secret),
        ("refresh_token", &creds.refresh_token),
    ];

    let response = client
        .post(&creds.token_uri)
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
        warn!(status = %status, "token exchange failed");
        bail!("token exchange failed (HTTP {status}): {body}");
    }

    let token: TokenResponse = response
        .json()
        .await
        .context("parsing token exchange response")?;

    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_authorized_user_json() {
        let json = r#"{
            "type": "authorized_user",
            "client_id": "123.apps.googleusercontent.com",
            "client_secret": "secret",
            "refresh_token": "1//refresh"
        }"#;
        let creds: AuthorizedUserCredentials = serde_json::from_str(json).unwrap();
        assert_eq!(creds.client_id, "123.apps.googleusercontent.com");
        assert_eq!(creds.token_uri, "https://oauth2.googleapis.com/token");
    }

    #[test]
    fn token_cache_miss_then_hit() {
        let cache = TokenCache::new();
        assert!(cache.get("k").is_none());
        cache.put("k", "tok", 3600);
        assert_eq!(cache.get("k").unwrap(), "tok");
    }

    #[test]
    fn token_cache_expired() {
        let cache = TokenCache::new();
        // expires_in=60 means after subtracting 60 margin, it's already expired.
        cache.put("k", "tok", 60);
        assert!(cache.get("k").is_none());
    }

    #[tokio::test]
    async fn derive_none_passthrough() {
        let cache = TokenCache::new();
        let result = derive(&DerivationMode::None, "test", "raw_material", &cache)
            .await
            .unwrap();
        assert_eq!(result, "raw_material");
    }
}
