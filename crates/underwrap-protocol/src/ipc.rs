//! Length-prefixed JSON framing for IPC over Unix domain sockets.
//!
//! Wire format: `[4 bytes big-endian length][JSON payload]`
//!
//! Provides both sync (std) and async (tokio) variants so that
//! underwrap-exec can stay synchronous while the daemon uses tokio.

use serde::{de::DeserializeOwned, Serialize};

/// Maximum allowed frame size (64 KiB). Prevents allocation bombs.
pub const MAX_FRAME_SIZE: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// Synchronous (std::os::unix::net)
// ---------------------------------------------------------------------------

/// Write a length-prefixed JSON frame to a std stream.
pub fn write_frame_sync(
    stream: &mut impl std::io::Write,
    msg: &impl Serialize,
) -> anyhow::Result<()> {
    let payload = serde_json::to_vec(msg)?;
    anyhow::ensure!(
        payload.len() <= MAX_FRAME_SIZE,
        "frame too large: {} bytes",
        payload.len()
    );
    stream.write_all(&(payload.len() as u32).to_be_bytes())?;
    stream.write_all(&payload)?;
    stream.flush()?;
    Ok(())
}

/// Read a length-prefixed JSON frame from a std stream.
pub fn read_frame_sync<T: DeserializeOwned>(
    stream: &mut impl std::io::Read,
) -> anyhow::Result<T> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    anyhow::ensure!(len <= MAX_FRAME_SIZE, "frame too large: {len} bytes");
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    let msg: T = serde_json::from_slice(&buf)?;
    Ok(msg)
}

// ---------------------------------------------------------------------------
// Async (tokio)
// ---------------------------------------------------------------------------

/// Write a length-prefixed JSON frame to a tokio async stream.
pub async fn write_frame_async(
    stream: &mut (impl tokio::io::AsyncWriteExt + Unpin),
    msg: &impl Serialize,
) -> anyhow::Result<()> {
    let payload = serde_json::to_vec(msg)?;
    anyhow::ensure!(
        payload.len() <= MAX_FRAME_SIZE,
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

/// Read a length-prefixed JSON frame from a tokio async stream.
pub async fn read_frame_async<T: DeserializeOwned>(
    stream: &mut (impl tokio::io::AsyncReadExt + Unpin),
) -> anyhow::Result<T> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    anyhow::ensure!(len <= MAX_FRAME_SIZE, "frame too large: {len} bytes");
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let msg: T = serde_json::from_slice(&buf)?;
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn roundtrip_sync() {
        let msg = serde_json::json!({"hello": "world", "n": 42});
        let mut buf = Vec::new();
        write_frame_sync(&mut buf, &msg).unwrap();

        let mut cursor = Cursor::new(&buf);
        let decoded: serde_json::Value = read_frame_sync(&mut cursor).unwrap();
        assert_eq!(msg, decoded);
    }

    #[tokio::test]
    async fn roundtrip_async() {
        let msg = serde_json::json!({"async": true});
        let mut buf = Vec::new();
        write_frame_async(&mut buf, &msg).await.unwrap();

        let mut cursor = tokio::io::BufReader::new(&buf[..]);
        let decoded: serde_json::Value = read_frame_async(&mut cursor).await.unwrap();
        assert_eq!(msg, decoded);
    }
}
