//! Structured audit logging.
//!
//! All security-relevant events are emitted as structured JSON events
//! via the `tracing` framework. The daemon's tracing subscriber can be
//! configured to write these to a file, journald, or any other sink.
//!
//! This module provides helper functions that emit audit events with
//! consistent field names. Secret material is NEVER included.
//!
//! Grants are logged at `info`; denials at `warn`.

use tracing::{info, warn};

/// Audit event for a secret read.
pub fn read_granted(wrapper: &str, secret: &str, peer_uid: Option<u32>, scope: Option<&str>) {
    info!(
        audit = "read_granted",
        wrapper = wrapper,
        secret = secret,
        peer_uid = peer_uid,
        scope = scope,
    );
}

/// Audit event for a denied read.
pub fn read_denied(wrapper: &str, reason: &str, peer_uid: Option<u32>, scope: Option<&str>) {
    warn!(
        audit = "read_denied",
        wrapper = wrapper,
        reason = reason,
        peer_uid = peer_uid,
        scope = scope,
    );
}

/// Audit event for a secret store.
pub fn secret_stored(secret: &str, peer_uid: Option<u32>) {
    info!(
        audit = "secret_stored",
        secret = secret,
        peer_uid = peer_uid,
    );
}

/// Audit event for a denied store.
pub fn store_denied(secret: &str, reason: &str, peer_uid: Option<u32>) {
    warn!(
        audit = "store_denied",
        secret = secret,
        reason = reason,
        peer_uid = peer_uid,
    );
}

/// Audit event for unlock.
pub fn unlocked(wrapper: &str, scope: Option<&str>, seconds: Option<u64>, peer_uid: Option<u32>) {
    info!(
        audit = "unlocked",
        wrapper = wrapper,
        scope = scope,
        seconds = seconds,
        peer_uid = peer_uid,
    );
}

/// Audit event for lock.
pub fn locked(target: Option<&str>, scope: Option<&str>, peer_uid: Option<u32>) {
    info!(
        audit = "locked",
        target = target,
        scope = scope,
        peer_uid = peer_uid,
    );
}

/// Audit event for auth denial on a socket.
pub fn auth_denied(socket: &str, reason: &str, peer_uid: Option<u32>, peer_gid: Option<u32>) {
    warn!(
        audit = "auth_denied",
        socket = socket,
        reason = reason,
        peer_uid = peer_uid,
        peer_gid = peer_gid,
    );
}

/// Audit event for a secretsd channel read.
pub fn secretsd_read(wrapper: &str, item: &str) {
    info!(audit = "secretsd_read", wrapper = wrapper, item = item,);
}

/// Audit event for a secretsd channel write.
pub fn secretsd_write(wrapper: &str, item: &str) {
    info!(audit = "secretsd_write", wrapper = wrapper, item = item,);
}

/// Audit event for a secretsd channel delete.
pub fn secretsd_delete(wrapper: &str, item: &str) {
    info!(audit = "secretsd_delete", wrapper = wrapper, item = item,);
}
