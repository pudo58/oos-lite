use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use tracing::info;

use crate::chunk::ChunkId;
use crate::crypto::VaultKey;
use crate::error::Result;
use crate::index::MetadataStore;
use crate::segment::format::{
    EncryptionScheme, RecordHeader, SegmentHeader, RECORD_HEADER_SIZE_V2, SEGMENT_HEADER_SIZE,
};
use crate::segment::SegmentStore;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FsckReport {
    pub objects_checked: usize,
    pub manifests_checked: usize,
    pub chunks_checked: usize,
    pub segments_checked: usize,
    pub corrupted_chunks: usize,
    pub missing_chunks: usize,
    pub is_healthy: bool,
    pub errors: Vec<String>,
}

pub struct FsckRunner;

impl FsckRunner {
    /// Scans all segment files, records, manifests, snapshots, and object records
    /// verifying CRC32C, BLAKE3 content-hashes, and reference integrity.
    pub fn check(
        segments_dir: &Path,
        segment_store: &SegmentStore,
        metadata_store: &MetadataStore,
    ) -> Result<FsckReport> {
        let mut report = FsckReport::default();
        let mut physical_chunk_ids: HashSet<ChunkId> = HashSet::new();
        let vault_key = segment_store.vault_key().map(|a| a.as_ref());

        // 1. Scan physical segment files on disk
        if segments_dir.exists() {
            let mut seg_paths = Vec::new();
            for entry in fs::read_dir(segments_dir)? {
                let entry = entry?;
                let path = entry.path();
                let file_name = entry.file_name();
                let name_str = file_name.to_string_lossy();
                if name_str.starts_with("segment_") && name_str.ends_with(".seg") {
                    seg_paths.push(path);
                }
            }
            seg_paths.sort();

            for path in seg_paths {
                report.segments_checked += 1;
                Self::check_segment_file(&path, vault_key, &mut report, &mut physical_chunk_ids)?;
            }
        }

        // 2. Scan Manifests in MetadataStore
        let manifest_ids = metadata_store.list_all_manifest_ids()?;
        for mid in manifest_ids {
            report.manifests_checked += 1;
            if let Some(manifest) = metadata_store.get_manifest(&mid)? {
                for cid in &manifest.chunks {
                    if !segment_store.has_chunk(cid) {
                        report.missing_chunks += 1;
                        report.errors.push(format!(
                            "Manifest {} references missing chunk {}",
                            mid, cid
                        ));
                    } else {
                        // Attempt to read chunk through reader to verify full end-to-end extraction
                        match segment_store.get_chunk(cid) {
                            Ok(data) => {
                                let actual_id = ChunkId::from_data(&data);
                                if actual_id != *cid {
                                    report.corrupted_chunks += 1;
                                    report.errors.push(format!(
                                        "Chunk {} read from SegmentStore failed BLAKE3 verification (actual {})",
                                        cid, actual_id
                                    ));
                                }
                            }
                            Err(e) => {
                                report.corrupted_chunks += 1;
                                report.errors.push(format!(
                                    "Failed to read chunk {} referenced by manifest {}: {}",
                                    cid, mid, e
                                ));
                            }
                        }
                    }
                }
            } else {
                report.errors.push(format!("Manifest {} listed in index could not be retrieved", mid));
            }
        }

        // 3. Scan Named Objects and their version records
        let named_objects = metadata_store.list_named_objects()?;
        for (name, obj_id, record) in named_objects {
            report.objects_checked += 1;
            for v in &record.versions {
                if metadata_store.get_manifest(&v.manifest_id)?.is_none() {
                    report.errors.push(format!(
                        "File '{}' (ObjectId {}) version #{} references missing manifest {}",
                        name, obj_id, v.version, v.manifest_id
                    ));
                }
            }
        }

        // 4. Scan Snapshots
        let snapshots = metadata_store.list_snapshots()?;
        for snap in snapshots {
            for entry in &snap.entries {
                if metadata_store.get_manifest(&entry.manifest_id)?.is_none() {
                    report.errors.push(format!(
                        "Snapshot '{}' entry '{}' references missing manifest {}",
                        snap.label, entry.name, entry.manifest_id
                    ));
                }
            }
        }

        report.is_healthy = report.corrupted_chunks == 0
            && report.missing_chunks == 0
            && report.errors.is_empty();

        info!(
            healthy = report.is_healthy,
            segments = report.segments_checked,
            chunks = report.chunks_checked,
            manifests = report.manifests_checked,
            objects = report.objects_checked,
            corrupted = report.corrupted_chunks,
            missing = report.missing_chunks,
            "FSCK verification finished"
        );

        Ok(report)
    }

