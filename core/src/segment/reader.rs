use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::chunk::ChunkId;
use crate::crypto::VaultKey;
use crate::error::{OosLiteError, Result};
use super::format::{ChunkLocation, CompressionCodec, EncryptionScheme, RecordHeader};
use super::writer::segment_file_name;

pub const DEFAULT_MAX_OPEN_SEGMENTS: usize = 128;

pub struct SegmentReader {
    segments_dir: PathBuf,
    cache: Mutex<SegmentFdCache>,
    vault_key: Option<Arc<VaultKey>>,
}

struct SegmentFdCache {
    files: HashMap<u64, File>,
    access_order: VecDeque<u64>,
    max_entries: usize,
}

impl SegmentFdCache {
    fn new(max_entries: usize) -> Self {
        Self {
            files: HashMap::new(),
            access_order: VecDeque::new(),
            max_entries,
        }
    }

    fn clear(&mut self) {
        self.files.clear();
        self.access_order.clear();
    }

    fn get_mut_or_open<F>(&mut self, seg_id: u64, open_fn: F) -> Result<&mut File>
    where
        F: FnOnce() -> Result<File>,
    {
        if self.files.contains_key(&seg_id) {
            if let Some(pos) = self.access_order.iter().position(|&id| id == seg_id) {
                self.access_order.remove(pos);
            }
            self.access_order.push_back(seg_id);
            return Ok(self.files.get_mut(&seg_id).unwrap());
        }

        while self.files.len() >= self.max_entries && !self.access_order.is_empty() {
            if let Some(oldest) = self.access_order.pop_front() {
                self.files.remove(&oldest);
            }
        }

        let file = open_fn()?;
        self.files.insert(seg_id, file);
        self.access_order.push_back(seg_id);
        Ok(self.files.get_mut(&seg_id).unwrap())
    }
}

impl SegmentReader {
    pub fn new<P: AsRef<Path>>(segments_dir: P) -> Self {
        Self::with_max_open_segments_and_vault(segments_dir, DEFAULT_MAX_OPEN_SEGMENTS, None)
    }

    pub fn with_vault<P: AsRef<Path>>(segments_dir: P, vault_key: Option<Arc<VaultKey>>) -> Self {
        Self::with_max_open_segments_and_vault(segments_dir, DEFAULT_MAX_OPEN_SEGMENTS, vault_key)
    }

    pub fn with_max_open_segments<P: AsRef<Path>>(segments_dir: P, max_open: usize) -> Self {
        Self::with_max_open_segments_and_vault(segments_dir, max_open, None)
    }

    pub fn with_max_open_segments_and_vault<P: AsRef<Path>>(
        segments_dir: P,
        max_open: usize,
        vault_key: Option<Arc<VaultKey>>,
    ) -> Self {
        Self {
            segments_dir: segments_dir.as_ref().to_path_buf(),
            cache: Mutex::new(SegmentFdCache::new(max_open)),
            vault_key,
        }
    }

    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    /// Random reads a chunk from segment file with multi-layer defense:
    /// 1. Physical Layer: Verifies CRC32C over the on-disk bytes BEFORE decompressing/decrypting.
    /// 2. Cryptographic Layer: Authenticates and decrypts ciphertext via XChaCha20-Poly1305 with AAD.
    /// 3. Compression Layer: Unpacks Zstd if chunk was compressed.
    /// 4. Logical Layer: Verifies BLAKE3 content-addressed identity against the requested ChunkId.
    pub fn read_chunk(&self, chunk_id: &ChunkId, location: &ChunkLocation) -> Result<Vec<u8>> {
        let mut cache = self.cache.lock().map_err(|e| {
            OosLiteError::Internal(format!("SegmentReader cache lock poisoned: {e}"))
        })?;

        let seg_id = location.segment_id;
        let seg_path = self.segments_dir.join(segment_file_name(seg_id));
        let file = cache.get_mut_or_open(seg_id, || {
            File::open(&seg_path).map_err(|e| {
                OosLiteError::Internal(format!(
                    "Failed to open segment file {}: {}",
                    seg_path.display(),
                    e
                ))
            })
        })?;

        // Read Record Header
        file.seek(SeekFrom::Start(location.record_offset))?;
        let header = match RecordHeader::read_from(file)? {
            Some(h) => h,
            None => {
                return Err(OosLiteError::CorruptedSegment {
                    offset: location.record_offset,
                    reason: "Unexpected EOF while reading record header".to_string(),
                });
            }
        };

        if header.payload_len != location.payload_len {
            return Err(OosLiteError::CorruptedSegment {
                offset: location.record_offset,
                reason: format!(
                    "Record payload length mismatch: index {}, header {}",
                    location.payload_len, header.payload_len
                ),
            });
        }

        // Read physical stored payload from disk
        let mut stored_payload = vec![0u8; header.payload_len as usize];
        file.read_exact(&mut stored_payload)?;

        // 1. Physical Layer: Verify CRC32C of stored bytes on disk BEFORE decrypting/decompressing
        let actual_crc = crc32fast::hash(&stored_payload);
        if header.payload_crc != actual_crc {
            return Err(OosLiteError::ChecksumMismatch {
                chunk_id: chunk_id.to_string(),
                expected: header.payload_crc,
                actual: actual_crc,
            });
        }

        // 2. Cryptographic Layer: Authenticate and decrypt if encrypted
        let decompressed_payload = match header.encryption_scheme {
            EncryptionScheme::None => stored_payload,
            EncryptionScheme::XChaCha20Poly1305 => {
                let vk = self.vault_key.as_ref().ok_or_else(|| {
                    OosLiteError::PasswordRequired
                })?;
                vk.decrypt_chunk(&stored_payload, &header.nonce, &header.aad())?
            }
        };

        // 3. Compression Layer: Decompress if Zstd, otherwise use raw bytes
        let raw_bytes = match header.compression_codec {
            CompressionCodec::None => decompressed_payload,
            CompressionCodec::Zstd => {
                zstd::decode_all(&decompressed_payload[..]).map_err(|e| {
                    OosLiteError::CorruptedSegment {
                        offset: location.record_offset,
                        reason: format!("Failed to decompress zstd chunk {}: {}", chunk_id, e),
                    }
                })?
            }
        };

        // 4. Logical Layer: Verify BLAKE3 Content Identity on decompressed raw bytes
        let actual_id = ChunkId::from_data(&raw_bytes);
        if &actual_id != chunk_id {
            return Err(OosLiteError::Internal(format!(
                "Content-addressed hash mismatch for chunk {}: actual is {}",
                chunk_id, actual_id
            )));
        }

        Ok(raw_bytes)
    }

}
