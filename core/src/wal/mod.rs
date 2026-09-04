//! Write-Ahead Logging (WAL) for redo-only crash consistency.

pub mod format;

pub use format::{WalPutPayload, WalRecord, WalRecordPayload, WalRecordType, WAL_HEADER_SIZE, WAL_MAGIC};

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::error::{OosLiteError, Result};

pub struct Wal {
    _dir: PathBuf,
    log_path: PathBuf,
    meta_path: PathBuf,
    file: File,
    current_lsn: u64,
    checkpoint_lsn: u64,
}

impl Wal {
    pub fn open<P: AsRef<Path>>(wal_dir: P) -> Result<Self> {
        let dir = wal_dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;

        let log_path = dir.join("wal.log");
        let meta_path = dir.join("checkpoint.meta");

        let checkpoint_lsn = if meta_path.exists() {
            let bytes = fs::read(&meta_path)?;
            if bytes.len() >= 8 {
                u64::from_le_bytes(bytes[0..8].try_into().unwrap())
            } else {
                0
            }
        } else {
            0
        };

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&log_path)?;

        let mut content = Vec::new();
        file.read_to_end(&mut content)?;

        let mut current_lsn = checkpoint_lsn;
        let mut offset = 0;
        let mut valid_end_offset = 0;

        while offset < content.len() {
            let remaining = &content[offset..];
            if remaining.len() < WAL_HEADER_SIZE {
                warn!(
                    offset = offset,
                    remaining = remaining.len(),
                    "Partial WAL header at EOF, truncating tail"
                );
                break;
            }

            if remaining[0..4] != WAL_MAGIC {
                warn!(offset = offset, "Invalid WAL magic bytes found");
                break;
            }

            match WalRecord::decode(remaining) {
                Ok((record, consumed)) => {
                    if record.lsn > current_lsn {
                        current_lsn = record.lsn;
                    }
                    offset += consumed;
                    valid_end_offset = offset;
                }
                Err(OosLiteError::ChecksumMismatch { expected, actual, .. }) => {
                    warn!(
                        offset = offset,
                        expected = expected,
                        actual = actual,
                        "Corrupted WAL record payload, stopping at last valid record"
                    );
                    break;
                }
                Err(OosLiteError::WalRecovery(msg)) => {
                    warn!(
                        offset = offset,
                        error = %msg,
                        "Corrupted WAL record, stopping at last valid record"
                    );
                    break;
                }
                Err(e) => return Err(e),
            }
        }

        // Truncate any incomplete or corrupted tail
        if valid_end_offset < content.len() {
            info!(
                truncated_bytes = content.len() - valid_end_offset,
                valid_bytes = valid_end_offset,
                "Truncating corrupted/partial WAL tail"
            );
            file.set_len(valid_end_offset as u64)?;
        }

        file.seek(SeekFrom::End(0))?;

