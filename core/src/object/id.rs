use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::error::OosLiteError;

/// 128-bit Hybrid Logical Object ID
/// Layout (16 bytes):
/// [0..8] : Timestamp in milliseconds since UNIX_EPOCH (u64 big-endian)
/// [8..16]: Counter/Entropy bits (u64 big-endian)
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId([u8; 16]);

static OBJECT_ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl ObjectId {
    pub const fn from_raw(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub fn generate() -> Self {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let cnt = OBJECT_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let pid = std::process::id() as u64;
        let entropy = (pid << 32) ^ cnt ^ 0x5a5a_a5a5_3c3c_c3c3;

        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&ts.to_be_bytes());
        bytes[8..16].copy_from_slice(&entropy.to_be_bytes());

        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        let mut hex = String::with_capacity(32);
        for b in &self.0 {
            use std::fmt::Write;
            let _ = write!(hex, "{:02x}", b);
        }
        hex
    }
}

impl fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectId({})", self.to_hex())
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl AsRef<[u8]> for ObjectId {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl FromStr for ObjectId {
    type Err = OosLiteError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.len() != 32 {
            return Err(OosLiteError::Internal(format!(
                "Invalid ObjectId length: expected 32 hex chars, got {}",
                s.len()
            )));
        }
        let mut bytes = [0u8; 16];
        for (i, chunk) in s.as_bytes().chunks_exact(2).enumerate() {
            let hex_str = std::str::from_utf8(chunk).map_err(|e| {
                OosLiteError::Internal(format!("Invalid UTF-8 in ObjectId hex: {}", e))
            })?;
            bytes[i] = u8::from_str_radix(hex_str, 16).map_err(|e| {
                OosLiteError::Internal(format!("Invalid hex in ObjectId: {}", e))
            })?;
        }
        Ok(Self(bytes))
    }
}
