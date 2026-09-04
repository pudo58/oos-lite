//! Zero-copy, reference-only store snapshots.

use std::io::{Cursor, Read};
use crate::error::{OosLiteError, Result};
use crate::object::ObjectId;

pub const SNAPSHOT_MAGIC: [u8; 4] = *b"OOSS";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotEntry {
    pub name: String,
    pub object_id: ObjectId,
    pub version: u32,
    pub manifest_id: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub label: String,
    pub created_at: u64,
    pub entries: Vec<SnapshotEntry>,
}

impl Snapshot {
    pub fn new(label: String, entries: Vec<SnapshotEntry>) -> Self {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            label,
            created_at,
            entries,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&SNAPSHOT_MAGIC);
        buf.extend_from_slice(&self.created_at.to_le_bytes());

        let label_bytes = self.label.as_bytes();
        buf.extend_from_slice(&(label_bytes.len() as u16).to_le_bytes());
        buf.extend_from_slice(label_bytes);

        buf.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for entry in &self.entries {
            let name_bytes = entry.name.as_bytes();
            buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            buf.extend_from_slice(name_bytes);

            buf.extend_from_slice(entry.object_id.as_bytes());
            buf.extend_from_slice(&entry.version.to_le_bytes());

            let manifest_bytes = entry.manifest_id.as_bytes();
            buf.extend_from_slice(&(manifest_bytes.len() as u16).to_le_bytes());
            buf.extend_from_slice(manifest_bytes);

            buf.extend_from_slice(&entry.size_bytes.to_le_bytes());
        }

        let crc = crc32fast::hash(&buf);
        buf.extend_from_slice(&crc.to_le_bytes());
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 4 + 8 + 2 + 4 + 4 {
            return Err(OosLiteError::Internal("Snapshot bytes too small".to_string()));
        }

        let data_len = bytes.len() - 4;
        let stored_crc = u32::from_le_bytes(bytes[data_len..].try_into().unwrap());
        let actual_crc = crc32fast::hash(&bytes[..data_len]);
        if stored_crc != actual_crc {
            return Err(OosLiteError::ChecksumMismatch {
                chunk_id: "snapshot".to_string(),
                expected: stored_crc,
                actual: actual_crc,
            });
        }

        if &bytes[0..4] != &SNAPSHOT_MAGIC {
            return Err(OosLiteError::Internal("Invalid snapshot magic".to_string()));
        }

        let created_at = u64::from_le_bytes(bytes[4..12].try_into().unwrap());

        let mut cursor = Cursor::new(&bytes[12..data_len]);

        let mut label_len_buf = [0u8; 2];
        cursor.read_exact(&mut label_len_buf)?;
        let label_len = u16::from_le_bytes(label_len_buf) as usize;

        let mut label_buf = vec![0u8; label_len];
        cursor.read_exact(&mut label_buf)?;
        let label = String::from_utf8(label_buf)
            .map_err(|e| OosLiteError::Internal(format!("Invalid UTF-8 snapshot label: {e}")))?;

        let mut num_entries_buf = [0u8; 4];
        cursor.read_exact(&mut num_entries_buf)?;
        let num_entries = u32::from_le_bytes(num_entries_buf) as usize;

        let mut entries = Vec::with_capacity(num_entries);
        for _ in 0..num_entries {
            let mut name_len_buf = [0u8; 2];
            cursor.read_exact(&mut name_len_buf)?;
            let name_len = u16::from_le_bytes(name_len_buf) as usize;

            let mut name_buf = vec![0u8; name_len];
            cursor.read_exact(&mut name_buf)?;
            let name = String::from_utf8(name_buf)
                .map_err(|e| OosLiteError::Internal(format!("Invalid UTF-8 file name: {e}")))?;

            let mut oid_buf = [0u8; 16];
            cursor.read_exact(&mut oid_buf)?;
            let object_id = ObjectId::from_raw(oid_buf);

            let mut version_buf = [0u8; 4];
            cursor.read_exact(&mut version_buf)?;
            let version = u32::from_le_bytes(version_buf);

            let mut mid_len_buf = [0u8; 2];
            cursor.read_exact(&mut mid_len_buf)?;
            let mid_len = u16::from_le_bytes(mid_len_buf) as usize;

            let mut mid_buf = vec![0u8; mid_len];
            cursor.read_exact(&mut mid_buf)?;
            let manifest_id = String::from_utf8(mid_buf)
                .map_err(|e| OosLiteError::Internal(format!("Invalid UTF-8 manifest ID: {e}")))?;

            let mut size_buf = [0u8; 8];
            cursor.read_exact(&mut size_buf)?;
            let size_bytes = u64::from_le_bytes(size_buf);

            entries.push(SnapshotEntry {
                name,
                object_id,
                version,
                manifest_id,
                size_bytes,
            });
        }

        Ok(Self {
            label,
            created_at,
            entries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_encode_decode_roundtrip() {
        let entry1 = SnapshotEntry {
            name: "docs/readme.txt".to_string(),
            object_id: ObjectId::generate(),
            version: 1,
            manifest_id: "manifest_hash_1".to_string(),
            size_bytes: 1024,
        };
        let entry2 = SnapshotEntry {
            name: "photos/img.png".to_string(),
            object_id: ObjectId::generate(),
            version: 3,
            manifest_id: "manifest_hash_2".to_string(),
            size_bytes: 2048576,
        };

        let snap = Snapshot::new("v1.0.0".to_string(), vec![entry1, entry2]);
        let bytes = snap.to_bytes();
        let decoded = Snapshot::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.label, "v1.0.0");
        assert_eq!(decoded.entries.len(), 2);
        assert_eq!(decoded.entries[0].name, "docs/readme.txt");
        assert_eq!(decoded.entries[1].name, "photos/img.png");
        assert_eq!(decoded.entries[1].version, 3);
        assert_eq!(decoded.entries[1].size_bytes, 2048576);
    }

    #[test]
    fn test_snapshot_corrupted_crc() {
        let snap = Snapshot::new("corrupt_test".to_string(), Vec::new());
        let mut bytes = snap.to_bytes();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF; // Flip CRC bit

        assert!(Snapshot::from_bytes(&bytes).is_err());
    }
}