        Ok(Self {
            _dir: dir,
            log_path,
            meta_path,
            file,
            current_lsn,
            checkpoint_lsn,
        })
    }

    pub fn current_lsn(&self) -> u64 {
        self.current_lsn
    }

    pub fn checkpoint_lsn(&self) -> u64 {
        self.checkpoint_lsn
    }

    pub fn append_put_and_sync(&mut self, payload: WalPutPayload) -> Result<u64> {
        self.current_lsn += 1;
        let record = WalRecord::new_put(self.current_lsn, payload);
        let encoded = record.encode();

        self.file.write_all(&encoded)?;
        self.file.sync_all()?;

        Ok(self.current_lsn)
    }

    pub fn read_uncheckpointed_records(&self) -> Result<Vec<WalRecord>> {
        if !self.log_path.exists() {
            return Ok(Vec::new());
        }

        let mut file = File::open(&self.log_path)?;
        let mut content = Vec::new();
        file.read_to_end(&mut content)?;

        let mut records = Vec::new();
        let mut offset = 0;

        while offset < content.len() {
            let remaining = &content[offset..];
            if remaining.len() < WAL_HEADER_SIZE {
                break;
            }
            if remaining[0..4] != WAL_MAGIC {
                break;
            }

            match WalRecord::decode(remaining) {
                Ok((record, consumed)) => {
                    offset += consumed;
                    if record.lsn > self.checkpoint_lsn {
                        records.push(record);
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Error decoding WAL record during replay");
                    return Err(e);
                }
            }
        }

        Ok(records)
    }

    pub fn checkpoint(&mut self, lsn: u64) -> Result<()> {
        self.checkpoint_lsn = lsn;

        // Persist checkpoint LSN atomically
        let mut meta_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&self.meta_path)?;
        meta_file.write_all(&lsn.to_le_bytes())?;
        meta_file.sync_all()?;

        // If all records up to current_lsn are checkpointed, truncate the WAL log to 0
        if self.checkpoint_lsn >= self.current_lsn {
            self.file.set_len(0)?;
            self.file.seek(SeekFrom::Start(0))?;
            self.file.sync_all()?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::ChunkId;
    use crate::manifest::Manifest;
    use crate::object::ObjectId;
    use tempfile::tempdir;

    fn sample_payload(name: &str) -> WalPutPayload {
        let chunk_data = b"wal test chunk data".to_vec();
        let chunk_id = ChunkId::from_data(&chunk_data);
        let manifest = Manifest::new(vec![chunk_id], chunk_data.len() as u64, [1u8; 32]);
        WalPutPayload {
            name: name.to_string(),
            object_id: ObjectId::generate(),
            version: 1,
            manifest,
            chunks: vec![(chunk_id, chunk_data)],
        }
    }

    #[test]
    fn test_wal_append_and_read_records() {
        let dir = tempdir().unwrap();
        let wal_dir = dir.path().join("wal");

        let mut wal = Wal::open(&wal_dir).unwrap();
        assert_eq!(wal.current_lsn(), 0);
        assert_eq!(wal.checkpoint_lsn(), 0);

        let p1 = sample_payload("file1.txt");
        let lsn1 = wal.append_put_and_sync(p1).unwrap();
        assert_eq!(lsn1, 1);

        let p2 = sample_payload("file2.txt");
        let lsn2 = wal.append_put_and_sync(p2).unwrap();
        assert_eq!(lsn2, 2);

        let records = wal.read_uncheckpointed_records().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].lsn, 1);
        assert_eq!(records[1].lsn, 2);

        // Checkpoint record 1
        wal.checkpoint(1).unwrap();
        assert_eq!(wal.checkpoint_lsn(), 1);

        // Now only record 2 is uncheckpointed
        let records = wal.read_uncheckpointed_records().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].lsn, 2);

        // Checkpoint record 2 (all checkpointed -> truncated)
        wal.checkpoint(2).unwrap();
        assert_eq!(wal.checkpoint_lsn(), 2);
        let records = wal.read_uncheckpointed_records().unwrap();
        assert_eq!(records.len(), 0);
    }

    #[test]
    fn test_wal_crc_mismatch_detected() {
        let dir = tempdir().unwrap();
        let wal_dir = dir.path().join("wal");

        let mut wal = Wal::open(&wal_dir).unwrap();
        let p = sample_payload("corrupt_test.txt");
        wal.append_put_and_sync(p).unwrap();
        drop(wal);

        // Corrupt a byte in the payload
        let log_file = wal_dir.join("wal.log");
        let mut bytes = fs::read(&log_file).unwrap();
        assert!(bytes.len() > WAL_HEADER_SIZE);
        // Flip bit in payload
        let last_idx = bytes.len() - 1;
        bytes[last_idx] ^= 0xFF;
        fs::write(&log_file, &bytes).unwrap();

        // Reopening WAL should detect corrupted payload and truncate tail
        let wal = Wal::open(&wal_dir).unwrap();
        let records = wal.read_uncheckpointed_records().unwrap();
        assert_eq!(records.len(), 0);
    }

    #[test]
    fn test_wal_partial_tail_truncation() {
        let dir = tempdir().unwrap();
        let wal_dir = dir.path().join("wal");

        let mut wal = Wal::open(&wal_dir).unwrap();
        let p = sample_payload("tail_test.txt");
        wal.append_put_and_sync(p).unwrap();
        drop(wal);

        // Append incomplete header bytes at EOF (simulating crash mid-write)
        let log_file = wal_dir.join("wal.log");
        let mut file = OpenOptions::new().append(true).open(&log_file).unwrap();
        file.write_all(b"OOSW_INCOMPLETE").unwrap();
        file.sync_all().unwrap();
        drop(file);

        // Reopening WAL should cleanly truncate partial write
        let wal = Wal::open(&wal_dir).unwrap();
        let records = wal.read_uncheckpointed_records().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].lsn, 1);
    }
}
