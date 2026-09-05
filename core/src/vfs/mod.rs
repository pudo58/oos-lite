//! Virtual Filesystem (VFS) abstractions for read-only mounts.

pub mod cache;
pub mod tree;

pub use cache::DecompressedChunkCache;
pub use tree::{
    VfsNode, VfsNodeType, VfsTree, CURRENT_INODE, HISTORY_INODE, ROOT_INODE, SNAPSHOTS_INODE,
};

#[cfg(test)]
mod tests;
