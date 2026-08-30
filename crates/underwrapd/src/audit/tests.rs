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
fn every_audit_helper_emits_expected_event_names_and_fields_without_scope() {
    let output = capture_logs(|| {
        read_granted("gh-authorized", "github/token", Some(1000));
        read_denied("gh-authorized", "credential missing", Some(1001));
        auth_denied("wrapper", "gid mismatch", Some(1002), Some(2000));
        resource_checkout("gh-authorized", "res_abc");
        resource_checkin("gh-authorized", "res_abc");
        resource_usage_signal("gh-authorized", "res_abc");
        wrapper_write_denied(
            "resource",
            "gh-authorized",
            "res_abc",
            "<full-blob>",
            "permission is read-only",
        );
    });

    assert_eq!(output.matches("audit=").count(), 7, "{output}");
    for event in [
        "read_granted",
        "read_denied",
        "auth_denied",
        "resource_checkout",
        "resource_checkin",
        "resource_usage_signal",
        "store_denied",
    ] {
        assert!(output.contains(&format!("audit=\"{event}\"")), "{output}");
    }
    assert!(output.contains("wrapper=\"gh-authorized\""), "{output}");
    assert!(output.contains("secret=\"github/token\""), "{output}");
    assert!(output.contains("reason=\"credential missing\""), "{output}");
    assert!(output.contains("peer_uid=1000"), "{output}");
    assert!(output.contains("peer_gid=2000"), "{output}");
    assert!(output.contains("resource=\"res_abc\""), "{output}");
    assert!(output.contains("kind=\"resource\""), "{output}");
    assert!(output.contains("credential=\"res_abc\""), "{output}");
    assert!(output.contains("item=\"<full-blob>\""), "{output}");
    assert!(!output.contains("scope"), "{output}");
}

#[test]
fn audit_logging_includes_identifiers_but_not_secret_material_values() {
    let output = capture_logs(|| {
        read_granted("gh-authorized", "github/token", Some(1000));
        wrapper_write_denied(
            "resource",
            "gh-authorized",
            "res_abc",
            "<full-blob>",
            "permission is read-only",
        );
    });

    assert!(output.contains("github/token"), "{output}");
    assert!(output.contains("res_abc"), "{output}");
    assert!(!output.contains("ghp_secret_value"), "{output}");
    assert!(!output.contains("super-secret-token"), "{output}");
}
