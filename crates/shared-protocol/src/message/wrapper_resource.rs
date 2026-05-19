//! Wrapper-side bulk resource IPC.
//!
//! Replaces the per-call libsecret/directory protocols. Three verbs:
//!
//! - `Checkout` — wrapper requests the resource blob and writability
//!   flag at start of run.
//! - `Checkin` — wrapper sends a replacement blob at end of run when
//!   the JCS-canonical bytes changed and the grant was writable.
//! - `UsageSignal` — wrapper pings the daemon to record that the
//!   resource was used in this run (bumps `last_used_at` on the
//!   `(wrapper, resource)` pair) without sending a blob.
//!
//! The channel is per-resource: the daemon sets up one socketpair per
//! resource in scope and hands the wrapper end via `SCM_RIGHTS`. The
//! `resource_id` is implicit per channel and not on the wire.

use anyhow::{Result, anyhow};

use crate::ipc::{WireDecode, WireEncode, WireReader};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrapperResourceRequest {
    Checkout,
    Checkin { blob: Vec<u8> },
    UsageSignal,
}

impl WireEncode for WrapperResourceRequest {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Checkout => 0u8.encode(out),
            Self::Checkin { blob } => {
                1u8.encode(out)?;
                blob.encode(out)
            }
            Self::UsageSignal => 2u8.encode(out),
        }
    }
}

impl WireDecode for WrapperResourceRequest {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::Checkout),
            1 => Ok(Self::Checkin {
                blob: Vec::<u8>::decode(reader)?,
            }),
            2 => Ok(Self::UsageSignal),
            other => Err(anyhow!(
                "invalid wrapper-resource request discriminant: {other}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrapperResourceResponse {
    CheckoutOk {
        blob: Vec<u8>,
        writable: bool,
    },
    CheckinOk,
    UsageSignalOk,
    Error {
        code: ResourceErrorCode,
        message: String,
    },
}

impl WireEncode for WrapperResourceResponse {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::CheckoutOk { blob, writable } => {
                0u8.encode(out)?;
                blob.encode(out)?;
                writable.encode(out)
            }
            Self::CheckinOk => 1u8.encode(out),
            Self::UsageSignalOk => 2u8.encode(out),
            Self::Error { code, message } => {
                3u8.encode(out)?;
                code.encode(out)?;
                message.encode(out)
            }
        }
    }
}

impl WireDecode for WrapperResourceResponse {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::CheckoutOk {
                blob: Vec::<u8>::decode(reader)?,
                writable: bool::decode(reader)?,
            }),
            1 => Ok(Self::CheckinOk),
            2 => Ok(Self::UsageSignalOk),
            3 => Ok(Self::Error {
                code: ResourceErrorCode::decode(reader)?,
                message: String::decode(reader)?,
            }),
            other => Err(anyhow!(
                "invalid wrapper-resource response discriminant: {other}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceErrorCode {
    InvalidRequest,
    Unauthorized,
    NotFound,
    Internal,
}

impl WireEncode for ResourceErrorCode {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::InvalidRequest => 0u8.encode(out),
            Self::Unauthorized => 1u8.encode(out),
            Self::NotFound => 2u8.encode(out),
            Self::Internal => 3u8.encode(out),
        }
    }
}

impl WireDecode for ResourceErrorCode {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::InvalidRequest),
            1 => Ok(Self::Unauthorized),
            2 => Ok(Self::NotFound),
            3 => Ok(Self::Internal),
            other => Err(anyhow!(
                "invalid wrapper-resource error code discriminant: {other}"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{decode_from_slice, encode_to_vec};

    #[test]
    fn checkout_roundtrips() {
        let req = WrapperResourceRequest::Checkout;
        let bytes = encode_to_vec(&req).unwrap();
        let back: WrapperResourceRequest = decode_from_slice(&bytes).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn checkin_roundtrips() {
        let req = WrapperResourceRequest::Checkin {
            blob: b"\x00\x01canonical-json-bytes".to_vec(),
        };
        let bytes = encode_to_vec(&req).unwrap();
        let back: WrapperResourceRequest = decode_from_slice(&bytes).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn usage_signal_roundtrips() {
        let req = WrapperResourceRequest::UsageSignal;
        let bytes = encode_to_vec(&req).unwrap();
        let back: WrapperResourceRequest = decode_from_slice(&bytes).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn checkout_ok_response_roundtrips() {
        let resp = WrapperResourceResponse::CheckoutOk {
            blob: b"{}".to_vec(),
            writable: true,
        };
        let bytes = encode_to_vec(&resp).unwrap();
        let back: WrapperResourceResponse = decode_from_slice(&bytes).unwrap();
        assert_eq!(back, resp);
    }

    #[test]
    fn error_response_roundtrips() {
        let resp = WrapperResourceResponse::Error {
            code: ResourceErrorCode::Unauthorized,
            message: "writes not permitted by grant".to_string(),
        };
        let bytes = encode_to_vec(&resp).unwrap();
        let back: WrapperResourceResponse = decode_from_slice(&bytes).unwrap();
        assert_eq!(back, resp);
    }

    #[test]
    fn unknown_request_discriminant_rejected() {
        let bad = vec![9u8];
        let err = decode_from_slice::<WrapperResourceRequest>(&bad).unwrap_err();
        assert!(format!("{err:#}").contains("invalid wrapper-resource request"));
    }
}