    fn check_segment_file(
        path: &Path,
        vault_key: Option<&VaultKey>,
        report: &mut FsckReport,
        physical_chunk_ids: &mut HashSet<ChunkId>,
    ) -> Result<()> {
        let mut file = File::open(path)?;
        let file_len = file.metadata()?.len();

        if file_len < SEGMENT_HEADER_SIZE as u64 {
            report.errors.push(format!(
                "Segment file {} size ({} bytes) is less than segment header size",
                path.display(),
                file_len
            ));
            return Ok(());
        }

        // Read and verify segment header
        if let Err(e) = SegmentHeader::read_from(&mut file) {
            report.errors.push(format!(
                "Segment file {} has invalid segment header: {}",
                path.display(),
                e
            ));
            return Ok(());
        }

        let mut offset = SEGMENT_HEADER_SIZE as u64;
        while offset < file_len {
            if offset + RECORD_HEADER_SIZE_V2 as u64 > file_len {
                report.corrupted_chunks += 1;
                report.errors.push(format!(
                    "Segment file {} truncated record header at offset {}",
                    path.display(),
                    offset
                ));
                break;
            }

            file.seek(SeekFrom::Start(offset))?;
            match RecordHeader::read_from(&mut file) {
                Ok(Some(header)) => {
                    let payload_len = header.payload_len as usize;
                    if offset + header.header_size as u64 + payload_len as u64 > file_len {
                        report.corrupted_chunks += 1;
                        report.errors.push(format!(
                            "Segment file {} payload exceeds EOF at offset {}",
                            path.display(),
                            offset
                        ));
                        break;
                    }

                    let mut payload = vec![0u8; payload_len];
                    if let Err(e) = file.read_exact(&mut payload) {
                        report.corrupted_chunks += 1;
                        report.errors.push(format!(
                            "Segment file {} failed reading payload at offset {}: {}",
                            path.display(),
                            offset,
                            e
                        ));
                        break;
                    }

                    report.chunks_checked += 1;

                    // 1. Physical Layer: Verify CRC32C of stored bytes on disk
                    let actual_crc = crc32fast::hash(&payload);
                    if actual_crc != header.payload_crc {
                        report.corrupted_chunks += 1;
                        report.errors.push(format!(
                            "Segment file {} chunk {} at offset {} has CRC32C mismatch: expected {:08x}, actual {:08x}",
                            path.display(),
                            header.chunk_id,
                            offset,
                            header.payload_crc,
                            actual_crc
                        ));
                    }

                    // 2. Cryptographic Layer: Decrypt if encrypted
                    let decrypted_result = match header.encryption_scheme {
                        EncryptionScheme::None => Ok(payload),
                        EncryptionScheme::XChaCha20Poly1305 => {
                            if let Some(vk) = vault_key {
                                vk.decrypt_chunk(&payload, &header.nonce, &header.aad()).map_err(|e| {
                                    format!(
                                        "Segment file {} chunk {} failed decryption/authentication: {}",
                                        path.display(),
                                        header.chunk_id,
                                        e
                                    )
                                })
                            } else {
                                Err(format!(
                                    "Segment file {} chunk {} is encrypted but no vault key provided",
                                    path.display(),
                                    header.chunk_id
                                ))
                            }
                        }
                    };

                    // 3. Compression Layer: Decompress if Zstd, otherwise use raw bytes
                    let decompressed_result = match decrypted_result {
                        Ok(decrypted_payload) => match header.compression_codec {
                            crate::segment::format::CompressionCodec::None => Ok(decrypted_payload),
                            crate::segment::format::CompressionCodec::Zstd => {
                                zstd::decode_all(&decrypted_payload[..]).map_err(|e| {
                                    format!(
                                        "Segment file {} failed to decompress zstd chunk {} at offset {}: {}",
                                        path.display(),
                                        header.chunk_id,
                                        offset,
                                        e
                                    )
                                })
                            }
                        },
                        Err(err_msg) => Err(err_msg),
                    };

                    // 4. Logical Layer: Verify BLAKE3 Content Identity on decompressed bytes
                    match decompressed_result {
                        Ok(raw_data) => {
                            let actual_id = ChunkId::from_data(&raw_data);
                            if actual_id != header.chunk_id {
                                report.corrupted_chunks += 1;
                                report.errors.push(format!(
                                    "Segment file {} at offset {} chunk ID mismatch: expected {}, actual {}",
                                    path.display(),
                                    offset,
                                    header.chunk_id,
                                    actual_id
                                ));
                            }
                        }
                        Err(err_msg) => {
                            report.corrupted_chunks += 1;
                            report.errors.push(err_msg);
                        }
                    }

                    physical_chunk_ids.insert(header.chunk_id);
                    offset += header.header_size as u64 + payload_len as u64;
                }
                Ok(None) => break,
                Err(err) => {
                    report.corrupted_chunks += 1;
                    report.errors.push(format!(
                        "Segment file {} corrupted record header at offset {}: {}",
                        path.display(),
                        offset,
                        err
                    ));
                    break;
                }
            }
        }

        Ok(())
    }
}
