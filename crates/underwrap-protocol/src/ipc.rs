//! Length-prefixed binary framing for IPC over Unix domain sockets.
//!
//! Wire format: `[4 bytes big-endian length][binary payload]`
//!
//! Message payloads use a small custom binary codec implemented via the
//! [`WireEncode`] and [`WireDecode`] traits. The framing stays intentionally
//! simple so synchronous wrapper code and async daemon code can share the same
//! protocol.

use anyhow::{Context, Result, anyhow, ensure};
use std::collections::HashMap;

/// Maximum allowed frame size (64 KiB). Prevents allocation bombs.
pub const MAX_FRAME_SIZE: usize = 64 * 1024;
/// Payload capacity of the authenticated wrapper-resource channel: a complete
/// resource plus the largest (`CheckoutOk`) six-byte message envelope. The
/// four-byte outer frame prefix is separate.
pub const MAX_WRAPPER_RESOURCE_FRAME_SIZE: usize =
    crate::resource_blob::MAX_CANONICAL_RESOURCE_BYTES + 6;

pub trait WireEncode {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()>;
}

pub trait WireDecode: Sized {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self>;
}

pub struct WireReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> WireReader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn finish(&self) -> Result<()> {
        ensure!(self.pos == self.buf.len(), "trailing bytes in payload");
        Ok(())
    }

    pub fn read_exact(&mut self, len: usize) -> Result<&'a [u8]> {
        ensure!(
            self.pos + len <= self.buf.len(),
            "truncated payload: need {} bytes, have {} remaining",
            len,
            self.buf.len().saturating_sub(self.pos)
        );
        let out = &self.buf[self.pos..self.pos + len];
        self.pos += len;
        Ok(out)
    }

    pub fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_exact(1)?[0])
    }

    pub fn read_bool(&mut self) -> Result<bool> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(anyhow!("invalid bool discriminant: {other}")),
        }
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_be_bytes(
            bytes.try_into().expect("length checked"),
        ))
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub fn read_u64(&mut self) -> Result<u64> {
        let bytes = self.read_exact(8)?;
        Ok(u64::from_be_bytes(
            bytes.try_into().expect("length checked"),
        ))
    }

    pub fn read_bytes(&mut self) -> Result<Vec<u8>> {
        let len = self.read_u32()? as usize;
        Ok(self.read_exact(len)?.to_vec())
    }

    /// Read one canonical resource blob after enforcing its inclusive byte cap.
    ///
    /// The length check precedes both reading and copying the blob body so a
    /// wrapper-resource payload cannot turn an over-limit inner vector length
    /// into a truncation error or allocation/copy attempt.
    pub fn read_canonical_resource_bytes(&mut self) -> Result<Vec<u8>> {
        let len = self.read_u32()? as usize;
        crate::resource_blob::ensure_resource_size(len)?;
        Ok(self.read_exact(len)?.to_vec())
    }

    pub fn read_string(&mut self) -> Result<String> {
        let bytes = self.read_bytes()?;
        String::from_utf8(bytes).context("invalid UTF-8 string in payload")
    }
}

pub fn encode_to_vec<T: WireEncode>(value: &T) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    value.encode(&mut out)?;
    Ok(out)
}

pub fn decode_from_slice<T: WireDecode>(buf: &[u8]) -> Result<T> {
    let mut reader = WireReader::new(buf);
    let value = T::decode(&mut reader)?;
    reader.finish()?;
    Ok(value)
}

impl WireEncode for u8 {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        out.push(*self);
        Ok(())
    }
}

impl WireDecode for u8 {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        reader.read_u8()
    }
}

impl WireEncode for bool {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        out.push(u8::from(*self));
        Ok(())
    }
}

impl WireDecode for bool {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        reader.read_bool()
    }
}

impl WireEncode for u32 {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        out.extend_from_slice(&self.to_be_bytes());
        Ok(())
    }
}

impl WireDecode for u32 {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        reader.read_u32()
    }
}

impl WireEncode for u64 {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        out.extend_from_slice(&self.to_be_bytes());
        Ok(())
    }
}

impl WireDecode for u64 {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        reader.read_u64()
    }
}

impl WireEncode for String {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        let bytes = self.as_bytes();
        ensure!(bytes.len() <= u32::MAX as usize, "string too large");
        (bytes.len() as u32).encode(out)?;
        out.extend_from_slice(bytes);
        Ok(())
    }
}

impl WireEncode for &str {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        let bytes = self.as_bytes();
        ensure!(bytes.len() <= u32::MAX as usize, "string too large");
        (bytes.len() as u32).encode(out)?;
        out.extend_from_slice(bytes);
        Ok(())
    }
}

impl WireDecode for String {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        reader.read_string()
    }
}

impl WireEncode for std::path::PathBuf {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        // Paths in this system are NixOS store / config paths and are
        // UTF-8; encode as the string form to reuse the length-prefixed
        // string framing.
        let s = self
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("path is not valid UTF-8: {}", self.display()))?;
        s.encode(out)
    }
}

impl WireDecode for std::path::PathBuf {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(std::path::PathBuf::from(reader.read_string()?))
    }
}

impl<T: WireEncode> WireEncode for Option<T> {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Some(value) => {
                true.encode(out)?;
                value.encode(out)
            }
            None => false.encode(out),
        }
    }
}

impl<T: WireDecode> WireDecode for Option<T> {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        if bool::decode(reader)? {
            Ok(Some(T::decode(reader)?))
        } else {
            Ok(None)
        }
    }
}

