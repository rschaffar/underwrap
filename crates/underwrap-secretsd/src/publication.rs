//! Serialized publication client owned only by secretsd.

use std::io::{Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use underwrap_protocol::libsecret_publication::{
    RESPONSE_LEN, ResponseKind, decode_response, encode_request_header,
};
use underwrap_protocol::resource_blob::LibsecretSection;
use underwrap_secretsd_store::SecretStore;

use crate::section_store::section_from_store;

pub type SharedPublisher = Arc<Mutex<Publisher>>;

pub struct Publisher {
    stream: UnixStream,
    generation: Option<u64>,
}

impl Publisher {
    /// Adopt the exact descriptor inherited across the reviewed secretsd exec
    /// exception. Ownership transfers to `UnixStream`; CLOEXEC is restored
    /// before any publication bytes are handled.
    pub fn adopt(fd: RawFd) -> Result<Self> {
        // SAFETY: --publication-fd names the sole inherited publisher endpoint;
        // this function is called exactly once and assumes RAII ownership.
        let stream = unsafe { UnixStream::from_raw_fd(fd) };
        set_cloexec(stream.as_raw_fd(), true)?;
        Ok(Self {
            stream,
            generation: Some(0),
        })
    }

    #[cfg(test)]
    pub fn from_stream(stream: UnixStream) -> Self {
        set_cloexec(stream.as_raw_fd(), true).unwrap();
        Self {
            stream,
            generation: Some(0),
        }
    }

    pub fn publish_section(&mut self, section: &LibsecretSection) -> Result<()> {
        let bytes = section
            .to_canonical_bytes()
            .context("canonicalizing libsecret publication")?;
        self.publish_bytes(&bytes)
    }

    fn publish_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        let generation = self
            .generation
            .ok_or_else(|| anyhow!("publication generation overflow"))?;
        let header = encode_request_header(generation, bytes.len()).map_err(anyhow::Error::msg)?;
        self.generation = generation.checked_add(1);
        self.stream
            .write_all(&header)
            .context("writing publication header")?;
        self.stream
            .write_all(bytes)
            .context("writing publication body")?;
        self.stream.flush().context("flushing publication")?;
        let mut response = [0; RESPONSE_LEN];
        self.stream
            .read_exact(&mut response)
            .context("reading publication response")?;
        let (kind, response_generation) = decode_response(&response).map_err(anyhow::Error::msg)?;
        if response_generation != generation {
            return Err(anyhow!("publication response generation mismatch"));
        }
        match kind {
            ResponseKind::Ack => Ok(()),
            ResponseKind::Reject => Err(anyhow!(
                "host rejected libsecret publication generation {generation}"
            )),
        }
    }
}

pub fn publish_store(
    publisher: &SharedPublisher,
    store: &SecretStore,
) -> Result<(), zbus::fdo::Error> {
    let section = section_from_store(store);
    let mut publisher = publisher
        .lock()
        .map_err(|_| zbus::fdo::Error::Failed("publication lock poisoned".into()))?;
    publisher.publish_section(&section).map_err(|error| {
        tracing::error!(error = %error, "publishing libsecret generation failed");
        zbus::fdo::Error::Failed(format!("publishing libsecret state failed: {error:#}"))
    })
}

pub fn set_cloexec(fd: RawFd, cloexec: bool) -> Result<()> {
    // SAFETY: fcntl only inspects/updates descriptor flags for the supplied live fd.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let updated = if cloexec {
        flags | libc::FD_CLOEXEC
    } else {
        flags & !libc::FD_CLOEXEC
    };
    // SAFETY: updated preserves all descriptor flags except the intended CLOEXEC bit.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, updated) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

use std::os::fd::AsRawFd;

#[cfg(test)]
mod tests;
