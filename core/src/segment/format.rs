use std::io::{Read, Write};
use crate::chunk::ChunkId;
use crate::error::{OosLiteError, Result};

pub const SEGMENT_MAGIC: &[u8; 4] = b"OOSS";
pub const SEGMENT_VERSION: u32 = 1;
pub const SEGMENT_HEADER_SIZE: usize = 32;

pub const RECORD_MAGIC: &[u8; 4] = b"OOSR";
pub const RECORD_HEADER_SIZE: usize = 4 + 32 + 4 + 4 + 4; // 48 bytes

pub const DEFAULT_MAX_SEGMENT_SIZE: u64 = 256 * 1024 * 1024; // 256 MiB

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentHeader {
    pub segment_id: u64,
    pub created_at: u64,
}

impl SegmentHeader {
    pub fn new(segment_id: u64) -> Self {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self { segment_id, created_at }
    }

    pub fn write_to<W: Write>(&self, w: &mut W) -> Result<()> {
        let mut buf = [0u8; SEGMENT_HEADER_SIZE];
        buf[0..4].copy_from_slice(SEGMENT_MAGIC);
        buf[4..8].copy_from_slice(&SEGMENT_VERSION.to_le_bytes());
        buf[8..16].copy_from_slice(&self.segment_id.to_le_bytes());
        buf[16..24].copy_from_slice(&self.created_at.to_le_bytes());
        // Remaining 8 bytes reserved (all zeros)
        w.write_all(&buf)?;
        Ok(())
    }

    pub fn read_from<R: Read>(r: &mut R) -> Result<Self> {
        let mut buf = [0u8; SEGMENT_HEADER_SIZE];
        r.read_exact(&mut buf)?;
        if &buf[0..4] != SEGMENT_MAGIC {
            return Err(OosLiteError::CorruptedSegment {
                offset: 0,
                reason: "Invalid segment magic header".to_string(),
            });
        }
        let version = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        if version != SEGMENT_VERSION {
            return Err(OosLiteError::CorruptedSegment {
                offset: 4,
                reason: format!("Unsupported segment version: {}", version),
            });
        }
        let segment_id = u64::from_le_bytes(buf[8..16].try_into().unwrap());
        let created_at = u64::from_le_bytes(buf[16..24].try_into().unwrap());
        Ok(Self { segment_id, created_at })
    }
}

/// Chunk Record Layout in Segment:
/// [0..4]   : Record Magic "OOSR"
/// [4..36]  : ChunkId BLAKE3 (32 bytes)
/// [36..40] : CRC32C checksum of data payload (4 bytes, u32-le)
/// [40..44] : Data length (4 bytes, u32-le)
/// [44..48] : Header Checksum = CRC32C([0..44])
/// [48..48+N]: Data Payload
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordHeader {
    pub chunk_id: ChunkId,
    pub payload_crc: u32,
    pub payload_len: u32,
}

impl RecordHeader {
    pub fn new(chunk_id: ChunkId, data: &[u8]) -> Self {
        let payload_crc = crc32fast::hash(data);
        let payload_len = data.len() as u32;
        Self { chunk_id, payload_crc, payload_len }
    }

    pub fn write_to<W: Write>(&self, w: &mut W) -> Result<()> {
        let mut buf = [0u8; RECORD_HEADER_SIZE];
        buf[0..4].copy_from_slice(RECORD_MAGIC);
        buf[4..36].copy_from_slice(self.chunk_id.as_bytes());
        buf[36..40].copy_from_slice(&self.payload_crc.to_le_bytes());
        buf[40..44].copy_from_slice(&self.payload_len.to_le_bytes());

        let header_crc = crc32fast::hash(&buf[0..44]);
        buf[44..48].copy_from_slice(&header_crc.to_le_bytes());

        w.write_all(&buf)?;
        Ok(())
    }

    pub fn read_from<R: Read>(r: &mut R) -> std::io::Result<Option<Self>> {
        let mut buf = [0u8; RECORD_HEADER_SIZE];
        match r.read_exact(&mut buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }

        if &buf[0..4] != RECORD_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid record magic",
            ));
        }

        let expected_header_crc = crc32fast::hash(&buf[0..44]);
        let stored_header_crc = u32::from_le_bytes(buf[44..48].try_into().unwrap());
        if expected_header_crc != stored_header_crc {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Record header checksum mismatch (partial write)",
            ));
        }

        let mut id_bytes = [0u8; 32];
        id_bytes.copy_from_slice(&buf[4..36]);
        let chunk_id = ChunkId::from_raw(id_bytes);
        let payload_crc = u32::from_le_bytes(buf[36..40].try_into().unwrap());
        let payload_len = u32::from_le_bytes(buf[40..44].try_into().unwrap());

        Ok(Some(Self { chunk_id, payload_crc, payload_len }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLocation {
    pub segment_id: u64,
    pub record_offset: u64,
    pub payload_offset: u64,
    pub payload_len: u32,
}
