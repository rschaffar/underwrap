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

fn assert_event_line(line: &str, level: &str, fields: &str) {
    assert!(
        line.trim_start().starts_with(&format!("{level} ")),
        "{line}"
    );
    assert!(line.ends_with(fields), "{line}");
    assert_eq!(line.matches("audit=").count(), 1, "{line}");
}

#[test]
fn every_audit_helper_emits_one_exact_event_line() {
    let output = capture_logs(|| {
        read_allowed("gh-authorized", "res_read", Some(1000));
        read_denied("gh-authorized", "permission missing", Some(1001));
        auth_denied("wrapper", "gid mismatch", Some(1002), Some(2000));
        resource_checkout("gh-authorized", "res_abc");
        resource_checkin("gh-authorized", "res_abc");
        resource_usage_signal("gh-authorized", "res_abc");
        resource_checkin_denied("gh-authorized", "res_abc", "permission is read-only");
    });

    let lines = output.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 7, "{output}");
    for (line, level, fields) in [
        (
            lines[0],
            "INFO",
            "audit=\"read_allowed\" wrapper=\"gh-authorized\" resource=\"res_read\" peer_uid=1000",
        ),
        (
            lines[1],
            "WARN",
            "audit=\"read_denied\" wrapper=\"gh-authorized\" reason=\"permission missing\" peer_uid=1001",
        ),
        (
            lines[2],
            "WARN",
            "audit=\"auth_denied\" socket=\"wrapper\" reason=\"gid mismatch\" peer_uid=1002 peer_gid=2000",
        ),
        (
            lines[3],
            "INFO",
            "audit=\"resource_checkout\" wrapper=\"gh-authorized\" resource=\"res_abc\"",
        ),
        (
            lines[4],
            "INFO",
            "audit=\"resource_checkin\" wrapper=\"gh-authorized\" resource=\"res_abc\"",
        ),
        (
            lines[5],
            "INFO",
            "audit=\"resource_usage_signal\" wrapper=\"gh-authorized\" resource=\"res_abc\"",
        ),
        (
            lines[6],
            "WARN",
            "audit=\"resource_checkin_denied\" wrapper=\"gh-authorized\" resource=\"res_abc\" reason=\"permission is read-only\"",
        ),
    ] {
        assert_event_line(line, level, fields);
    }
    for legacy in [
        "read_granted",
        "store_denied",
        "secret=",
        "kind=",
        "credential=",
        "item=",
        "scope=",
    ] {
        assert!(!output.contains(legacy), "legacy {legacy:?}: {output}");
    }
}

#[test]
fn audit_logging_includes_identifiers_but_not_resource_material_values() {
    let output = capture_logs(|| {
        read_allowed("gh-authorized", "res_read", Some(1000));
        resource_checkin_denied("gh-authorized", "res_abc", "permission is read-only");
    });

    assert!(output.contains("res_read"), "{output}");
    assert!(output.contains("res_abc"), "{output}");
    assert!(!output.contains("ghp_secret_value"), "{output}");
    assert!(!output.contains("super-secret-token"), "{output}");
}
