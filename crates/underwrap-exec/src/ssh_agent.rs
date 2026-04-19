//! Minimal SSH agent protocol client used to sign per-call approval nonces.
//!
//! Wire format reference: RFC 4251 §5 ("string" encoding) and the `ssh-agent`
//! protocol numbers in OpenSSH's `authfd.h`.

use anyhow::{Context, Result, bail};
use std::io::Write;
use std::os::unix::net::UnixStream;

const SSH_AGENTC_REQUEST_IDENTITIES: u8 = 11;
const SSH_AGENT_IDENTITIES_ANSWER: u8 = 12;
const SSH_AGENTC_SIGN_REQUEST: u8 = 13;
const SSH_AGENT_SIGN_RESPONSE: u8 = 14;
const MAX_SSH_AGENT_MESSAGE_SIZE: usize = underwrap_protocol::ipc::MAX_FRAME_SIZE;

/// Sign a challenge nonce using the first key reported by `$SSH_AUTH_SOCK`.
/// Returns `(signature_blob, public_key_blob)`.
pub(crate) fn sign_with_ssh_agent(nonce: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let auth_sock = std::env::var("SSH_AUTH_SOCK").context("SSH_AUTH_SOCK not set")?;
    let mut agent = UnixStream::connect(&auth_sock)
        .with_context(|| format!("connecting to SSH agent at {auth_sock}"))?;

    let identities = request_identities(&mut agent)?;
    if identities.is_empty() {
        bail!("no keys in SSH agent");
    }

    let (key_blob, key_comment) = &identities[0];
    let signature = sign(&mut agent, key_blob, nonce)?;

    eprintln!("signed challenge with key: {key_comment}");
    Ok((signature, key_blob.clone()))
}

fn request_identities(agent: &mut UnixStream) -> Result<Vec<(Vec<u8>, String)>> {
    let msg = [SSH_AGENTC_REQUEST_IDENTITIES];
    let len = (msg.len() as u32).to_be_bytes();
    agent.write_all(&len)?;
    agent.write_all(&msg)?;

    let resp = read_message(agent)?;
    parse_identities_response(&resp)
}

fn sign(agent: &mut UnixStream, key_blob: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let mut msg = Vec::new();
    msg.push(SSH_AGENTC_SIGN_REQUEST);
    msg.extend_from_slice(&(key_blob.len() as u32).to_be_bytes());
    msg.extend_from_slice(key_blob);
    msg.extend_from_slice(&(data.len() as u32).to_be_bytes());
    msg.extend_from_slice(data);
    msg.extend_from_slice(&0u32.to_be_bytes()); // flags

    let len = (msg.len() as u32).to_be_bytes();
    agent.write_all(&len)?;
    agent.write_all(&msg)?;

    let resp = read_message(agent)?;
    parse_sign_response(&resp)
}

fn read_message(stream: &mut impl std::io::Read) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let resp_len = u32::from_be_bytes(len_buf) as usize;
    anyhow::ensure!(
        resp_len <= MAX_SSH_AGENT_MESSAGE_SIZE,
        "SSH agent response too large: {resp_len} bytes"
    );
    let mut resp = vec![0u8; resp_len];
    stream.read_exact(&mut resp)?;
    Ok(resp)
}

fn take_ssh_string<'a>(buf: &'a [u8], pos: &mut usize, label: &str) -> Result<&'a [u8]> {
    anyhow::ensure!(
        *pos + 4 <= buf.len(),
        "truncated SSH agent {label} length field"
    );
    let len = u32::from_be_bytes([buf[*pos], buf[*pos + 1], buf[*pos + 2], buf[*pos + 3]]) as usize;
    *pos += 4;
    anyhow::ensure!(
        *pos + len <= buf.len(),
        "truncated SSH agent {label} payload"
    );
    let out = &buf[*pos..*pos + len];
    *pos += len;
    Ok(out)
}

fn parse_identities_response(resp: &[u8]) -> Result<Vec<(Vec<u8>, String)>> {
    anyhow::ensure!(!resp.is_empty(), "empty SSH agent identities response");
    anyhow::ensure!(
        resp[0] == SSH_AGENT_IDENTITIES_ANSWER,
        "unexpected agent response type: {}",
        resp[0]
    );
    anyhow::ensure!(
        resp.len() >= 5,
        "truncated SSH agent identities response header"
    );

    let nkeys = u32::from_be_bytes([resp[1], resp[2], resp[3], resp[4]]) as usize;
    let mut pos = 5;
    let mut keys = Vec::with_capacity(nkeys.min(32));

    for _ in 0..nkeys {
        let blob = take_ssh_string(resp, &mut pos, "key blob")?.to_vec();
        let comment_bytes = take_ssh_string(resp, &mut pos, "key comment")?;
        let comment = String::from_utf8_lossy(comment_bytes).to_string();
        keys.push((blob, comment));
    }

    anyhow::ensure!(
        pos == resp.len(),
        "trailing bytes in SSH agent identities response"
    );
    Ok(keys)
}

