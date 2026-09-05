use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

use crate::chunk::ChunkId;
use crate::crypto::VaultKey;
use crate::error::{OosLiteError, Result};
use super::format::{
    ChunkLocation, RecordHeader, SegmentHeader, SEGMENT_HEADER_SIZE,
};
use super::index::SegmentIndex;
use super::reader::SegmentReader;
use super::writer::{segment_file_name, SegmentWriter};

pub struct SegmentStore {
    pub segments_dir: PathBuf,
    index: Arc<SegmentIndex>,
    reader: SegmentReader,
    writer: Mutex<SegmentWriter>,
    vault_key: Option<Arc<VaultKey>>,
}

impl SegmentStore {
    pub fn new<P: AsRef<Path>>(dir: P) -> Result<Self> {
        Self::with_max_segment_size_and_vault(dir, 0, None)
    }

    pub fn with_vault<P: AsRef<Path>>(dir: P, vault_key: Option<Arc<VaultKey>>) -> Result<Self> {
        Self::with_max_segment_size_and_vault(dir, 0, vault_key)
    }

    pub fn with_max_segment_size<P: AsRef<Path>>(
        dir: P,
        max_segment_size: u64,
    ) -> Result<Self> {
        Self::with_max_segment_size_and_vault(dir, max_segment_size, None)
    }

    pub fn with_max_segment_size_and_vault<P: AsRef<Path>>(
        dir: P,
        max_segment_size: u64,
        vault_key: Option<Arc<VaultKey>>,
    ) -> Result<Self> {
        let segments_dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&segments_dir)?;

        let index = Arc::new(SegmentIndex::new());
        let (latest_segment_id, resume_offset) = Self::recover_and_index(
            &segments_dir,
            &index,
        )?;

        let writer = SegmentWriter::open(
            &segments_dir,
            latest_segment_id,
            resume_offset,
            max_segment_size,
            vault_key.clone(),
        )?;
        let reader = SegmentReader::with_vault(&segments_dir, vault_key.clone());

