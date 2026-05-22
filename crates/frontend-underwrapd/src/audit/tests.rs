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
