//! Tiny time-of-day helper used across the backend.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current epoch seconds, saturating to 0 on the very-unlikely
/// pre-1970 clock case.
pub(crate) fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
