use thiserror::Error;

#[derive(Error, Debug)]
pub enum OosLiteError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(#[from] sled::Error),

    #[error("Checksum mismatch for chunk {chunk_id}: expected {expected:08x}, got {actual:08x}")]
    ChecksumMismatch {
        chunk_id: String,
        expected: u32,
        actual: u32,
    },

    #[error("Chunk not found: {0}")]
    ChunkNotFound(String),

    #[error("Segment full: current size {current}, requested {requested}, capacity {capacity}")]
    SegmentFull {
        current: u64,
        requested: u64,
        capacity: u64,
    },

    #[error("Corrupted segment at offset {offset}: {reason}")]
    CorruptedSegment {
        offset: u64,
        reason: String,
    },

    #[error("Object not found: {0}")]
    ObjectNotFound(String),

    #[error("Snapshot not found: {0}")]
    SnapshotNotFound(String),

    #[error("WAL recovery error: {0}")]
    WalRecovery(String),

    #[error("Internal storage error: {0}")]
    Internal(String),

    #[error("Store is locked by another process: {0}")]
    StoreLocked(String),

    #[error("Invalid logical file name '{name}': {reason}")]
    InvalidName { name: String, reason: String },

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Password required to unlock encrypted store")]
    PasswordRequired,

    #[error("Decryption failed for chunk: {0}")]
    DecryptionFailed(String),
}

pub type Result<T> = std::result::Result<T, OosLiteError>;
