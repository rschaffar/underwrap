//! Private child-to-parent authorization gate for end-of-run dispatch.
//!
//! The namespace child reports controlled setup failure explicitly. Once all
//! controlled setup and materialization is complete it writes `Armed`; a
//! successful target exec then closes the writer atomically through CLOEXEC. The
//! parent authorizes collection/finalization only for the exact `Armed` stream
//! followed by EOF.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use underwrap_protocol::exec::{EXEC_BUNDLE_FD, WRAPPER_RESOURCE_CHANNEL_FD};

const ARMED: &[u8] = b"Armed\n";
const FAILED: &[u8] = b"Failed\n";
const MAX_GATE_BYTES: usize = ARMED.len() + FAILED.len();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GateFailure {
    EofBeforeArmed,
    Failed,
    Malformed,
    Io,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GateAuthorization {
    Accepted,
    Suppressed(GateFailure),
}

pub(crate) struct ExecGate {
    reader: GateReader,
    writer: GateWriter,
}

pub(crate) struct GateReader {
    file: File,
}

pub(crate) struct GateWriter {
    file: File,
}

impl ExecGate {
    pub(crate) fn new() -> Result<Self, String> {
        Self::create(true)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test() -> Result<Self, String> {
        Self::create(false)
    }

    fn create(require_reserved_fds_occupied: bool) -> Result<Self, String> {
        let mut fds = [-1; 2];
        // SAFETY: `fds` points to two writable integers. On success pipe2
        // returns two new, uniquely owned descriptors with CLOEXEC set
        // atomically; both are immediately adopted by OwnedFd.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(format!(
                "creating exec authorization gate: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: successful pipe2 returned two distinct, open descriptors and
        // ownership has not been transferred elsewhere.
        let reader = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        // SAFETY: same invariant as above for the pipe writer.
        let writer = unsafe { OwnedFd::from_raw_fd(fds[1]) };

        // The gate is created while fixed fd 3 and fd 20 are still occupied.
        // Reject rather than corrupt either local ABI if that ordering ever
        // regresses.
        for fd in [reader.as_raw_fd(), writer.as_raw_fd()] {
            if require_reserved_fds_occupied
                && (fd == EXEC_BUNDLE_FD || fd == WRAPPER_RESOURCE_CHANNEL_FD)
            {
                return Err(format!(
                    "exec authorization gate collided with reserved fd {fd}"
                ));
            }
        }

        Ok(Self {
            reader: GateReader {
                file: File::from(reader),
            },
            writer: GateWriter {
                file: File::from(writer),
            },
        })
    }

    #[cfg(test)]
    pub(crate) fn into_ends(self) -> (GateReader, GateWriter) {
        (self.reader, self.writer)
    }

    /// Consume the post-fork copy in the supervisor, closing its writer.
    pub(crate) fn into_parent(self) -> GateReader {
        let Self { reader, writer } = self;
        drop(writer);
        reader
    }

    /// Consume the post-fork copy in the namespace child, closing its reader.
    pub(crate) fn into_child(self) -> GateWriter {
        let Self { reader, writer } = self;
        drop(reader);
        writer
    }
}

impl GateReader {
    pub(crate) fn observe(mut self) -> GateAuthorization {
        let mut bytes = Vec::with_capacity(MAX_GATE_BYTES);
        let mut chunk = [0_u8; 16];
        loop {
            match self.file.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => {
                    bytes.extend_from_slice(&chunk[..count]);
                    if bytes.len() > MAX_GATE_BYTES {
                        return GateAuthorization::Suppressed(GateFailure::Malformed);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return GateAuthorization::Suppressed(GateFailure::Io),
            }
        }

        if bytes == ARMED {
            GateAuthorization::Accepted
        } else if bytes.is_empty() {
            GateAuthorization::Suppressed(GateFailure::EofBeforeArmed)
        } else if bytes == FAILED || bytes == [ARMED, FAILED].concat() {
            GateAuthorization::Suppressed(GateFailure::Failed)
        } else {
            GateAuthorization::Suppressed(GateFailure::Malformed)
        }
    }
}

impl GateWriter {
    pub(crate) fn raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    pub(crate) fn send_armed(&mut self) -> Result<(), String> {
        self.write_marker(ARMED, "Armed")
    }

    pub(crate) fn send_failed(&mut self) -> Result<(), String> {
        self.write_marker(FAILED, "Failed")
    }

    fn write_marker(&mut self, marker: &[u8], name: &str) -> Result<(), String> {
        self.file
            .write_all(marker)
            .map_err(|e| format!("writing exec authorization {name} marker: {e}"))
    }
}

#[cfg(test)]
mod tests;
