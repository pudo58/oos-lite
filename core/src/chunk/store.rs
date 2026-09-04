use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tracing::debug;

use super::id::ChunkId;
use crate::error::{OosLiteError, Result};

const CHUNK_MAGIC: &[u8; 4] = b"OOSC";
const CHUNK_HEADER_SIZE: usize = 4 + 4 + 4; // magic(4) + crc32c(4) + len(4)

/// Physical Chunk on disk layout:
/// [0..4]   : Magic "OOSC"
/// [4..8]   : CRC32C checksum (u32 little-endian)
/// [8..12]  : Data length (u32 little-endian)
/// [12..12+N]: Raw chunk payload
#[derive(Debug, Clone)]
pub struct Chunk {
    pub id: ChunkId,
    pub checksum: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ChunkStore {
    root_dir: PathBuf,
}

impl ChunkStore {
    pub fn new<P: AsRef<Path>>(root_dir: P) -> Result<Self> {
        let root_dir = root_dir.as_ref().to_path_buf();
        fs::create_dir_all(&root_dir)?;
        Ok(Self { root_dir })
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    fn chunk_path(&self, id: &ChunkId) -> PathBuf {
        let hex = id.to_hex();
        let prefix = &hex[0..2];
        self.root_dir.join(prefix).join(format!("{}.chunk", hex))
    }

    pub fn has_chunk(&self, id: &ChunkId) -> bool {
        self.chunk_path(id).exists()
    }

    /// Stores a chunk on disk.
    /// If a chunk with the same ChunkID already exists, it is NOT rewritten (immutable + dedup).
    /// Returns (ChunkId, is_new: bool).
    pub fn put_chunk(&self, data: &[u8]) -> Result<(ChunkId, bool)> {
        let id = ChunkId::from_data(data);
        let path = self.chunk_path(&id);

        if path.exists() {
            debug!(chunk_id = %id, "Chunk already exists, deduplicated");
            return Ok((id, false));
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let checksum = crc32fast::hash(data);
        let data_len = u32::try_from(data.len()).map_err(|_| {
            OosLiteError::Internal(format!("Chunk data size {} exceeds u32 limit", data.len()))
        })?;

        // Atomic write via temporary file
        let tmp_path = path.with_extension(format!("tmp.{}", std::process::id()));
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp_path)?;

            file.write_all(CHUNK_MAGIC)?;
            file.write_all(&checksum.to_le_bytes())?;
            file.write_all(&data_len.to_le_bytes())?;
            file.write_all(data)?;
            file.sync_all()?;
        }

        // Rename atomic replace
        fs::rename(&tmp_path, &path)?;
        debug!(chunk_id = %id, size = data.len(), "Persisted new chunk");
        Ok((id, true))
    }

    /// Retrieves a chunk from disk, verifying both CRC32C and BLAKE3 integrity.
    pub fn get_chunk(&self, id: &ChunkId) -> Result<Vec<u8>> {
        let path = self.chunk_path(id);
        if !path.exists() {
            return Err(OosLiteError::ChunkNotFound(id.to_string()));
        }

        let mut file = File::open(&path)?;
        let mut header = [0u8; CHUNK_HEADER_SIZE];
        file.read_exact(&mut header)?;

        if &header[0..4] != CHUNK_MAGIC {
            return Err(OosLiteError::CorruptedSegment {
                offset: 0,
                reason: format!("Invalid magic header for chunk {}", id),
            });
        }

        let stored_checksum = u32::from_le_bytes(
            header[4..8]
                .try_into()
                .map_err(|e| OosLiteError::Internal(format!("Checksum slice parse error: {}", e)))?,
        );
        let data_len = u32::from_le_bytes(
            header[8..12]
                .try_into()
                .map_err(|e| OosLiteError::Internal(format!("Data len slice parse error: {}", e)))?,
        ) as usize;

        let mut data = vec![0u8; data_len];
        file.read_exact(&mut data)?;

        // Verify CRC32C
        let actual_checksum = crc32fast::hash(&data);
        if stored_checksum != actual_checksum {
            return Err(OosLiteError::ChecksumMismatch {
                chunk_id: id.to_string(),
                expected: stored_checksum,
                actual: actual_checksum,
            });
        }

        // Verify BLAKE3 (Content-addressed identity)
        let actual_id = ChunkId::from_data(&data);
        if &actual_id != id {
            return Err(OosLiteError::Internal(format!(
                "Content-addressed hash mismatch for chunk {}: actual hash {}",
                id, actual_id
            )));
        }

        Ok(data)
    }

    pub fn delete_chunk(&self, id: &ChunkId) -> Result<bool> {
        let path = self.chunk_path(id);
        if path.exists() {
            fs::remove_file(path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Returns the total number of physical chunks stored.
    pub fn count_chunks(&self) -> Result<usize> {
        let mut count = 0;
        if !self.root_dir.exists() {
            return Ok(0);
        }
        for entry in fs::read_dir(&self.root_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                for sub_entry in fs::read_dir(entry.path())? {
                    let sub_entry = sub_entry?;
                    if sub_entry.path().extension().and_then(|s| s.to_str()) == Some("chunk") {
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
    }
}
