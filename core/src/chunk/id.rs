use std::fmt;
use std::str::FromStr;
use crate::error::OosLiteError;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkId([u8; 32]);

impl ChunkId {
    pub const fn from_raw(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_data(data: &[u8]) -> Self {
        let hash = blake3::hash(data);
        Self(*hash.as_bytes())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        let mut hex = String::with_capacity(64);
        for byte in &self.0 {
            use std::fmt::Write;
            let _ = write!(hex, "{:02x}", byte);
        }
        hex
    }
}

impl fmt::Debug for ChunkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ChunkId({})", self.to_hex())
    }
}

impl fmt::Display for ChunkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl AsRef<[u8]> for ChunkId {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl FromStr for ChunkId {
    type Err = OosLiteError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 64 {
            return Err(OosLiteError::Internal(format!(
                "Invalid ChunkId length: expected 64 hex characters, got {}",
                s.len()
            )));
        }
        let mut bytes = [0u8; 32];
        for (i, chunk) in s.as_bytes().chunks_exact(2).enumerate() {
            let hex_str = std::str::from_utf8(chunk).map_err(|e| {
                OosLiteError::Internal(format!("Invalid UTF-8 in ChunkId hex: {}", e))
            })?;
            bytes[i] = u8::from_str_radix(hex_str, 16).map_err(|e| {
                OosLiteError::Internal(format!("Invalid hex in ChunkId: {}", e))
            })?;
        }
        Ok(Self(bytes))
    }
}