impl<T: WireEncode> WireEncode for Vec<T> {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        ensure!(self.len() <= u32::MAX as usize, "vector too large");
        (self.len() as u32).encode(out)?;
        for item in self {
            item.encode(out)?;
        }
        Ok(())
    }
}

impl<T: WireDecode> WireDecode for Vec<T> {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        let len = reader.read_u32()? as usize;
        let mut out = Vec::with_capacity(len.min(reader.remaining()));
        for _ in 0..len {
            out.push(T::decode(reader)?);
        }
        Ok(out)
    }
}

impl<T: WireEncode> WireEncode for HashMap<String, T> {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        ensure!(self.len() <= u32::MAX as usize, "map too large");
        (self.len() as u32).encode(out)?;
        let mut entries: Vec<_> = self.iter().collect();
        entries.sort_by_key(|(key, _)| *key);
        for (key, value) in entries {
            key.encode(out)?;
            value.encode(out)?;
        }
        Ok(())
    }
}

impl<T: WireDecode> WireDecode for HashMap<String, T> {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        let len = reader.read_u32()? as usize;
        let mut out = HashMap::with_capacity(len.min(reader.remaining()));
        for _ in 0..len {
            let key = String::decode(reader)?;
            let value = T::decode(reader)?;
            ensure!(
                out.insert(key, value).is_none(),
                "duplicate map key in payload"
            );
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Synchronous (std::os::unix::net)
// ---------------------------------------------------------------------------

/// Write a length-prefixed binary frame to a std stream.
pub fn write_frame_sync(stream: &mut impl std::io::Write, msg: &impl WireEncode) -> Result<()> {
    write_frame_sync_bounded(stream, msg, MAX_FRAME_SIZE)
}

/// Write a frame for the authenticated wrapper-resource channel only.
pub fn write_wrapper_resource_frame_sync(
    stream: &mut impl std::io::Write,
    msg: &impl WireEncode,
) -> Result<()> {
    write_frame_sync_bounded(stream, msg, MAX_WRAPPER_RESOURCE_FRAME_SIZE)
}

fn write_frame_sync_bounded(
    stream: &mut impl std::io::Write,
    msg: &impl WireEncode,
    max: usize,
) -> Result<()> {
    let payload = encode_to_vec(msg)?;
    ensure!(
        payload.len() <= max,
        "frame too large: {} bytes",
        payload.len()
    );
    stream.write_all(&(payload.len() as u32).to_be_bytes())?;
    stream.write_all(&payload)?;
    stream.flush()?;
    Ok(())
}

/// Read a length-prefixed binary frame from a std stream.
pub fn read_frame_sync<T: WireDecode>(stream: &mut impl std::io::Read) -> Result<T> {
    read_frame_sync_bounded(stream, MAX_FRAME_SIZE)
}

/// Read a frame for the authenticated wrapper-resource channel only.
pub fn read_wrapper_resource_frame_sync<T: WireDecode>(
    stream: &mut impl std::io::Read,
) -> Result<T> {
    read_frame_sync_bounded(stream, MAX_WRAPPER_RESOURCE_FRAME_SIZE)
}

fn read_frame_sync_bounded<T: WireDecode>(
    stream: &mut impl std::io::Read,
    max: usize,
) -> Result<T> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    ensure!(len <= max, "frame too large: {len} bytes");
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    decode_from_slice(&buf)
}

// ---------------------------------------------------------------------------
// Async (tokio)
// ---------------------------------------------------------------------------

/// Write a length-prefixed binary frame to a tokio async stream.
pub async fn write_frame_async(
    stream: &mut (impl tokio::io::AsyncWriteExt + Unpin),
    msg: &impl WireEncode,
) -> Result<()> {
    write_frame_async_bounded(stream, msg, MAX_FRAME_SIZE).await
}

/// Write a frame for the authenticated wrapper-resource channel only.
pub async fn write_wrapper_resource_frame_async(
    stream: &mut (impl tokio::io::AsyncWriteExt + Unpin),
    msg: &impl WireEncode,
) -> Result<()> {
    write_frame_async_bounded(stream, msg, MAX_WRAPPER_RESOURCE_FRAME_SIZE).await
}

async fn write_frame_async_bounded(
    stream: &mut (impl tokio::io::AsyncWriteExt + Unpin),
    msg: &impl WireEncode,
    max: usize,
) -> Result<()> {
    let payload = encode_to_vec(msg)?;
    ensure!(
        payload.len() <= max,
        "frame too large: {} bytes",
        payload.len()
    );
    stream
        .write_all(&(payload.len() as u32).to_be_bytes())
        .await?;
    stream.write_all(&payload).await?;
    stream.flush().await?;
    Ok(())
}

/// Read a length-prefixed binary frame from a tokio async stream.
pub async fn read_frame_async<T: WireDecode>(
    stream: &mut (impl tokio::io::AsyncReadExt + Unpin),
) -> Result<T> {
    read_frame_async_bounded(stream, MAX_FRAME_SIZE).await
}

/// Read a frame for the authenticated wrapper-resource channel only.
pub async fn read_wrapper_resource_frame_async<T: WireDecode>(
    stream: &mut (impl tokio::io::AsyncReadExt + Unpin),
) -> Result<T> {
    read_frame_async_bounded(stream, MAX_WRAPPER_RESOURCE_FRAME_SIZE).await
}

async fn read_frame_async_bounded<T: WireDecode>(
    stream: &mut (impl tokio::io::AsyncReadExt + Unpin),
    max: usize,
) -> Result<T> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    ensure!(len <= max, "frame too large: {len} bytes");
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    decode_from_slice(&buf)
}

#[cfg(test)]
mod tests;
