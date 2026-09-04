//! Ordered chunk lists, file manifest records, and version metadata.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use crate::chunk::ChunkId;
use crate::error::{OosLiteError, Result};

pub const MANIFEST_MAGIC: &[u8; 4] = b"OOSM";
pub const MANIFEST_VERSION: u32 = 1;
pub const MANIFEST_HEADER_SIZE: usize = 4 + 4 + 8 + 32 + 8 + 4; // 60 bytes

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub total_size: u64,
    pub content_hash: [u8; 32],
    pub created_at: u64,
    pub chunks: Vec<ChunkId>,
}

impl Manifest {
    pub fn new(chunks: Vec<ChunkId>, total_size: u64, content_hash: [u8; 32]) -> Self {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            total_size,
            content_hash,
            created_at,
            chunks,
        }
    }

    pub fn content_id(&self) -> String {
        let mut hex = String::with_capacity(64);
        for byte in &self.content_hash {
            use std::fmt::Write;
            let _ = write!(hex, "{:02x}", byte);
        }
        hex
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let num_chunks = self.chunks.len() as u32;
        let mut buf = Vec::with_capacity(MANIFEST_HEADER_SIZE + (num_chunks as usize * 32) + 4);

        buf.extend_from_slice(MANIFEST_MAGIC);
        buf.extend_from_slice(&MANIFEST_VERSION.to_le_bytes());
        buf.extend_from_slice(&self.total_size.to_le_bytes());
        buf.extend_from_slice(&self.content_hash);
        buf.extend_from_slice(&self.created_at.to_le_bytes());
        buf.extend_from_slice(&num_chunks.to_le_bytes());

        for chunk_id in &self.chunks {
            buf.extend_from_slice(chunk_id.as_bytes());
        }

        // Add CRC32C over the whole manifest data
        let crc = crc32fast::hash(&buf);
        buf.extend_from_slice(&crc.to_le_bytes());
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < MANIFEST_HEADER_SIZE + 4 {
            return Err(OosLiteError::Internal("Manifest bytes too small".to_string()));
        }

        let stored_crc = u32::from_le_bytes(
            bytes[bytes.len() - 4..]
                .try_into()
                .map_err(|e| OosLiteError::Internal(format!("{}", e)))?,
        );
        let actual_crc = crc32fast::hash(&bytes[..bytes.len() - 4]);
        if stored_crc != actual_crc {
            return Err(OosLiteError::ChecksumMismatch {
                chunk_id: "manifest".to_string(),
                expected: stored_crc,
                actual: actual_crc,
            });
        }

        if &bytes[0..4] != MANIFEST_MAGIC {
            return Err(OosLiteError::Internal("Invalid manifest magic".to_string()));
        }

        let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if version != MANIFEST_VERSION {
            return Err(OosLiteError::Internal(format!("Unsupported manifest version: {}", version)));
        }

        let total_size = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let mut content_hash = [0u8; 32];
        content_hash.copy_from_slice(&bytes[16..48]);
        let created_at = u64::from_le_bytes(bytes[48..56].try_into().unwrap());
        let num_chunks = u32::from_le_bytes(bytes[56..60].try_into().unwrap()) as usize;

        let expected_len = MANIFEST_HEADER_SIZE + (num_chunks * 32) + 4;
        if bytes.len() != expected_len {
            return Err(OosLiteError::Internal(format!(
                "Manifest length mismatch: expected {}, got {}",
                expected_len,
                bytes.len()
            )));
        }

        let mut chunks = Vec::with_capacity(num_chunks);
        let mut offset = MANIFEST_HEADER_SIZE;
        for _ in 0..num_chunks {
            let mut id_bytes = [0u8; 32];
            id_bytes.copy_from_slice(&bytes[offset..offset + 32]);
            chunks.push(ChunkId::from_raw(id_bytes));
            offset += 32;
        }

        Ok(Self {
            total_size,
            content_hash,
            created_at,
            chunks,
        })
    }
}

pub struct ManifestStore {
    root_dir: PathBuf,
}

impl ManifestStore {
    pub fn new<P: AsRef<Path>>(root_dir: P) -> Result<Self> {
        let root_dir = root_dir.as_ref().to_path_buf();
        fs::create_dir_all(&root_dir)?;
        Ok(Self { root_dir })
    }

    fn manifest_path(&self, id: &str) -> PathBuf {
        self.root_dir.join(format!("{}.manifest", id))
    }

    pub fn save(&self, manifest: &Manifest) -> Result<String> {
        let id = manifest.content_id();
        let path = self.manifest_path(&id);
        let tmp_path = path.with_extension(format!("tmp.{}", std::process::id()));

        let bytes = manifest.to_bytes();
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp_path)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }

        fs::rename(&tmp_path, &path)?;
        Ok(id)
    }

    pub fn load(&self, id: &str) -> Result<Manifest> {
        let path = self.manifest_path(id);
        if !path.exists() {
            return Err(OosLiteError::ObjectNotFound(id.to_string()));
        }

        let mut file = File::open(&path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;

        Manifest::from_bytes(&bytes)
    }
}
