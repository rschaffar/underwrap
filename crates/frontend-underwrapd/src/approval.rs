//! Per-call approval state and verification.
//!
//! When the backend policy puts a wrapper in `PerCallApproval` mode, every
//! single read must be accompanied by a fresh SSH-agent signature over a
//! daemon-issued nonce. The daemon keeps short-lived pending challenges
//! in memory so the verification side can match proof back to the nonce
//! that was issued.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use ssh_key::public::KeyData;

/// How long a single pending challenge is valid before it is evicted.
const CHALLENGE_TTL: Duration = Duration::from_secs(60);

/// Size of the random nonce presented to the caller.
const NONCE_LEN: usize = 32;

/// A challenge issued but not yet proved.
#[derive(Debug, Clone)]
struct PendingChallenge {
    wrapper: String,
    nonce: Vec<u8>,
    approval_key: String,
    expires_at: Instant,
}

/// Thread-safe store of pending per-call approval challenges.
pub struct PendingApprovals {
    by_id: Mutex<HashMap<String, PendingChallenge>>,
}

impl Default for PendingApprovals {
    fn default() -> Self {
        Self::new()
    }
}

/// A freshly-issued challenge, handed back to the wrapper over the wire.
pub struct IssuedChallenge {
    pub challenge_id: String,
    pub nonce: Vec<u8>,
    pub approval_key: String,
}

impl PendingApprovals {
    pub fn new() -> Self {
        Self {
            by_id: Mutex::new(HashMap::new()),
        }
    }

    /// Create a fresh challenge for `wrapper` bound to `approval_key`.
    pub fn issue(&self, wrapper: &str, approval_key: &str) -> IssuedChallenge {
        let nonce: [u8; NONCE_LEN] = rand::random();
        let challenge_id = gen_challenge_id();
        let mut by_id = self.by_id.lock().unwrap();
        gc(&mut by_id);
        by_id.insert(
            challenge_id.clone(),
            PendingChallenge {
                wrapper: wrapper.to_string(),
                nonce: nonce.to_vec(),
                approval_key: approval_key.to_string(),
                expires_at: Instant::now() + CHALLENGE_TTL,
            },
        );
        IssuedChallenge {
            challenge_id,
            nonce: nonce.to_vec(),
            approval_key: approval_key.to_string(),
        }
    }

    /// Look up and consume a pending challenge.
    ///
    /// Returns `Ok((wrapper, nonce, approval_key))` on hit. The challenge is
    /// removed regardless of whether the signature later verifies, so each
    /// nonce can only be used once.
    pub fn consume(&self, challenge_id: &str) -> Result<(String, Vec<u8>, String)> {
        let mut by_id = self.by_id.lock().unwrap();
        gc(&mut by_id);
        let entry = by_id
            .remove(challenge_id)
            .ok_or_else(|| anyhow!("unknown or expired approval challenge"))?;
        if Instant::now() >= entry.expires_at {
            return Err(anyhow!("approval challenge expired"));
        }
        Ok((entry.wrapper, entry.nonce, entry.approval_key))
    }
}

fn gc(map: &mut HashMap<String, PendingChallenge>) {
    let now = Instant::now();
    map.retain(|_, c| c.expires_at > now);
}

fn gen_challenge_id() -> String {
    let raw: [u8; 16] = rand::random();
    let mut s = String::with_capacity(2 * raw.len());
    for byte in raw {
        use std::fmt::Write;
        write!(&mut s, "{byte:02x}").expect("writing to String never fails");
    }
    s
}

