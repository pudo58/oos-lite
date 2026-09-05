//! Chunk engine: FastCDC, BLAKE3 content addressing, CRC32C verification.

pub mod cdc;
pub mod id;
pub mod store;

pub use cdc::{ChunkCut, Chunker, StreamChunker, MAX_CHUNK_SIZE, MIN_CHUNK_SIZE, TARGET_CHUNK_SIZE};
pub use id::ChunkId;
pub use store::{Chunk, ChunkStore};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::OosLiteError;
    use std::fs::OpenOptions;
    use std::io::{Read, Seek, SeekFrom, Write};
    use tempfile::tempdir;

    #[test]
    fn test_chunk_id_deterministic() {
        let data = b"Hello, OOS-Lite Chunk Engine!";
        let id1 = ChunkId::from_data(data);
        let id2 = ChunkId::from_data(data);
        assert_eq!(id1, id2);
        assert_eq!(id1.to_hex().len(), 64);

        let parsed: ChunkId = id1.to_hex().parse().expect("Parse ChunkId failed");
        assert_eq!(id1, parsed);
    }

    #[test]
    fn test_put_get_and_deduplication() {
        let dir = tempdir().expect("Failed to create tempdir");
        let store = ChunkStore::new(dir.path()).expect("Failed to create ChunkStore");

        let data = b"Deduplication test data payload 1234567890";

        // First put -> must be new
        let (id1, is_new1) = store.put_chunk(data).expect("First put_chunk failed");
        assert!(is_new1, "First put must return is_new = true");
        assert!(store.has_chunk(&id1));
        assert_eq!(store.count_chunks().expect("count failed"), 1);

        // Second put with identical data -> must be deduplicated (not written again)
        let (id2, is_new2) = store.put_chunk(data).expect("Second put_chunk failed");
        assert!(!is_new2, "Duplicate put must return is_new = false");
        assert_eq!(id1, id2);
        assert_eq!(
            store.count_chunks().expect("count failed"),
            1,
            "Physical chunk count must remain exactly 1 after duplicate put"
        );

        // Get chunk back and verify byte-for-byte content
        let fetched = store.get_chunk(&id1).expect("get_chunk failed");
        assert_eq!(fetched, data);
    }

    #[test]
    fn test_persistence_across_instances() {
        let dir = tempdir().expect("Failed to create tempdir");
        let data = b"Persistent data across process / store restart";
        let id = {
            let store = ChunkStore::new(dir.path()).expect("Store 1 failed");
            let (id, is_new) = store.put_chunk(data).expect("put failed");
            assert!(is_new);
            id
        };

        // Reload store from the exact same directory (simulating process restart)
        let reloaded_store = ChunkStore::new(dir.path()).expect("Store 2 failed");
        assert!(reloaded_store.has_chunk(&id));
        let fetched = reloaded_store.get_chunk(&id).expect("get on reloaded store failed");
        assert_eq!(fetched, data);
    }

    #[test]
    fn test_checksum_mismatch_detection() {
        let dir = tempdir().expect("Failed to create tempdir");
        let store = ChunkStore::new(dir.path()).expect("Failed to create ChunkStore");

        let data = b"Very important uncorrupted data";
        let (id, _) = store.put_chunk(data).expect("put_chunk failed");

        // Locate the physical chunk file and corrupt exactly 1 byte in the data payload
        let hex = id.to_hex();
        let chunk_file_path = dir.path().join(&hex[0..2]).join(format!("{}.chunk", hex));
        assert!(chunk_file_path.exists());

        // Header is 12 bytes. Byte 13 is the first byte of payload.
        {
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&chunk_file_path)
                .expect("Failed to open chunk file for bit flip");
            file.seek(SeekFrom::Start(13)).expect("Seek failed");
            let mut b = [0u8; 1];
            file.read_exact(&mut b).expect("Read byte failed");
            b[0] ^= 0xFF; // Flip all bits of this 1 byte
            file.seek(SeekFrom::Start(13)).expect("Seek back failed");
            file.write_all(&b).expect("Write corrupted byte failed");
            file.sync_all().expect("Sync failed");
        }

        // Now attempt to read the chunk -> must return ChecksumMismatch error
        let result = store.get_chunk(&id);
        match result {
            Err(OosLiteError::ChecksumMismatch { chunk_id, expected, actual }) => {
                assert_eq!(chunk_id, id.to_hex());
                assert_ne!(expected, actual);
            }
            other => panic!("Expected ChecksumMismatch error, but got {:?}", other),
        }
    }

    #[test]
    fn test_delete_chunk() {
        let dir = tempdir().expect("Failed to create tempdir");
        let store = ChunkStore::new(dir.path()).expect("Failed to create ChunkStore");

        let data = b"Data to be deleted";
        let (id, is_new) = store.put_chunk(data).expect("put failed");
        assert!(is_new);
        assert!(store.has_chunk(&id));

        assert!(store.delete_chunk(&id).expect("delete failed"));
        assert!(!store.has_chunk(&id));
        assert!(!store.delete_chunk(&id).expect("delete non-existing failed"));
    }
}
