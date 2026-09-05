use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tracing::{debug, info};

use crate::chunk::ChunkId;
use crate::error::Result;
use super::format::{
    ChunkLocation, CompressionCodec, RecordHeader, SegmentHeader, DEFAULT_MAX_SEGMENT_SIZE,
    RECORD_HEADER_SIZE, SEGMENT_HEADER_SIZE,
};
use super::index::SegmentIndex;

pub fn segment_file_name(segment_id: u64) -> String {
    format!("segment_{:08}.seg", segment_id)
}

pub struct SegmentWriter {
    segments_dir: PathBuf,
    current_segment_id: u64,
    current_file: Option<BufWriter<File>>,
    current_offset: u64,
    max_segment_size: u64,
}

impl SegmentWriter {
    pub fn open(
        segments_dir: &Path,
        segment_id: u64,
        resume_offset: u64,
        max_segment_size: u64,
    ) -> Result<Self> {
        let path = segments_dir.join(segment_file_name(segment_id));
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)?;

        let current_offset = if resume_offset == 0 {
            // New segment file: write segment header
            let header = SegmentHeader::new(segment_id);
            header.write_to(&mut file)?;
            file.sync_all()?;
            SEGMENT_HEADER_SIZE as u64
        } else {
            file.seek(SeekFrom::Start(resume_offset))?;
            resume_offset
        };

        Ok(Self {
            segments_dir: segments_dir.to_path_buf(),
            current_segment_id: segment_id,
            current_file: Some(BufWriter::new(file)),
            current_offset,
            max_segment_size: if max_segment_size == 0 {
                DEFAULT_MAX_SEGMENT_SIZE
            } else {
                max_segment_size
            },
        })
    }

    pub fn current_segment_id(&self) -> u64 {
        self.current_segment_id
    }

    pub fn current_offset(&self) -> u64 {
        self.current_offset
    }

    pub fn max_segment_size(&self) -> u64 {
        self.max_segment_size
    }

    /// Appends a chunk to the active segment.
    /// Deduplication identity (chunk_id) was already computed on raw data via BLAKE3.
    /// Evaluates conditional compression (Zstandard level 3): stores compressed if savings >= 5%.
    pub fn append_chunk(
        &mut self,
        chunk_id: ChunkId,
        data: &[u8],
        index: &SegmentIndex,
    ) -> Result<ChunkLocation> {
        let compressed = zstd::encode_all(data, 3);
        let (payload_bytes, codec): (&[u8], CompressionCodec) = match &compressed {
            Ok(comp) if (comp.len() as u64) < (data.len() as u64 * 95 / 100) => {
                (comp.as_slice(), CompressionCodec::Zstd)
            }
            _ => (data, CompressionCodec::None),
        };

        let record_size = (RECORD_HEADER_SIZE + payload_bytes.len()) as u64;

        if self.current_offset + record_size > self.max_segment_size {
            self.rotate()?;
        }

        let record_offset = self.current_offset;
        let payload_offset = record_offset + RECORD_HEADER_SIZE as u64;

        let record_header = RecordHeader::new(chunk_id, codec, data.len() as u32, payload_bytes);
        let file = self.current_file.as_mut().ok_or_else(|| {
            crate::error::OosLiteError::Internal("SegmentWriter file handle is closed".to_string())
        })?;
        record_header.write_to(file)?;
        file.write_all(payload_bytes)?;
        file.flush()?;

        let location = ChunkLocation {
            segment_id: self.current_segment_id,
            record_offset,
            payload_offset,
            payload_len: payload_bytes.len() as u32,
            raw_len: data.len() as u32,
        };

        self.current_offset += record_size;
        index.insert(chunk_id, location);

        debug!(
            chunk_id = %chunk_id,
            segment_id = self.current_segment_id,
            offset = record_offset,
            raw_size = data.len(),
            stored_size = payload_bytes.len(),
            codec = ?codec,
            "Appended chunk to segment"
        );

        Ok(location)
    }


    pub fn sync(&mut self) -> Result<()> {
        if let Some(ref mut f) = self.current_file {
            f.flush()?;
            f.get_ref().sync_data()?;
        }
        Ok(())
    }

    pub fn close_file(&mut self) -> Result<()> {
        if let Some(mut f) = self.current_file.take() {
            f.flush()?;
            f.get_ref().sync_all()?;
        }
        Ok(())
    }

    fn rotate(&mut self) -> Result<()> {
        self.sync()?;
        let next_segment_id = self.current_segment_id + 1;
        info!(
            closed_segment = self.current_segment_id,
            new_segment = next_segment_id,
            "Rotating segment store"
        );

        let new_writer = Self::open(
            &self.segments_dir,
            next_segment_id,
            0,
            self.max_segment_size,
        )?;
        *self = new_writer;
        Ok(())
    }
}