/// Verify that `signature` is a valid SSH-agent signature made by `public_key`
/// over `nonce`, and that `public_key` matches the configured OpenSSH
/// `approval_key`.
pub fn verify_proof(
    approval_key: &str,
    nonce: &[u8],
    signature: &[u8],
    public_key_blob: &[u8],
) -> Result<()> {
    let expected = ssh_key::PublicKey::from_openssh(approval_key)
        .map_err(|e| anyhow!("invalid configured approval_key: {e}"))?;
    let caller = ssh_key::PublicKey::from_bytes(public_key_blob)
        .map_err(|e| anyhow!("invalid caller public key: {e}"))?;
    if expected.key_data() != caller.key_data() {
        return Err(anyhow!("approval key mismatch"));
    }
    let ssh_sig = ssh_key::Signature::try_from(signature)
        .map_err(|e| anyhow!("invalid SSH signature format: {e}"))?;
    use signature::Verifier;
    let ok = match caller.key_data() {
        KeyData::Ed25519(key) => key.verify(nonce, &ssh_sig).is_ok(),
        KeyData::Rsa(key) => key.verify(nonce, &ssh_sig).is_ok(),
        _ => {
            return Err(anyhow!(
                "unsupported approval key type (ed25519 or rsa only)"
            ));
        }
    };
    if !ok {
        return Err(anyhow!("approval signature failed verification"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Signer;
    use ssh_key::PrivateKey;
    use ssh_key::private::{Ed25519Keypair, KeypairData};

    fn test_key() -> (PrivateKey, ed25519_dalek::SigningKey, String, Vec<u8>) {
        let keypair = Ed25519Keypair::random(&mut rand::thread_rng());
        let private = PrivateKey::new(KeypairData::Ed25519(keypair.clone()), "t").unwrap();
        let openssh = private.public_key().to_openssh().unwrap();
        let pk_blob = private.public_key().to_bytes().unwrap();
        let signing = ed25519_dalek::SigningKey::from_bytes(&keypair.private.to_bytes());
        (private, signing, openssh, pk_blob)
    }

    fn wire_signature(algo: &[u8], sig_bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + algo.len() + 4 + sig_bytes.len());
        out.extend_from_slice(&(algo.len() as u32).to_be_bytes());
        out.extend_from_slice(algo);
        out.extend_from_slice(&(sig_bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(sig_bytes);
        out
    }

    #[test]
    fn pending_approvals_issue_and_consume() {
        let store = PendingApprovals::new();
        let issued = store.issue("gh-authorized", "ssh-ed25519 AAAA");
        assert_eq!(issued.nonce.len(), NONCE_LEN);
        let (wrapper, nonce, key) = store.consume(&issued.challenge_id).unwrap();
        assert_eq!(wrapper, "gh-authorized");
        assert_eq!(nonce, issued.nonce);
        assert_eq!(key, "ssh-ed25519 AAAA");
    }

    #[test]
    fn pending_approvals_nonce_is_single_use() {
        let store = PendingApprovals::new();
        let issued = store.issue("w", "k");
        assert!(store.consume(&issued.challenge_id).is_ok());
        assert!(store.consume(&issued.challenge_id).is_err());
    }

    #[test]
    fn verify_proof_accepts_ed25519_signature() {
        let (_priv, signing, openssh, pk_blob) = test_key();
        let nonce = b"test-nonce-bytes";
        let sig = signing.sign(nonce).to_bytes();
        let wire = wire_signature(b"ssh-ed25519", &sig);
        verify_proof(&openssh, nonce, &wire, &pk_blob).unwrap();
    }

    #[test]
    fn verify_proof_rejects_mismatched_pubkey() {
        let (_priv, signing, _openssh, _pk_blob) = test_key();
        let (_priv2, _signing2, openssh2, pk_blob2) = test_key();
        let nonce = b"nonce";
        let sig = signing.sign(nonce).to_bytes();
        let wire = wire_signature(b"ssh-ed25519", &sig);
        let err = verify_proof(&openssh2, nonce, &wire, &pk_blob2).unwrap_err();
        assert!(err.to_string().contains("verification"));
    }

    #[test]
    fn verify_proof_rejects_wrong_nonce() {
        let (_priv, signing, openssh, pk_blob) = test_key();
        let sig = signing.sign(b"original-nonce").to_bytes();
        let wire = wire_signature(b"ssh-ed25519", &sig);
        let err = verify_proof(&openssh, b"other-nonce", &wire, &pk_blob).unwrap_err();
        assert!(err.to_string().contains("verification"));
    }

    #[test]
    fn verify_proof_rejects_wrong_configured_key() {
        let (_priv_a, signing_a, _openssh_a, pk_blob_a) = test_key();
        let (_priv_b, _signing_b, openssh_b, _pk_blob_b) = test_key();
        let nonce = b"n";
        let sig = signing_a.sign(nonce).to_bytes();
        let wire = wire_signature(b"ssh-ed25519", &sig);
        let err = verify_proof(&openssh_b, nonce, &wire, &pk_blob_a).unwrap_err();
        assert!(err.to_string().contains("mismatch"));
    }
}