        Ok(Self {
            segments_dir,
            index,
            reader,
            writer: Mutex::new(writer),
            vault_key,
        })
    }

    /// Scans all segment files in sequential order, validates records,
    /// populates the index, and repairs any partial/corrupted record at EOF caused by crash/kill.
    fn recover_and_index(
        segments_dir: &Path,
        index: &SegmentIndex,
    ) -> Result<(u64, u64)> {
        // Cleanup leftover staging or restore from .old in case of crash during compaction
        let marker_path = segments_dir.join(".compact_done");
        let compact_done = fs::read(&marker_path)
            .map(|b| b == b"COMPACT_DONE")
            .unwrap_or(false);

        let mut old_seg_files = Vec::new();
        let mut new_seg_files = Vec::new();

        if let Ok(entries) = fs::read_dir(segments_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with(".compact_tmp_") {
                    let _ = fs::remove_dir_all(&path);
                } else if name_str.starts_with("segment_") && name_str.ends_with(".seg") {
                    new_seg_files.push(path);
                } else if name_str.starts_with("segment_") && name_str.ends_with(".seg.old") {
                    old_seg_files.push(path);
                }
            }
        }

        if !old_seg_files.is_empty() {
            if compact_done {
                // Compaction succeeded (all new segments were swapped in), crash occurred during .old cleanup
                for old_path in old_seg_files {
                    let _ = fs::remove_file(old_path);
                }
                let _ = fs::remove_file(&marker_path);
            } else {
                // Compaction crashed MID-SWAP! Rollback!
                // 1. Delete any partially swapped .seg files
                for seg_path in new_seg_files {
                    let _ = fs::remove_file(seg_path);
                }
                // 2. Restore all .seg.old files back to .seg
                for old_path in old_seg_files {
                    let path_str = old_path.to_string_lossy();
                    let new_path_str = &path_str[..path_str.len() - 4]; // strip ".old"
                    let _ = fs::rename(&old_path, new_path_str);
                }
                let _ = fs::remove_file(&marker_path);
            }
        } else if marker_path.exists() {
            let _ = fs::remove_file(&marker_path);
        }

        let mut segment_ids = Vec::new();

        for entry in fs::read_dir(segments_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("segment_") && name_str.ends_with(".seg") {
                let id_str = &name_str[8..name_str.len() - 4];
                if let Ok(id) = id_str.parse::<u64>() {
                    segment_ids.push(id);
                }
            }
        }

        segment_ids.sort_unstable();

        if segment_ids.is_empty() {
            return Ok((1, 0));
        }

        let mut latest_id = 1;
        let mut latest_valid_offset = 0;

        for &seg_id in &segment_ids {
            latest_id = seg_id;
            let path = segments_dir.join(segment_file_name(seg_id));
            let mut file = OpenOptions::new().read(true).write(true).open(&path)?;

            // 1. Read & Validate Segment Header
            let _seg_header = match SegmentHeader::read_from(&mut file) {
                Ok(h) => h,
                Err(e) => {
                    return Err(OosLiteError::CorruptedSegment {
                        offset: 0,
                        reason: format!("Corrupted header in {}: {}", path.display(), e),
                    });
                }
            };

            // 2. Sequentially scan records
            let mut current_offset = SEGMENT_HEADER_SIZE as u64;
            let file_len = file.metadata()?.len();

            while current_offset < file_len {
                file.seek(SeekFrom::Start(current_offset))?;
                match RecordHeader::read_from(&mut file) {
                    Ok(Some(header)) => {
                        let payload_offset = current_offset + header.header_size as u64;
                        let record_len = header.header_size as u64 + header.payload_len as u64;

                        // Verify payload existence and CRC
                        if current_offset + record_len > file_len {
                            warn!(
                                segment = seg_id,
                                offset = current_offset,
                                "Incomplete record payload detected, truncating segment to valid point"
                            );
                            file.set_len(current_offset)?;
                            file.sync_all()?;
                            break;
                        }

                        let mut payload_buf = vec![0u8; header.payload_len as usize];
                        file.read_exact(&mut payload_buf)?;

                        let actual_crc = crc32fast::hash(&payload_buf);
                        if actual_crc != header.payload_crc {
                            warn!(
                                segment = seg_id,
                                offset = current_offset,
                                "Corrupted payload CRC at tail, truncating segment to valid point"
                            );
                            file.set_len(current_offset)?;
                            file.sync_all()?;
                            break;
                        }

                        // Record is 100% valid -> index it
                        index.insert(
                            header.chunk_id,
                            ChunkLocation {
                                segment_id: seg_id,
                                record_offset: current_offset,
                                payload_offset,
                                payload_len: header.payload_len,
                                raw_len: header.raw_len,
                            },
                        );


                        current_offset += record_len;
                    }
                    Ok(None) => {
                        // Clean EOF
                        break;
                    }
                    Err(err) => {
                        // Corrupted record header at tail due to crash
                        warn!(
                            segment = seg_id,
                            offset = current_offset,
                            error = %err,
                            "Partial record header at tail detected, recovering segment"
                        );
                        file.set_len(current_offset)?;
                        file.sync_all()?;
                        break;
                    }
                }
            }

            latest_valid_offset = current_offset;
        }

        info!(
            latest_segment = latest_id,
            resume_offset = latest_valid_offset,
            indexed_chunks = index.len(),
            "Segment store recovered and initialized"
        );

        Ok((latest_id, latest_valid_offset))
    }

    pub fn has_chunk(&self, id: &ChunkId) -> bool {
        self.index.contains(id)
    }

    pub fn put_chunk(&self, data: &[u8]) -> Result<(ChunkId, bool)> {
        let chunk_id = ChunkId::from_data(data);

        // Deduplication check
        if self.index.contains(&chunk_id) {
            return Ok((chunk_id, false));
        }

        let mut writer = self.writer.lock().map_err(|e| {
            OosLiteError::Internal(format!("SegmentWriter mutex poisoned: {e}"))
        })?;
        // Double check after lock
        if self.index.contains(&chunk_id) {
            return Ok((chunk_id, false));
        }

        writer.append_chunk(chunk_id, data, &self.index)?;
        Ok((chunk_id, true))
    }

    pub fn get_chunk(&self, id: &ChunkId) -> Result<Vec<u8>> {
        let location = self
            .index
            .get(id)
            .ok_or_else(|| OosLiteError::ChunkNotFound(id.to_string()))?;

        self.reader.read_chunk(id, &location)
    }

    pub fn sync(&self) -> Result<()> {
        let mut writer = self.writer.lock().map_err(|e| {
            OosLiteError::Internal(format!("SegmentWriter mutex poisoned: {e}"))
        })?;
        writer.sync()
    }

    pub fn chunk_count(&self) -> usize {
        self.index.len()
    }

    pub fn vault_key(&self) -> Option<&Arc<VaultKey>> {
        self.vault_key.as_ref()
    }

    pub fn get_location(&self, id: &ChunkId) -> Option<ChunkLocation> {
        self.index.get(id)
    }

    pub fn clear_cache(&self) {
        self.reader.clear_cache();
    }


    pub fn reclaim_unreachable_chunks(&self, reachable: &std::collections::HashSet<ChunkId>) -> usize {
        self.index.retain(|id, _loc| reachable.contains(id))
    }

    pub fn compact_and_reclaim(&self, reachable: &std::collections::HashSet<ChunkId>) -> Result<usize> {
        let mut writer_guard = self.writer.lock().map_err(|e| {
            OosLiteError::Internal(format!("SegmentWriter mutex poisoned: {e}"))
        })?;
        writer_guard.sync()?;

        let total_before = self.index.len();

        // 1. Quick check: if all chunks are reachable, nothing to reclaim
        let mut has_dead_chunks = false;
        for (id, _) in self.index.entries() {
            if !reachable.contains(&id) {
                has_dead_chunks = true;
                break;
            }
        }
        if !has_dead_chunks {
            return Ok(0);
        }

        let max_seg_size = writer_guard.max_segment_size();

        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp_compact_dir = self.segments_dir.join(format!(
            ".compact_tmp_{}_{:?}_{}",
            std::process::id(),
            std::thread::current().id(),
            now_ns
        ));
        if tmp_compact_dir.exists() {
            let _ = fs::remove_dir_all(&tmp_compact_dir);
        }
        fs::create_dir_all(&tmp_compact_dir)?;

        let new_index = SegmentIndex::new();
        let mut new_writer = SegmentWriter::open(&tmp_compact_dir, 1, 0, max_seg_size, self.vault_key.clone())?;

        // 3. Stream retained chunks ONE-BY-ONE (Zero-OOM streaming)
        for (id, loc) in self.index.entries() {
            if reachable.contains(&id) {
                let data = self.reader.read_chunk(&id, &loc)?;
                new_writer.append_chunk(id, &data, &new_index)?;
            }
        }

        let retained_count = new_index.len();
        let dead_count = total_before.saturating_sub(retained_count);

        new_writer.sync()?;
        let latest_seg_id = new_writer.current_segment_id();
        let resume_offset = new_writer.current_offset();
        new_writer.close_file()?;

        // 4. Release current writer and clear reader cache before file swap
        writer_guard.close_file()?;
        self.reader.clear_cache();

        // 5. Two-Phase Safe Swap:
        // Phase 5a: Rename all active segment_*.seg to segment_*.seg.old
        let mut old_files = Vec::new();
        for entry in fs::read_dir(&self.segments_dir)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("segment_") && name.ends_with(".seg") {
                    let old_path = self.segments_dir.join(format!("{}.old", name));
                    fs::rename(&path, &old_path)?;
                    old_files.push(old_path);
                }
            }
        }

        // Phase 5b: Finalize segment files
        let marker_path = self.segments_dir.join(".compact_done");
        if retained_count > 0 {
            for entry in fs::read_dir(&tmp_compact_dir)? {
                let entry = entry?;
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("segment_") && name.ends_with(".seg") {
                        let dest = self.segments_dir.join(name);
                        fs::rename(&path, &dest)?;
                    }
                }
            }
            let _ = fs::remove_dir_all(&tmp_compact_dir);

            // Fsync each newly placed segment file
            if let Ok(entries) = fs::read_dir(&self.segments_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with("segment_") && name.ends_with(".seg") {
                            if let Ok(f) = fs::File::open(&path) {
                                let _ = f.sync_all();
                            }
                        }
                    }
                }
            }

            // Sync segments directory entries before creating marker
            if let Ok(dir_f) = fs::File::open(&self.segments_dir) {
                let _ = dir_f.sync_all();
            }

            // Write completion marker indicating all new segments are safely swapped in, and fsync
            {
                let mut marker_file = fs::File::create(&marker_path)?;
                marker_file.write_all(b"COMPACT_DONE")?;
                marker_file.sync_all()?;
            }

            // Sync segments directory entries after creating marker
            if let Ok(dir_f) = fs::File::open(&self.segments_dir) {
                let _ = dir_f.sync_all();
            }

            // Phase 5c: New segments are safely in place, now delete backup .old files and marker
            for old_path in old_files {
                let _ = fs::remove_file(old_path);
            }
            let _ = fs::remove_file(&marker_path);

            // 6. Update in-memory index and re-attach writer
            self.index.replace_with(new_index);
            *writer_guard = SegmentWriter::open(&self.segments_dir, latest_seg_id, resume_offset, max_seg_size, self.vault_key.clone())?;
        } else {
            let _ = fs::remove_dir_all(&tmp_compact_dir);

            // Write completion marker and fsync
            {
                let mut marker_file = fs::File::create(&marker_path)?;
                marker_file.write_all(b"COMPACT_DONE")?;
                marker_file.sync_all()?;
            }

            // Phase 5c: Delete all old files and marker
            for old_path in old_files {
                let _ = fs::remove_file(old_path);
            }
            let _ = fs::remove_file(&marker_path);

            // 6. When zero chunks are kept, re-initialize fresh segment_00000001.seg with resume_offset 0
            self.index.replace_with(new_index);
            *writer_guard = SegmentWriter::open(&self.segments_dir, 1, 0, max_seg_size, self.vault_key.clone())?;
        }

        Ok(dead_count)
    }

    pub fn all_chunk_ids(&self) -> Vec<ChunkId> {
        self.index.all_chunk_ids()
    }

    pub fn unique_payload_bytes(&self) -> u64 {
        self.index.total_payload_bytes()
    }

    pub fn unique_raw_bytes(&self) -> u64 {
        self.index.total_raw_bytes()
    }


    pub fn segments_dir(&self) -> &Path {
        &self.segments_dir
    }

    pub fn physical_disk_bytes(&self) -> Result<u64> {
        let mut total = 0u64;
        if self.segments_dir.exists() {
            for entry in fs::read_dir(&self.segments_dir)? {
                let entry = entry?;
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("segment_") && name.ends_with(".seg") {
                        total += entry.metadata()?.len();
                    }
                }
            }
        }
        Ok(total)
    }
}
