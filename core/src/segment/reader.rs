use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::chunk::ChunkId;
use crate::error::{OosLiteError, Result};
use super::format::{ChunkLocation, RECORD_HEADER_SIZE};
use super::writer::segment_file_name;

pub struct SegmentReader {
    segments_dir: PathBuf,
}

impl SegmentReader {
    pub fn new<P: AsRef<Path>>(segments_dir: P) -> Self {
        Self {
            segments_dir: segments_dir.as_ref().to_path_buf(),
        }
    }

    /// Random reads a chunk from segment file, verifying CRC32C and BLAKE3 hash.
    pub fn read_chunk(&self, chunk_id: &ChunkId, location: &ChunkLocation) -> Result<Vec<u8>> {
        let seg_path = self.segments_dir.join(segment_file_name(location.segment_id));
        let mut file = File::open(&seg_path).map_err(|e| {
            OosLiteError::Internal(format!(
                "Failed to open segment file {}: {}",
                seg_path.display(),
                e
            ))
        })?;

        // Read Record Header
        file.seek(SeekFrom::Start(location.record_offset))?;
        let mut header_buf = [0u8; RECORD_HEADER_SIZE];
        file.read_exact(&mut header_buf)?;

        let stored_crc = u32::from_le_bytes(header_buf[36..40].try_into().unwrap());
        let stored_len = u32::from_le_bytes(header_buf[40..44].try_into().unwrap()) as usize;

        if stored_len != location.payload_len as usize {
            return Err(OosLiteError::CorruptedSegment {
                offset: location.record_offset,
                reason: format!(
                    "Record payload length mismatch: index {}, header {}",
                    location.payload_len, stored_len
                ),
            });
        }

        // Read Payload
        let mut data = vec![0u8; stored_len];
        file.read_exact(&mut data)?;

        // Verify CRC32C
        let actual_crc = crc32fast::hash(&data);
        if stored_crc != actual_crc {
            return Err(OosLiteError::ChecksumMismatch {
                chunk_id: chunk_id.to_string(),
                expected: stored_crc,
                actual: actual_crc,
            });
        }

        // Verify BLAKE3 Content Identity
        let actual_id = ChunkId::from_data(&data);
        if &actual_id != chunk_id {
            return Err(OosLiteError::Internal(format!(
                "Content-addressed hash mismatch for chunk {}: actual is {}",
                chunk_id, actual_id
            )));
        }

        Ok(data)
    }
}
