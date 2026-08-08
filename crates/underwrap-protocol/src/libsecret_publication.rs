//! Private invocation-local libsecret publication framing.
//!
//! This ABI is deliberately independent of the generic 64-KiB IPC framing.

pub const MAGIC: [u8; 4] = *b"UWLP";
pub const VERSION: u8 = 1;
pub const PUBLISH_TAG: u8 = 1;
pub const ACK_TAG: u8 = 2;
pub const REJECT_TAG: u8 = 3;
pub const REQUEST_HEADER_LEN: usize = 18;
pub const RESPONSE_LEN: usize = 14;
pub const MAX_BODY_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestHeader {
    pub generation: u64,
    pub body_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseKind {
    Ack,
    Reject,
}

pub fn encode_request_header(
    generation: u64,
    body_len: usize,
) -> Result<[u8; REQUEST_HEADER_LEN], String> {
    if body_len > MAX_BODY_BYTES {
        return Err(format!(
            "publication body length {body_len} exceeds {MAX_BODY_BYTES}"
        ));
    }
    let mut out = [0; REQUEST_HEADER_LEN];
    out[..4].copy_from_slice(&MAGIC);
    out[4] = VERSION;
    out[5] = PUBLISH_TAG;
    out[6..14].copy_from_slice(&generation.to_be_bytes());
    out[14..18].copy_from_slice(&(body_len as u32).to_be_bytes());
    Ok(out)
}

pub fn decode_request_header(bytes: &[u8; REQUEST_HEADER_LEN]) -> Result<RequestHeader, String> {
    if bytes[..4] != MAGIC {
        return Err("invalid publication magic".into());
    }
    if bytes[4] != VERSION {
        return Err("invalid publication version".into());
    }
    if bytes[5] != PUBLISH_TAG {
        return Err("invalid publication request tag".into());
    }
    let generation = u64::from_be_bytes(bytes[6..14].try_into().unwrap());
    let body_len = u32::from_be_bytes(bytes[14..18].try_into().unwrap()) as usize;
    if body_len > MAX_BODY_BYTES {
        return Err(format!(
            "publication body length {body_len} exceeds {MAX_BODY_BYTES}"
        ));
    }
    Ok(RequestHeader {
        generation,
        body_len,
    })
}

pub fn encode_response(kind: ResponseKind, generation: u64) -> [u8; RESPONSE_LEN] {
    let mut out = [0; RESPONSE_LEN];
    out[..4].copy_from_slice(&MAGIC);
    out[4] = VERSION;
    out[5] = match kind {
        ResponseKind::Ack => ACK_TAG,
        ResponseKind::Reject => REJECT_TAG,
    };
    out[6..14].copy_from_slice(&generation.to_be_bytes());
    out
}

pub fn decode_response(bytes: &[u8; RESPONSE_LEN]) -> Result<(ResponseKind, u64), String> {
    if bytes[..4] != MAGIC {
        return Err("invalid publication response magic".into());
    }
    if bytes[4] != VERSION {
        return Err("invalid publication response version".into());
    }
    let kind = match bytes[5] {
        ACK_TAG => ResponseKind::Ack,
        REJECT_TAG => ResponseKind::Reject,
        _ => return Err("invalid publication response tag".into()),
    };
    Ok((kind, u64::from_be_bytes(bytes[6..14].try_into().unwrap())))
}

#[cfg(test)]
mod tests;
