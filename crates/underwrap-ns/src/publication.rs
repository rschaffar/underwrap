//! Host-owned libsecret publication receiver and invocation socket capability.

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use underwrap_protocol::libsecret_publication::{
    MAX_BODY_BYTES, REQUEST_HEADER_LEN, RESPONSE_LEN, ResponseKind, decode_request_header,
    encode_response,
};
use underwrap_protocol::resource_blob::{LibsecretSection, ResourceBlob};

pub struct PublicationPair {
    pub host: Receiver,
    pub publisher: UnixStream,
}

impl PublicationPair {
    pub fn new() -> Result<Self, String> {
        let mut fds = [-1; 2];
        // SAFETY: fds points to two writable integers. On success socketpair
        // returns two fresh AF_UNIX stream descriptors with CLOEXEC atomically;
        // each descriptor is adopted exactly once immediately below.
        if unsafe {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
                0,
                fds.as_mut_ptr(),
            )
        } != 0
        {
            return Err(format!(
                "creating CLOEXEC publication socketpair: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: successful socketpair initialized two independently owned
        // descriptors, each adopted exactly once by OwnedFd RAII.
        let (host, publisher) =
            unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        let host = relocate_reserved(host)?;
        let publisher = relocate_reserved(publisher)?;
        let host = UnixStream::from(host);
        let publisher = UnixStream::from(publisher);
        require_cloexec(&host)?;
        require_cloexec(&publisher)?;
        Ok(Self {
            host: Receiver::new(host),
            publisher,
        })
    }
}

#[derive(Clone)]
pub struct ValidatedGeneration {
    pub generation: u64,
    pub section: LibsecretSection,
}

struct PendingResponse {
    bytes: [u8; RESPONSE_LEN],
    offset: usize,
    candidate: Option<ValidatedGeneration>,
}

pub struct Receiver {
    stream: UnixStream,
    input: Vec<u8>,
    expected: Option<u64>,
    latest: Option<ValidatedGeneration>,
    pending: Option<PendingResponse>,
    poisoned: bool,
    eof: bool,
    /// Immutable Checkout env/files baseline used to check the complete
    /// candidate before ACK. Publication never owns persistence.
    composition_baseline: ResourceBlob,
    #[cfg(test)]
    response_failure_after: Option<usize>,
}

impl Receiver {
    fn new(stream: UnixStream) -> Self {
        Self {
            stream,
            input: Vec::new(),
            expected: Some(0),
            latest: None,
            pending: None,
            poisoned: false,
            eof: false,
            composition_baseline: ResourceBlob::empty(),
            #[cfg(test)]
            response_failure_after: None,
        }
    }

    pub fn set_composition_baseline(&mut self, checkout: &ResourceBlob) {
        self.composition_baseline = checkout.clone();
    }

    fn validate_candidate(&self, section: LibsecretSection) -> Result<LibsecretSection, String> {
        let mut complete = self.composition_baseline.clone();
        complete.libsecret = section.clone();
        complete
            .to_canonical_bytes()
            .map_err(|error| error.to_string())?;
        Ok(section)
    }

    pub fn receive_initial(&mut self) -> Result<(), String> {
        self.stream
            .set_nonblocking(false)
            .map_err(|e| e.to_string())?;
        let mut header = [0; REQUEST_HEADER_LEN];
        if let Err(error) = self.stream.read_exact(&mut header) {
            self.poison(format!("reading initial publication header: {error}"));
            return Err("initial publication framing failed".into());
        }
        let header = match decode_request_header(&header) {
            Ok(header) if header.generation == 0 => header,
            Ok(_) => {
                self.poison("initial publication generation was not zero".into());
                return Err("initial publication generation was not zero".into());
            }
            Err(error) => {
                self.poison(error.clone());
                return Err(error);
            }
        };
        let mut body = vec![0; header.body_len];
        if let Err(error) = self.stream.read_exact(&mut body) {
            self.poison(format!("reading initial publication body: {error}"));
            return Err("initial publication body failed".into());
        }
        match LibsecretSection::from_strict_publication_bytes(&body)
            .map_err(|error| error.to_string())
            .and_then(|section| self.validate_candidate(section))
        {
            Ok(section) => {
                if let Err(error) = self
                    .stream
                    .write_all(&encode_response(ResponseKind::Ack, 0))
                {
                    self.poison(format!("writing ACK(0): {error}"));
                    return Err("writing complete ACK(0) failed".into());
                }
                self.latest = Some(ValidatedGeneration {
                    generation: 0,
                    section,
                });
                self.expected = Some(1);
                Ok(())
            }
            Err(error) => {
                if let Err(write_error) = self
                    .stream
                    .write_all(&encode_response(ResponseKind::Reject, 0))
                {
                    self.poison(format!("writing REJECT(0): {write_error}"));
                    return Err("writing complete REJECT(0) failed".into());
                }
                self.expected = Some(1);
                Err(format!(
                    "rejecting initial libsecret publication: {error:#}"
                ))
            }
        }
    }

    pub fn set_nonblocking(&self) -> Result<(), String> {
        self.stream
            .set_nonblocking(true)
            .map_err(|e| format!("setting publication receiver nonblocking: {e}"))
    }

    /// Drain every currently available byte and flush every possible response.
    /// Returns true only when the final read observed WouldBlock.
    pub fn drain_available(&mut self) -> Result<bool, String> {
        if self.eof {
            return Ok(false);
        }
        let mut would_block = false;
        loop {
            self.flush_pending()?;
            let mut buf = [0_u8; 16 * 1024];
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    self.eof = true;
                    break;
                }
                Ok(n) => {
                    if !self.poisoned {
                        if self.input.len().saturating_add(n) > REQUEST_HEADER_LEN + MAX_BODY_BYTES
                        {
                            self.poison("publication input exceeded one bounded frame".into());
                        } else {
                            self.input.extend_from_slice(&buf[..n]);
                            self.parse_complete_frames()?;
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    would_block = true;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    self.poison(format!("reading publication: {error}"));
                    break;
                }
            }
        }
        self.flush_pending()?;
        Ok(would_block)
    }

    fn parse_complete_frames(&mut self) -> Result<(), String> {
        while self.pending.is_none() && !self.poisoned {
            if self.input.len() < REQUEST_HEADER_LEN {
                return Ok(());
            }
            let fixed: [u8; REQUEST_HEADER_LEN] =
                self.input[..REQUEST_HEADER_LEN].try_into().unwrap();
            let header = match decode_request_header(&fixed) {
                Ok(header) => header,
                Err(error) => {
                    self.poison(error);
                    return Ok(());
                }
            };
            if Some(header.generation) != self.expected {
                self.poison("publication generation replay, gap, or overflow".into());
                return Ok(());
            }
            let frame_len = REQUEST_HEADER_LEN + header.body_len;
            if self.input.len() < frame_len {
                return Ok(());
            }
            let body = self.input[REQUEST_HEADER_LEN..frame_len].to_vec();
            self.input.drain(..frame_len);
            let (kind, candidate) = match LibsecretSection::from_strict_publication_bytes(&body)
                .map_err(|error| error.to_string())
                .and_then(|section| self.validate_candidate(section))
            {
                Ok(section) => (
                    ResponseKind::Ack,
                    Some(ValidatedGeneration {
                        generation: header.generation,
                        section,
                    }),
                ),
                Err(error) => {
                    eprintln!(
                        "underwrap-ns: rejecting libsecret publication generation {}: {error:#}",
                        header.generation
                    );
                    (ResponseKind::Reject, None)
                }
            };
            self.pending = Some(PendingResponse {
                bytes: encode_response(kind, header.generation),
                offset: 0,
                candidate,
            });
            self.flush_pending()?;
        }
        Ok(())
    }

    fn flush_pending(&mut self) -> Result<(), String> {
        while let Some(pending) = &mut self.pending {
            #[cfg(test)]
            if self.response_failure_after == Some(pending.offset) {
                self.poison("injected publication response write failure".into());
                return Ok(());
            }
            #[cfg(test)]
            let response_end = self
                .response_failure_after
                .unwrap_or(RESPONSE_LEN)
                .min(RESPONSE_LEN);
            #[cfg(not(test))]
            let response_end = RESPONSE_LEN;
            match self
                .stream
                .write(&pending.bytes[pending.offset..response_end])
            {
                Ok(0) => {
                    self.poison("zero-byte publication response write".into());
                    return Ok(());
                }
                Ok(n) => pending.offset += n,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    self.poison(format!("writing publication response: {error}"));
                    return Ok(());
                }
            }
            if pending.offset == RESPONSE_LEN {
                let completed = self.pending.take().unwrap();
                if let Some(candidate) = completed.candidate {
                    self.latest = Some(candidate);
                }
                let generation = self.expected.unwrap();
                self.expected = generation.checked_add(1);
                if self.expected.is_none() {
                    self.poison("publication generation overflow".into());
                }
                self.parse_complete_frames()?;
            }
        }
        Ok(())
    }

    fn poison(&mut self, reason: String) {
        if !self.poisoned {
            eprintln!("underwrap-ns: publication stream poisoned: {reason}");
        }
        self.poisoned = true;
        self.pending = None;
        self.input.clear();
        let _ = self.stream.shutdown(std::net::Shutdown::Write);
    }

    pub fn terminal_generation(
        &self,
        final_would_block: bool,
    ) -> Result<ValidatedGeneration, String> {
        if final_would_block || !self.eof {
            return Err("publication endpoint remained open after relay reap".into());
        }
        self.latest
            .clone()
            .ok_or_else(|| "no ACK-written libsecret generation".into())
    }
}

pub fn wait_for_relay_with_publication(
    pid: libc::pid_t,
    receiver: &mut Receiver,
) -> Result<libc::c_int, String> {
    receiver.set_nonblocking()?;
    loop {
        receiver.drain_available()?;
        let mut status = 0;
        // SAFETY: status is writable and pid names the direct namespace relay.
        let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if result == pid {
            return Ok(status);
        }
        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("waitpid failed: {error}"));
        }
        if result != 0 {
            return Err(format!("waitpid returned unexpected pid {result}"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

const RESERVED_HANDOFF_FDS: [i32; 3] = [3, 20, 21];
const FIRST_PRIVATE_FD: i32 = 22;

fn relocate_reserved(fd: OwnedFd) -> Result<OwnedFd, String> {
    if !RESERVED_HANDOFF_FDS.contains(&fd.as_raw_fd()) {
        return Ok(fd);
    }
    // SAFETY: F_DUPFD_CLOEXEC duplicates the live owned descriptor at or above
    // FIRST_PRIVATE_FD without consuming it. On success the fresh descriptor is
    // adopted exactly once; on failure `fd` remains RAII-owned and closes.
    let relocated = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, FIRST_PRIVATE_FD) };
    if relocated < 0 {
        return Err(format!(
            "relocating publication endpoint away from reserved fd {}: {}",
            fd.as_raw_fd(),
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: successful F_DUPFD_CLOEXEC returned one fresh owned descriptor.
    let relocated = unsafe { OwnedFd::from_raw_fd(relocated) };
    drop(fd);
    Ok(relocated)
}

fn require_cloexec(stream: &UnixStream) -> Result<(), String> {
    // SAFETY: F_GETFD inspects flags on a live owned descriptor.
    let flags = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    if flags & libc::FD_CLOEXEC == 0 {
        return Err("publication socketpair was not CLOEXEC".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
