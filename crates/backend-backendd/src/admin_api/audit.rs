//! Structured `admin event` log emitter.
//!
//! Every admin mutation calls this helper after the action completes
//! (success or failure) so operators have a single grep target for
//! audit-relevant lines. Per-subdomain audit functions
//! (`write_admin_user_audit`, `write_admin_store_audit`, ...) live
//! next to their subdomain's handlers and call into this.

use tracing::info;

pub(crate) fn log_admin_event(
    action: &str,
    operator: &str,
    wrapper: Option<&str>,
    resource: Option<&str>,
    secret_id: Option<&str>,
    operation: Option<&str>,
    result: &str,
) {
    info!(
        action = %action,
        operator = %operator,
        wrapper = ?wrapper,
        resource = ?resource,
        secret_id = ?secret_id,
        operation = ?operation,
        result = %result,
        "admin event"
    );
}
