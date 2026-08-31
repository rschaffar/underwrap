//! Security-relevant daemon event helpers.
//!
//! These helpers emit consistently named, typed [`tracing`] fields without
//! resource material values. Production `underwrapd` installs the default
//! human-readable [`tracing_subscriber::fmt()`] formatter and its default stdout
//! writer. `RUST_LOG` configures the environment filter and defaults to `info`.
//! The daemon has no audit JSON mode or configured file or journald sink; a
//! service manager can route stdout externally.
//!
//! Unit tests use a test-only writer to inspect fields. They do not define the
//! production format or sink contract.

use tracing::{info, warn};

/// Audit event for an allowed resource read.
pub fn read_allowed(wrapper: &str, resource: &str, peer_uid: Option<u32>) {
    info!(
        audit = "read_allowed",
        wrapper = wrapper,
        resource = resource,
        peer_uid = peer_uid,
    );
}

/// Audit event for a denied resource-read permission outcome.
pub fn read_denied(wrapper: &str, reason: &str, peer_uid: Option<u32>) {
    warn!(
        audit = "read_denied",
        wrapper = wrapper,
        reason = reason,
        peer_uid = peer_uid,
    );
}

/// Audit event for a denied peer-authentication outcome on a socket.
pub fn auth_denied(socket: &str, reason: &str, peer_uid: Option<u32>, peer_gid: Option<u32>) {
    warn!(
        audit = "auth_denied",
        socket = socket,
        reason = reason,
        peer_uid = peer_uid,
        peer_gid = peer_gid,
    );
}

/// Audit event for a wrapper-resource channel Checkout.
pub fn resource_checkout(wrapper: &str, resource: &str) {
    info!(
        audit = "resource_checkout",
        wrapper = wrapper,
        resource = resource,
    );
}

/// Audit event for a wrapper-resource channel Checkin (accepted blob write).
pub fn resource_checkin(wrapper: &str, resource: &str) {
    info!(
        audit = "resource_checkin",
        wrapper = wrapper,
        resource = resource,
    );
}

/// Audit event for a wrapper-resource channel UsageSignal (no-op end-of-run ping).
pub fn resource_usage_signal(wrapper: &str, resource: &str) {
    info!(
        audit = "resource_usage_signal",
        wrapper = wrapper,
        resource = resource,
    );
}

/// Audit event for a denied complete-resource Checkin.
pub fn resource_checkin_denied(wrapper: &str, resource: &str, reason: &str) {
    warn!(
        audit = "resource_checkin_denied",
        wrapper = wrapper,
        resource = resource,
        reason = reason,
    );
}

#[cfg(test)]
mod tests;
