//! Exact private relay-to-host direct-target status framing.

use std::os::fd::{AsRawFd, OwnedFd};

use crate::supervision::cloexec_pipe;

const MAGIC: [u8; 4] = *b"UWTS";
const VERSION: u8 = 1;
const EXITED: u8 = 1;
const SIGNALED: u8 = 2;
const FRAME_LEN: usize = 7;

fn valid_linux_signal(signal: libc::c_int) -> bool {
    signal > 0 && signal <= libc::SIGRTMAX()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetStatus {
    Exited(u8),
    Signaled(libc::c_int),
}

pub(crate) struct TargetStatusPipe {
    reader: TargetStatusReader,
    writer: TargetStatusWriter,
}

pub(crate) struct TargetStatusReader(OwnedFd);
pub(crate) struct TargetStatusWriter(OwnedFd);

impl TargetStatusPipe {
    pub(crate) fn new() -> Result<Self, String> {
        let (reader, writer) =
            cloexec_pipe().map_err(|error| format!("creating target-status pipe: {error}"))?;
        let reader = TargetStatusReader(reader);
        let writer = TargetStatusWriter(writer);
        Ok(Self { reader, writer })
    }

    pub(crate) fn into_host(self) -> TargetStatusReader {
        let Self { reader, writer } = self;
        drop(writer);
        reader
    }

    pub(crate) fn into_relay(self) -> TargetStatusWriter {
        let Self { reader, writer } = self;
        drop(reader);
        writer
    }
}

impl TargetStatus {
    pub(crate) fn from_wait_status(status: libc::c_int) -> Result<Self, String> {
        if libc::WIFEXITED(status) {
            return Ok(Self::Exited(libc::WEXITSTATUS(status) as u8));
        }
        if libc::WIFSIGNALED(status) {
            let signal = libc::WTERMSIG(status);
            if valid_linux_signal(signal) {
                return Ok(Self::Signaled(signal));
            }
        }
        Err(format!(
            "direct target returned invalid wait status {status}"
        ))
    }
}

impl TargetStatusWriter {
    pub(crate) fn write(self, status: TargetStatus) -> Result<(), String> {
        let (tag, value) = match status {
            TargetStatus::Exited(code) => (EXITED, code),
            TargetStatus::Signaled(signal) if valid_linux_signal(signal) => {
                (SIGNALED, signal as u8)
            }
            TargetStatus::Signaled(signal) => {
                return Err(format!("invalid target signal {signal}"));
            }
        };
        let frame = [MAGIC[0], MAGIC[1], MAGIC[2], MAGIC[3], VERSION, tag, value];
        loop {
            // SAFETY: frame remains valid for the complete one-shot write and
            // the owned pipe descriptor is open. The frame is below PIPE_BUF.
            let written = unsafe {
                libc::write(
                    self.0.as_raw_fd(),
                    frame.as_ptr().cast::<libc::c_void>(),
                    frame.len(),
                )
            };
            if written == frame.len() as isize {
                return Ok(());
            }
            if written < 0
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            {
                continue;
            }
            if written < 0 {
                return Err(format!(
                    "writing target-status frame: {}",
                    std::io::Error::last_os_error()
                ));
            }
            return Err(format!(
                "short target-status write: {written}/{}",
                frame.len()
            ));
        }
    }

    #[cfg(test)]
    pub(crate) fn write_raw_for_test(self, bytes: &[u8]) -> Result<(), String> {
        // SAFETY: bytes remains valid for this single bounded test write and the
        // writer owns an open pipe descriptor.
        let written = unsafe {
            libc::write(
                self.0.as_raw_fd(),
                bytes.as_ptr().cast::<libc::c_void>(),
                bytes.len(),
            )
        };
        if written == bytes.len() as isize {
            Ok(())
        } else {
            Err(format!("test raw target-status write returned {written}"))
        }
    }
}

impl TargetStatusReader {
    /// Parse only after a clean relay reap. Nonblocking mode ensures a leaked
    /// out-of-namespace writer fails with EAGAIN rather than hanging.
    pub(crate) fn read_after_clean_relay(self) -> Result<TargetStatus, String> {
        let fd = self.0.as_raw_fd();
        // SAFETY: F_GETFL queries status flags on this live owned descriptor
        // without changing its ownership.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        // SAFETY: after a successful query, F_SETFL updates only status flags on
        // the same live fd; the trusted O_NONBLOCK bit and return are checked.
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } != 0 {
            return Err(format!(
                "setting target-status reader nonblocking: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut bytes = Vec::with_capacity(FRAME_LEN + 1);
        let mut chunk = [0_u8; FRAME_LEN + 1];
        loop {
            // SAFETY: chunk is writable for its full length and fd is owned.
            let count =
                unsafe { libc::read(fd, chunk.as_mut_ptr().cast::<libc::c_void>(), chunk.len()) };
            if count == 0 {
                break;
            }
            if count > 0 {
                bytes.extend_from_slice(&chunk[..count as usize]);
                if bytes.len() > FRAME_LEN {
                    return Err(
                        "target-status stream contains trailing or duplicate bytes".to_string()
                    );
                }
                continue;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if error.kind() == std::io::ErrorKind::WouldBlock {
                return Err("target-status writer remains open after clean relay reap".to_string());
            }
            return Err(format!("reading target-status frame: {error}"));
        }
        parse_frame(&bytes)
    }
}

fn parse_frame(bytes: &[u8]) -> Result<TargetStatus, String> {
    if bytes.len() != FRAME_LEN {
        return Err(format!(
            "target-status frame has length {}, expected {FRAME_LEN}",
            bytes.len()
        ));
    }
    if bytes[..4] != MAGIC {
        return Err("target-status frame has invalid magic".to_string());
    }
    if bytes[4] != VERSION {
        return Err(format!(
            "target-status frame has unsupported version {}",
            bytes[4]
        ));
    }
    match (bytes[5], bytes[6]) {
        (EXITED, code) => Ok(TargetStatus::Exited(code)),
        (SIGNALED, signal) if valid_linux_signal(signal as libc::c_int) => {
            Ok(TargetStatus::Signaled(signal as libc::c_int))
        }
        (SIGNALED, signal) => Err(format!("target-status frame has invalid signal {signal}")),
        (tag, _) => Err(format!("target-status frame has invalid tag {tag}")),
    }
}

#[cfg(test)]
mod tests;
