pub mod chunk;
pub mod engine;
pub mod error;
pub mod fsck;
pub mod gc;
pub mod index;
pub mod manifest;
pub mod object;
pub mod segment;
pub mod snapshot;
pub mod vfs;
pub mod wal;
pub mod crypto;
pub mod watcher;

pub use chunk::{ChunkId, Chunker};
pub use crypto::{write_vault_file_atomic, VaultKey};
pub use engine::{EngineStats, PutSummary, StorageEngine};
pub use error::{OosLiteError, Result};
pub use fsck::{FsckReport, FsckRunner};
pub use gc::{GarbageCollector, GcStats};
pub use manifest::Manifest;
pub use object::{ObjectId, ObjectRecord, ObjectVersion};
pub use snapshot::{Snapshot, SnapshotEntry};
pub use vfs::{DecompressedChunkCache, VfsNode, VfsNodeType, VfsTree};
pub use watcher::{WatcherConfig, WatcherHandle, WatcherService};


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = OosLiteError::ChunkNotFound("test_chunk_id".to_string());
        assert_eq!(format!("{}", err), "Chunk not found: test_chunk_id");

        let checksum_err = OosLiteError::ChecksumMismatch {
            chunk_id: "chk1".to_string(),
            expected: 0x12345678,
            actual: 0x87654321,
        };
        assert!(format!("{}", checksum_err).contains("Checksum mismatch"));
    }

    #[test]
    fn test_tracing_init_in_tests() {
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .try_init();
    }
}
