use std::io::{Read, Write};
use crate::chunk::ChunkId;
use crate::error::{OosLiteError, Result};

pub const SEGMENT_MAGIC: &[u8; 4] = b"OOSS";
pub const SEGMENT_VERSION: u32 = 2;
pub const SEGMENT_HEADER_SIZE: usize = 32;

pub const RECORD_MAGIC: &[u8; 4] = b"OOSR";
pub const RECORD_HEADER_SIZE: usize = 4 + 32 + 1 + 3 + 4 + 4 + 4 + 4; // 56 bytes

pub const DEFAULT_MAX_SEGMENT_SIZE: u64 = 256 * 1024 * 1024; // 256 MiB

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionCodec {
    None = 0x00,
    Zstd = 0x01,
}

impl CompressionCodec {
    pub fn from_u8(val: u8) -> Result<Self> {
        match val {
            0x00 => Ok(Self::None),
            0x01 => Ok(Self::Zstd),
            other => Err(OosLiteError::CorruptedSegment {
                offset: 0,
                reason: format!("Unknown compression codec: 0x{:02x}", other),
            }),
        }
    }
}

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
        if version != SEGMENT_VERSION && version != 1 {
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

/// Chunk Record Layout in Segment (v2, 56 bytes header):
/// [0..4]   : Record Magic "OOSR"
/// [4..36]  : ChunkId BLAKE3 (32 bytes)
/// [36]     : Compression Codec (1 byte: 0x00 = None, 0x01 = Zstd)
/// [37..40] : Reserved padding (3 bytes, [0, 0, 0])
/// [40..44] : Raw uncompressed data length (4 bytes, u32-le)
/// [44..48] : Stored payload length on disk (4 bytes, u32-le)
/// [48..52] : Stored payload CRC32C checksum (4 bytes, u32-le)
/// [52..56] : Header Checksum = CRC32C([0..52]) (4 bytes, u32-le)
/// [56..56+payload_len]: Physical Data Payload
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordHeader {
    pub chunk_id: ChunkId,
    pub compression_codec: CompressionCodec,
    pub raw_len: u32,
    pub payload_len: u32,
    pub payload_crc: u32,
}

impl RecordHeader {
    pub fn new(
        chunk_id: ChunkId,
        compression_codec: CompressionCodec,
        raw_len: u32,
        stored_payload: &[u8],
    ) -> Self {
        let payload_crc = crc32fast::hash(stored_payload);
        let payload_len = stored_payload.len() as u32;
        Self {
            chunk_id,
            compression_codec,
            raw_len,
            payload_len,
            payload_crc,
        }
    }

    pub fn write_to<W: Write>(&self, w: &mut W) -> Result<()> {
        let mut buf = [0u8; RECORD_HEADER_SIZE];
        buf[0..4].copy_from_slice(RECORD_MAGIC);
        buf[4..36].copy_from_slice(self.chunk_id.as_bytes());
        buf[36] = self.compression_codec as u8;
        buf[37..40].copy_from_slice(&[0, 0, 0]);
        buf[40..44].copy_from_slice(&self.raw_len.to_le_bytes());
        buf[44..48].copy_from_slice(&self.payload_len.to_le_bytes());
        buf[48..52].copy_from_slice(&self.payload_crc.to_le_bytes());

        let header_crc = crc32fast::hash(&buf[0..52]);
        buf[52..56].copy_from_slice(&header_crc.to_le_bytes());

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

        let expected_header_crc = crc32fast::hash(&buf[0..52]);
        let stored_header_crc = u32::from_le_bytes(buf[52..56].try_into().unwrap());
        if expected_header_crc != stored_header_crc {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Record header checksum mismatch (partial write)",
            ));
        }

        let mut id_bytes = [0u8; 32];
        id_bytes.copy_from_slice(&buf[4..36]);
        let chunk_id = ChunkId::from_raw(id_bytes);
        let codec_u8 = buf[36];
        let compression_codec = match codec_u8 {
            0x00 => CompressionCodec::None,
            0x01 => CompressionCodec::Zstd,
            other => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Unsupported compression codec 0x{:02x}", other),
                ))
            }
        };
        let raw_len = u32::from_le_bytes(buf[40..44].try_into().unwrap());
        let payload_len = u32::from_le_bytes(buf[44..48].try_into().unwrap());
        let payload_crc = u32::from_le_bytes(buf[48..52].try_into().unwrap());

        Ok(Some(Self {
            chunk_id,
            compression_codec,
            raw_len,
            payload_len,
            payload_crc,
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLocation {
    pub segment_id: u64,
    pub record_offset: u64,
    pub payload_offset: u64,
    pub payload_len: u32,
    pub raw_len: u32,
}
