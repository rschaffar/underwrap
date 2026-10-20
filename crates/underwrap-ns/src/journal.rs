//! Minimal client for the systemd-journald native protocol.
//!
//! `/run/systemd/journal/socket` is an `AF_UNIX` `SOCK_DGRAM` listener
//! that accepts structured log entries as one datagram per record. Safe values
//! use text `KEY=value\n` fields. Values containing a newline use journald's
//! native binary framing: `KEY\n`, a little-endian `u64` byte length, the raw
//! UTF-8 value, and a trailing newline. This prevents metadata such as an
//! operator-facing resource name from injecting a second field. See
//! `systemd.journal-fields(7)` and `sd_journal_send(3)`.
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

use std::os::unix::net::UnixDatagram;
use std::path::Path;

const JOURNAL_SOCKET: &str = "/run/systemd/journal/socket";

/// Encode a journald native-protocol record without performing I/O.
///
/// `fields` keys must match journald's field-name rules (uppercase ASCII,
/// digits, underscore, and neither a digit nor underscore first). Text fields
/// are used only for values without a newline; newline-containing values use
/// native binary framing so their raw bytes stay part of one field value.
fn encode_record(fields: &[(&str, &str)]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);
    for (key, value) in fields {
        if value.contains('\n') {
            buf.extend_from_slice(key.as_bytes());
            buf.push(b'\n');
            buf.extend_from_slice(&(value.len() as u64).to_le_bytes());
            buf.extend_from_slice(value.as_bytes());
            buf.push(b'\n');
        } else {
            buf.extend_from_slice(key.as_bytes());
            buf.push(b'=');
            buf.extend_from_slice(value.as_bytes());
            buf.push(b'\n');
        }
    }
    buf
}

/// Send a structured record to journald. Returns `Ok(())` if the
/// record was accepted by the kernel; `Err` if the socket is missing
/// or the send failed. Callers should treat the error case as
/// "journald not available" and fall back to stderr.
pub fn send_record(fields: &[(&str, &str)]) -> std::io::Result<()> {
    #[cfg(test)]
    let test_path = crate::test_support::JOURNAL_SOCKET.with(|path| path.borrow().clone());
    #[cfg(test)]
    let path = test_path.as_deref().unwrap_or(Path::new(JOURNAL_SOCKET));
    #[cfg(not(test))]
    let path = Path::new(JOURNAL_SOCKET);
    if !path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "journald socket not present",
        ));
    }

    let buf = encode_record(fields);
    let socket = UnixDatagram::unbound()?;
    socket.send_to(&buf, path)?;
    Ok(())
}

#[cfg(test)]
mod tests;
