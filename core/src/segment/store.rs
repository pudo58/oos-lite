use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

use crate::chunk::ChunkId;
use crate::error::{OosLiteError, Result};
use super::format::{
    ChunkLocation, RecordHeader, SegmentHeader, RECORD_HEADER_SIZE, SEGMENT_HEADER_SIZE,
};
use super::index::SegmentIndex;
use super::reader::SegmentReader;
use super::writer::{segment_file_name, SegmentWriter};

pub struct SegmentStore {
    pub segments_dir: PathBuf,
    index: Arc<SegmentIndex>,
    reader: SegmentReader,
    writer: Mutex<SegmentWriter>,
}

impl SegmentStore {
    pub fn new<P: AsRef<Path>>(dir: P) -> Result<Self> {
        Self::with_max_segment_size(dir, 0)
    }

    pub fn with_max_segment_size<P: AsRef<Path>>(
        dir: P,
        max_segment_size: u64,
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
        )?;
        let reader = SegmentReader::new(&segments_dir);

        Ok(Self {
            segments_dir,
            index,
            reader,
            writer: Mutex::new(writer),
        })
    }

    /// Scans all segment files in sequential order, validates records,
    /// populates the index, and repairs any partial/corrupted record at EOF caused by crash/kill.
    fn recover_and_index(
        segments_dir: &Path,
        index: &SegmentIndex,
    ) -> Result<(u64, u64)> {
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
                        let payload_offset = current_offset + RECORD_HEADER_SIZE as u64;
                        let record_len = RECORD_HEADER_SIZE as u64 + header.payload_len as u64;

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

        let mut writer = self.writer.lock().unwrap();
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
        let mut writer = self.writer.lock().unwrap();
        writer.sync()
    }

    pub fn chunk_count(&self) -> usize {
        self.index.len()
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

        // 1. Collect all chunks that must be preserved
        let mut chunks_to_keep: Vec<(ChunkId, Vec<u8>)> = Vec::new();
        for (id, loc) in self.index.entries() {
            if reachable.contains(&id) {
                let data = self.reader.read_chunk(&id, &loc)?;
                chunks_to_keep.push((id, data));
            }
        }

        let dead_count = total_before.saturating_sub(chunks_to_keep.len());
        if dead_count == 0 {
            return Ok(0);
        }

        let max_seg_size = writer_guard.max_segment_size();

        // 2. Release current segment file handle in writer
        writer_guard.close_file()?;

        // 3. If zero chunks left to keep, clear all segment files and reinitialize
        if chunks_to_keep.is_empty() {
            for entry in fs::read_dir(&self.segments_dir)? {
                let entry = entry?;
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("segment_") && name.ends_with(".seg") {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
            self.index.clear();
            *writer_guard = SegmentWriter::open(&self.segments_dir, 1, 0, max_seg_size)?;
            return Ok(dead_count);
        }

        // 4. Write all retained chunks into temporary directory
        let tmp_compact_dir = self.segments_dir.join(format!(".compact_tmp_{}", std::process::id()));
        if tmp_compact_dir.exists() {
            let _ = fs::remove_dir_all(&tmp_compact_dir);
        }
        fs::create_dir_all(&tmp_compact_dir)?;

        let new_index = SegmentIndex::new();
        let mut new_writer = SegmentWriter::open(&tmp_compact_dir, 1, 0, max_seg_size)?;
        for (id, data) in &chunks_to_keep {
            new_writer.append_chunk(*id, data, &new_index)?;
        }
        new_writer.sync()?;
        let latest_seg_id = new_writer.current_segment_id();
        let resume_offset = new_writer.current_offset();
        new_writer.close_file()?;

        // 5. Delete old segment files from segments_dir
        for entry in fs::read_dir(&self.segments_dir)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("segment_") && name.ends_with(".seg") {
                    let _ = fs::remove_file(&path);
                }
            }
        }

        // 6. Move newly compacted segment files into segments_dir
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

        // 7. Update memory index and re-attach writer
        self.index.replace_with(new_index);
        *writer_guard = SegmentWriter::open(&self.segments_dir, latest_seg_id, resume_offset, max_seg_size)?;

        Ok(dead_count)
    }

    pub fn all_chunk_ids(&self) -> Vec<ChunkId> {
        self.index.all_chunk_ids()
    }

    pub fn unique_payload_bytes(&self) -> u64 {
        self.index.total_payload_bytes()
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
