//! IPC message types exchanged over Unix domain sockets.
//!
//! All messages use the binary framing/codec defined in [`crate::ipc`].

mod common;
mod errors;
mod wrapper;
mod wrapper_resource;

pub use common::SecretBytes;
pub use errors::ErrorCode;
pub use wrapper::{WrapperRequest, WrapperResponse};
pub use wrapper_resource::{ResourceErrorCode, WrapperResourceRequest, WrapperResourceResponse};

#[cfg(test)]
mod tests;
