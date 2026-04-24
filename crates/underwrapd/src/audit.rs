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

/// Audit event for approval.
pub fn approved(wrapper: &str, scope: Option<&str>, seconds: Option<u64>, peer_uid: Option<u32>) {
    info!(
        audit = "approved",
        wrapper = wrapper,
        scope = scope,
        seconds = seconds,
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

/// Audit event for a denied wrapper-initiated write (secretsd or directory).
///
/// Emitted when the policy gate refuses a `store.write` for the given
/// wrapper/credential. `kind` is `"libsecret"` or `"directory"`, `item` is
/// the relative item id or file path requested by the wrapper.
pub fn wrapper_write_denied(
    kind: &str,
    wrapper: &str,
    credential: &str,
    item: &str,
    reason: &str,
) {
    warn!(
        audit = "store_denied",
        kind = kind,
        wrapper = wrapper,
        credential = credential,
        item = item,
        reason = reason,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::sync::{Arc, Mutex};
    use tracing::subscriber::with_default;
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    struct GuardedWriter(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for SharedWriter {
        type Writer = GuardedWriter;

        fn make_writer(&'a self) -> Self::Writer {
            GuardedWriter(Arc::clone(&self.0))
        }
    }

    impl io::Write for GuardedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn capture_logs(f: impl FnOnce()) -> String {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_writer(SharedWriter(Arc::clone(&buffer)))
            .with_ansi(false)
            .without_time()
            .finish();

        with_default(subscriber, f);
        String::from_utf8(buffer.lock().unwrap().clone()).unwrap()
    }

    #[test]
    fn audit_helpers_emit_expected_event_names_and_fields() {
        let output = capture_logs(|| {
            read_granted(
                "gh-authorized",
                "github/token",
                Some(1000),
                Some("session-1"),
            );
            auth_denied("wrapper", "gid mismatch", Some(1000), Some(2000));
            secretsd_write("gh-authorized", "default/item_1");
        });

        assert!(output.contains("audit=\"read_granted\""), "{output}");
        assert!(output.contains("wrapper=\"gh-authorized\""), "{output}");
        assert!(output.contains("secret=\"github/token\""), "{output}");
        assert!(output.contains("scope=\"session-1\""), "{output}");
        assert!(output.contains("audit=\"auth_denied\""), "{output}");
        assert!(output.contains("reason=\"gid mismatch\""), "{output}");
        assert!(output.contains("peer_gid=2000"), "{output}");
        assert!(output.contains("audit=\"secretsd_write\""), "{output}");
        assert!(output.contains("item=\"default/item_1\""), "{output}");
    }

    #[test]
    fn audit_logging_does_not_include_secret_material_values() {
        let output = capture_logs(|| {
            secret_stored("github/token", Some(1000));
            read_denied("gh-authorized", "credential missing", Some(1000), None);
        });

        assert!(output.contains("github/token"), "{output}");
        assert!(!output.contains("ghp_secret_value"), "{output}");
        assert!(!output.contains("super-secret-token"), "{output}");
    }
}