fn parse_sign_response(resp: &[u8]) -> Result<Vec<u8>> {
    anyhow::ensure!(!resp.is_empty(), "empty SSH agent sign response");
    anyhow::ensure!(
        resp[0] == SSH_AGENT_SIGN_RESPONSE,
        "agent refused to sign (response type: {})",
        resp[0]
    );
    let mut pos = 1;
    let sig = take_ssh_string(resp, &mut pos, "signature blob")?.to_vec();
    anyhow::ensure!(
        pos == resp.len(),
        "trailing bytes in SSH agent sign response"
    );
    Ok(sig)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build an agent-framed message (4-byte BE length + payload).
    fn frame(payload: &[u8]) -> Vec<u8> {
        let mut out = (payload.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(payload);
        out
    }

    /// Helper: build an SSH wire "string" (4-byte BE length + bytes).
    fn ssh_string(bytes: &[u8]) -> Vec<u8> {
        let mut out = (bytes.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(bytes);
        out
    }

    // --- read_message -------------------------------------------------------

    #[test]
    fn read_message_returns_payload_of_declared_length() {
        let wire = frame(&[1, 2, 3, 4]);
        let mut cursor = std::io::Cursor::new(wire);
        let msg = read_message(&mut cursor).unwrap();
        assert_eq!(msg, vec![1, 2, 3, 4]);
    }

    #[test]
    fn read_message_errors_when_stream_too_short() {
        let wire = [0, 0, 0, 8, 1, 2]; // claims 8 bytes, only 2 follow
        let mut cursor = std::io::Cursor::new(wire);
        assert!(read_message(&mut cursor).is_err());
    }

    #[test]
    fn read_message_rejects_oversize_frame() {
        let oversize = (MAX_SSH_AGENT_MESSAGE_SIZE + 1) as u32;
        let wire = oversize.to_be_bytes();
        let mut cursor = std::io::Cursor::new(wire);
        let err = read_message(&mut cursor).unwrap_err();
        assert!(err.to_string().contains("too large"));
    }

    // --- take_ssh_string ----------------------------------------------------

    #[test]
    fn take_ssh_string_reads_declared_bytes_and_advances_cursor() {
        let buf = ssh_string(b"abc");
        let mut pos = 0;
        let got = take_ssh_string(&buf, &mut pos, "x").unwrap();
        assert_eq!(got, b"abc");
        assert_eq!(pos, buf.len());
    }

    #[test]
    fn take_ssh_string_errors_on_truncated_length_field() {
        let buf = [0u8; 3]; // less than 4 bytes
        let mut pos = 0;
        let err = take_ssh_string(&buf, &mut pos, "x").unwrap_err();
        assert!(err.to_string().contains("length field"));
    }

    #[test]
    fn take_ssh_string_errors_on_truncated_payload() {
        let mut buf = (10u32).to_be_bytes().to_vec();
        buf.extend_from_slice(b"abc"); // claims 10, only 3 follow
        let mut pos = 0;
        let err = take_ssh_string(&buf, &mut pos, "x").unwrap_err();
        assert!(err.to_string().contains("payload"));
    }

    // --- parse_identities_response -----------------------------------------

    #[test]
    fn parse_identities_response_happy_path_with_two_keys() {
        let mut payload = vec![SSH_AGENT_IDENTITIES_ANSWER];
        payload.extend_from_slice(&2u32.to_be_bytes());
        payload.extend_from_slice(&ssh_string(b"blob-1"));
        payload.extend_from_slice(&ssh_string(b"comment one"));
        payload.extend_from_slice(&ssh_string(b"blob-2"));
        payload.extend_from_slice(&ssh_string(b"comment two"));

        let keys = parse_identities_response(&payload).unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].0, b"blob-1");
        assert_eq!(keys[0].1, "comment one");
        assert_eq!(keys[1].0, b"blob-2");
        assert_eq!(keys[1].1, "comment two");
    }

    #[test]
    fn parse_identities_response_happy_path_with_no_keys() {
        let mut payload = vec![SSH_AGENT_IDENTITIES_ANSWER];
        payload.extend_from_slice(&0u32.to_be_bytes());
        let keys = parse_identities_response(&payload).unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn parse_identities_response_rejects_empty() {
        assert!(parse_identities_response(&[]).is_err());
    }

    #[test]
    fn parse_identities_response_rejects_wrong_type_byte() {
        let payload = vec![99u8, 0, 0, 0, 0]; // not SSH_AGENT_IDENTITIES_ANSWER
        let err = parse_identities_response(&payload).unwrap_err();
        assert!(err.to_string().contains("unexpected agent response type"));
    }

    #[test]
    fn parse_identities_response_rejects_truncated_header() {
        let payload = vec![SSH_AGENT_IDENTITIES_ANSWER, 0, 0]; // missing count bytes
        assert!(parse_identities_response(&payload).is_err());
    }

    #[test]
    fn parse_identities_response_rejects_trailing_bytes() {
        let mut payload = vec![SSH_AGENT_IDENTITIES_ANSWER];
        payload.extend_from_slice(&0u32.to_be_bytes());
        payload.push(0xFF); // extra byte after declared 0 keys
        let err = parse_identities_response(&payload).unwrap_err();
        assert!(err.to_string().contains("trailing bytes"));
    }

    // --- parse_sign_response ------------------------------------------------

    #[test]
    fn parse_sign_response_happy_path() {
        let mut payload = vec![SSH_AGENT_SIGN_RESPONSE];
        payload.extend_from_slice(&ssh_string(b"signature-bytes"));
        let sig = parse_sign_response(&payload).unwrap();
        assert_eq!(sig, b"signature-bytes");
    }

    #[test]
    fn parse_sign_response_rejects_empty() {
        assert!(parse_sign_response(&[]).is_err());
    }

    #[test]
    fn parse_sign_response_rejects_wrong_type_byte() {
        let mut payload = vec![5u8]; // SSH_AGENT_FAILURE-ish, not 14
        payload.extend_from_slice(&ssh_string(b"x"));
        let err = parse_sign_response(&payload).unwrap_err();
        assert!(err.to_string().contains("agent refused to sign"));
    }

    #[test]
    fn parse_sign_response_rejects_trailing_bytes() {
        let mut payload = vec![SSH_AGENT_SIGN_RESPONSE];
        payload.extend_from_slice(&ssh_string(b"sig"));
        payload.push(0xAB);
        let err = parse_sign_response(&payload).unwrap_err();
        assert!(err.to_string().contains("trailing bytes"));
    }
}
