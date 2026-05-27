//! `WrapperVariant` — whether a wrapper invocation runs the configured
//! target (`Real`) or the unsafe debug tool (`Debug`).
//!
//! The variant travels on two wires: the wrapper→daemon IPC
//! (`WrapperRequest::Read`) and the daemon→backend runtime API
//! (`RuntimeWrapperInvocation*Request`). The backend is the sole policy
//! authority: a `Debug` invocation is only allowed when the matching
//! permission is in `UnsafeDebugUntil` mode (and not expired). Crucially,
//! the *daemon* — not the wrapper — chooses which binary to exec based on
//! the variant the backend authorised, so a client cannot send
//! `variant = Real` to pass an `Unlocked` policy and then locally exec the
//! debug tool.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::ipc::{WireDecode, WireEncode, WireReader};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, utoipa::ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum WrapperVariant {
    /// Run the wrapper's configured target binary.
    #[default]
    Real,
    /// Run the unsafe debug tool against the same resource. Only ever
    /// allowed under an `UnsafeDebugUntil` permission.
    Debug,
}

impl WireEncode for WrapperVariant {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Real => 0u8.encode(out),
            Self::Debug => 1u8.encode(out),
        }
    }
}

impl WireDecode for WrapperVariant {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::Real),
            1 => Ok(Self::Debug),
            other => Err(anyhow!("invalid wrapper variant discriminant: {other}")),
        }
    }
}
