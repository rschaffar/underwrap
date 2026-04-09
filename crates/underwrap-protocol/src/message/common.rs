use crate::config::{AccessMode, UnlockMode};
use crate::ipc::{WireDecode, WireEncode, WireReader};
use anyhow::{Result, anyhow};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Debug, Clone, Zeroize, ZeroizeOnDrop, PartialEq, Eq)]
pub struct SecretBytes(pub(crate) Vec<u8>);

impl SecretBytes {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn as_utf8(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.0)
    }

    pub fn into_utf8_string(mut self) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(std::mem::take(&mut self.0))
    }
}

impl std::ops::Deref for SecretBytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<[u8]> for SecretBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for SecretBytes {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl From<String> for SecretBytes {
    fn from(value: String) -> Self {
        Self(value.into_bytes())
    }
}

impl From<&str> for SecretBytes {
    fn from(value: &str) -> Self {
        Self(value.as_bytes().to_vec())
    }
}

impl From<&[u8]> for SecretBytes {
    fn from(value: &[u8]) -> Self {
        Self(value.to_vec())
    }
}

impl From<SecretBytes> for Vec<u8> {
    fn from(mut value: SecretBytes) -> Self {
        std::mem::take(&mut value.0)
    }
}

impl PartialEq<&str> for SecretBytes {
    fn eq(&self, other: &&str) -> bool {
        self.0 == other.as_bytes()
    }
}

impl PartialEq<SecretBytes> for &str {
    fn eq(&self, other: &SecretBytes) -> bool {
        self.as_bytes() == other.0.as_slice()
    }
}

impl PartialEq<String> for SecretBytes {
    fn eq(&self, other: &String) -> bool {
        self.0 == other.as_bytes()
    }
}

impl PartialEq<SecretBytes> for String {
    fn eq(&self, other: &SecretBytes) -> bool {
        self.as_bytes() == other.0.as_slice()
    }
}

impl<const N: usize> PartialEq<&[u8; N]> for SecretBytes {
    fn eq(&self, other: &&[u8; N]) -> bool {
        self.0 == other.as_slice()
    }
}

impl<const N: usize> PartialEq<SecretBytes> for &[u8; N] {
    fn eq(&self, other: &SecretBytes) -> bool {
        self.as_slice() == other.0.as_slice()
    }
}

impl WireEncode for SecretBytes {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.0.encode(out)
    }
}

impl WireDecode for SecretBytes {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self(Vec::<u8>::decode(reader)?))
    }
}

impl WireEncode for AccessMode {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::ReadOnly => 0u8.encode(out),
            Self::ReadWrite => 1u8.encode(out),
        }
    }
}

impl WireDecode for AccessMode {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::ReadOnly),
            1 => Ok(Self::ReadWrite),
            other => Err(anyhow!("invalid access mode discriminant: {other}")),
        }
    }
}

impl WireEncode for UnlockMode {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::AlwaysUnlocked => 0u8.encode(out),
            Self::Always => 1u8.encode(out),
            Self::Optional => 2u8.encode(out),
        }
    }
}

impl WireDecode for UnlockMode {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        match u8::decode(reader)? {
            0 => Ok(Self::AlwaysUnlocked),
            1 => Ok(Self::Always),
            2 => Ok(Self::Optional),
            other => Err(anyhow!("invalid unlock mode discriminant: {other}")),
        }
    }
}
