//! Append-only segment store (~256 MiB per segment file).

pub mod format;
pub mod index;
pub mod reader;
pub mod store;
pub mod writer;

pub use format::{ChunkLocation, DEFAULT_MAX_SEGMENT_SIZE};
pub use index::SegmentIndex;
pub use reader::SegmentReader;
pub use store::SegmentStore;
pub use writer::SegmentWriter;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_segment_append_and_random_read() {
        let dir = tempdir().expect("tempdir failed");
        let store = SegmentStore::new(dir.path()).expect("store init failed");

        let chunk1 = b"Chunk payload data number 1";
        let chunk2 = b"Chunk payload data number 2 - slightly longer";

        let (id1, is_new1) = store.put_chunk(chunk1).expect("put chunk1 failed");
        assert!(is_new1);
        let (id2, is_new2) = store.put_chunk(chunk2).expect("put chunk2 failed");
        assert!(is_new2);
        assert_ne!(id1, id2);

        assert_eq!(store.chunk_count(), 2);

        // Fast random read
        let read1 = store.get_chunk(&id1).expect("read chunk1 failed");
        assert_eq!(read1, chunk1);

        let read2 = store.get_chunk(&id2).expect("read chunk2 failed");
        assert_eq!(read2, chunk2);
    }

    #[test]
    fn test_segment_deduplication() {
        let dir = tempdir().expect("tempdir failed");
        let store = SegmentStore::new(dir.path()).expect("store init failed");

        let data = b"Identical chunk repeated";
        let (id1, is_new1) = store.put_chunk(data).expect("put1 failed");
        assert!(is_new1);

        let (id2, is_new2) = store.put_chunk(data).expect("put2 failed");
        assert!(!is_new2);
        assert_eq!(id1, id2);
        assert_eq!(store.chunk_count(), 1);
    }

    #[test]
    fn test_segment_rotation() {
        let dir = tempdir().expect("tempdir failed");
        // Use a tiny max_segment_size (150 bytes) to force rotation
        let store = SegmentStore::with_max_segment_size(dir.path(), 150)
            .expect("store init failed");

        let data1 = vec![b'A'; 60];
        let data2 = vec![b'B'; 60];
        let data3 = vec![b'C'; 60];

        let (id1, _) = store.put_chunk(&data1).expect("put 1 failed");
        let (id2, _) = store.put_chunk(&data2).expect("put 2 failed");
        let (id3, _) = store.put_chunk(&data3).expect("put 3 failed");

        store.sync().expect("sync failed");

        // Verify all 3 chunks can be read back across different rotated segment files
        assert_eq!(store.get_chunk(&id1).unwrap(), data1);
        assert_eq!(store.get_chunk(&id2).unwrap(), data2);
        assert_eq!(store.get_chunk(&id3).unwrap(), data3);

        // Verify multiple segment files were created on disk
        let mut seg_files = 0;
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) == Some("seg") {
                seg_files += 1;
            }
        }
        assert!(seg_files > 1, "Expected at least 2 segment files after rotation, found {}", seg_files);
    }

    #[test]
    fn test_crash_recovery_partial_write() {
        let dir = tempdir().expect("tempdir failed");
        let mut valid_ids = Vec::new();

        // 1. First run: write 3 valid chunks and fsync
        {
            let store = SegmentStore::new(dir.path()).expect("store init failed");
            for i in 0..3 {
                let data = format!("Committed chunk before kill #{}", i).into_bytes();
                let (id, _) = store.put_chunk(&data).expect("put failed");
                valid_ids.push((id, data));
            }
            store.sync().expect("sync failed");
        }

        // 2. Simulate kill -9 during write: append a partially written garbage record at tail
        let seg_path = dir.path().join("segment_00000001.seg");
        assert!(seg_path.exists());
        {
            let mut file = OpenOptions::new()
                .write(true)
                .append(true)
                .open(&seg_path)
                .expect("open segment failed");
            // Write corrupted partial bytes (incomplete header)
            file.write_all(b"OOSR_PARTIAL_CORRUPTED_BYTES_HERE_1234").expect("write garbage failed");
            file.sync_all().expect("sync garbage failed");
        }

        // 3. Restart store: verify automatic recovery repairs the file
        {
            let store = SegmentStore::new(dir.path()).expect("recovery store init failed");

            // All previously committed chunks must be 100% intact
            for (id, original_data) in &valid_ids {
                assert!(store.has_chunk(id));
                let read_data = store.get_chunk(id).expect("read chunk failed after recovery");
                assert_eq!(&read_data, original_data);
            }

            // Verify store can continue writing new chunks normally after recovery
            let new_data = b"New chunk written after clean recovery";
            let (new_id, is_new) = store.put_chunk(new_data).expect("put after recovery failed");
            assert!(is_new);
            assert_eq!(store.get_chunk(&new_id).unwrap(), new_data);
        }
    }

    #[test]
    fn test_chunk_compression_compressible_data() {
        let dir = tempdir().expect("tempdir failed");
        let store = SegmentStore::new(dir.path()).expect("store init failed");

        // 16 KiB of highly repetitive text (compressible)
        let pattern = b"OOS-Lite storage engine with zstandard transparent compression! ";
        let mut raw_data = Vec::with_capacity(16 * 1024);
        while raw_data.len() < 16 * 1024 {
            raw_data.extend_from_slice(pattern);
        }

        let (id, is_new) = store.put_chunk(&raw_data).expect("put chunk failed");
        assert!(is_new);

        // Verify ChunkId was computed on raw data
        assert_eq!(id, crate::chunk::ChunkId::from_data(&raw_data));

        // Verify physical stored size is drastically smaller (< 20% of raw size)
        let location = store.get_location(&id).expect("chunk not in index");
        assert_eq!(location.raw_len, raw_data.len() as u32);
        assert!(
            location.payload_len < (location.raw_len / 5),
            "Expected payload_len ({} bytes) to be < 20% of raw_len ({} bytes)",
            location.payload_len, location.raw_len
        );

        // Verify decompression extracts byte-for-byte identical content
        let read_data = store.get_chunk(&id).expect("read chunk failed");
        assert_eq!(read_data, raw_data);
    }

    #[test]
    fn test_chunk_compression_incompressible_data() {
        let dir = tempdir().expect("tempdir failed");
        let store = SegmentStore::new(dir.path()).expect("store init failed");

        // High entropy pseudo-random sequence (incompressible)
        let mut raw_data = vec![0u8; 1024];
        let mut x: u32 = 123456789;
        for byte in &mut raw_data {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *byte = (x & 0xFF) as u8;
        }

        let (id, is_new) = store.put_chunk(&raw_data).expect("put chunk failed");
        assert!(is_new);

        // Verify that because compression did not save >= 5%, it was saved as raw bytes
        let location = store.get_location(&id).expect("chunk not in index");
        assert_eq!(location.raw_len, raw_data.len() as u32);
        assert_eq!(location.payload_len, raw_data.len() as u32);

        let read_data = store.get_chunk(&id).expect("read chunk failed");
        assert_eq!(read_data, raw_data);
    }

    #[test]
    fn test_chunk_compression_bitrot_corruption_detected() {
        use std::fs::OpenOptions;
        use std::io::{Seek, SeekFrom, Write};

        let dir = tempdir().expect("tempdir failed");
        let store = SegmentStore::new(dir.path()).expect("store init failed");

        let raw_data = b"Predictable text for testing bit-rot CRC32C detection before decompression".repeat(20);
        let (id, _) = store.put_chunk(&raw_data).expect("put chunk failed");
        store.sync().expect("sync failed");

        let location = store.get_location(&id).expect("chunk not in index");

        // Flip a byte in the physical stored payload on disk
        let seg_path = dir.path().join("segment_00000001.seg");
        let mut file = OpenOptions::new().read(true).write(true).open(&seg_path).expect("open failed");
        file.seek(SeekFrom::Start(location.payload_offset)).expect("seek failed");
        let mut byte = [0u8; 1];
        std::io::Read::read_exact(&mut file, &mut byte).expect("read byte failed");
        byte[0] ^= 0xFF; // flip bits
        file.seek(SeekFrom::Start(location.payload_offset)).expect("seek failed");
        file.write_all(&byte).expect("write corrupted byte failed");
        file.sync_all().expect("sync failed");

        // Clear reader cache so it re-reads from disk
        store.clear_cache();


        // Reading the chunk must fail at the CRC32C physical verification step
        let result = store.get_chunk(&id);
        assert!(result.is_err());
        match result.unwrap_err() {
            crate::error::OosLiteError::ChecksumMismatch { .. } => {} // Expected
            other => panic!("Expected ChecksumMismatch, got: {:?}", other),
        }
    }
}

