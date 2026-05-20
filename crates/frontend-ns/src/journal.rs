//! Minimal client for the systemd-journald native protocol.
//!
//! `/run/systemd/journal/socket` is an `AF_UNIX` `SOCK_DGRAM` listener
//! that accepts structured log entries as one datagram per record.
//! Each entry is a concatenation of `KEY=value\n` lines. We only emit
//! short fields with no embedded newlines, so the simple text form is
//! always sufficient — no memfd fallback, no length-prefixed binary
//! frames. See `systemd.journal-fields(7)` and `sd_journal_send(3)`.
//!
//! Used by `report_outcomes` to route the structured `writeback_rejected`
//! event into the journal rather than the wrapper's stderr, so an
//! interactive terminal does not see the audit line next to the
//! human-readable warning. The audit channel must remain authoritative,
//! so we fall back to stderr only when the journal socket is not
//! reachable (non-systemd hosts, tests). Suppression env vars do not
//! gate either path.
//!
//! The socket is opened lazily per record. The data here is not
//! secret material and the cost is negligible compared to the
//! surrounding `Checkin` round-trip.
//!
//! Datagram size: kept well under the kernel default SO_SNDBUF (~200KB
//! on Linux) by emitting only short identifiers and a one-line message.

use std::io::Write;
use std::os::unix::net::UnixDatagram;
use std::path::Path;

const JOURNAL_SOCKET: &str = "/run/systemd/journal/socket";

/// Send a structured record to journald. Returns `Ok(())` if the
/// record was accepted by the kernel; `Err` if the socket is missing
/// or the send failed. Callers should treat the error case as
/// "journald not available" and fall back to stderr.
///
/// `fields` is a list of `(key, value)` pairs. `key` must match
/// journald's field-name rules (uppercase ASCII, digits, underscore,
/// must not start with a digit or underscore). `value` must not
/// contain `\n` — callers are responsible for normalizing newlines
/// out of any human-readable strings.
pub fn send_record(fields: &[(&str, &str)]) -> std::io::Result<()> {
    if !Path::new(JOURNAL_SOCKET).exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "journald socket not present",
        ));
    }

    let mut buf = Vec::with_capacity(256);
    for (key, value) in fields {
        debug_assert!(
            !value.contains('\n'),
            "journal field {key} contains newline; caller must normalize"
        );
        writeln!(buf, "{key}={value}")?;
    }

    let socket = UnixDatagram::unbound()?;
    socket.send_to(&buf, JOURNAL_SOCKET)?;
    Ok(())
}
