use std::collections::HashMap;
use std::sync::Arc;
use crate::engine::StorageEngine;
use crate::error::{OosLiteError, Result};
use super::cache::DecompressedChunkCache;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsNodeType {
    Directory,
    RegularFile,
    Symlink,
}

#[derive(Debug, Clone)]
pub struct VfsNode {
    pub ino: u64,
    pub parent_ino: u64,
    pub name: String,
    pub kind: VfsNodeType,
    pub size: u64,
    pub mtime: u64,
    pub manifest_id: Option<String>,
    pub symlink_target: Option<String>,
    pub children: Vec<u64>,
}

/// Static Point-in-Time Virtual Filesystem Tree.
/// Constructed at mount-time, providing read-only access to:
/// - `/current`: latest version of all files
/// - `/snapshots/<label>`: snapshot views
/// - `/history/<dir>/<file>@v<N>` & `<file>@latest`: full version history
pub struct VfsTree {
    nodes: HashMap<u64, VfsNode>,
    engine: Arc<StorageEngine>,
    chunk_cache: DecompressedChunkCache,
}

pub const ROOT_INODE: u64 = 1;
pub const CURRENT_INODE: u64 = 2;
pub const SNAPSHOTS_INODE: u64 = 3;
pub const HISTORY_INODE: u64 = 4;

impl VfsTree {
    /// Builds a static point-in-time VFS tree from the storage engine.
    pub fn build(engine: Arc<StorageEngine>, cache_max_bytes: usize) -> Result<Self> {
        let mut nodes = HashMap::new();
        let mut next_ino = 5u64;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // 1. Root Inode (1)
        nodes.insert(
            ROOT_INODE,
            VfsNode {
                ino: ROOT_INODE,
                parent_ino: ROOT_INODE,
                name: "/".to_string(),
                kind: VfsNodeType::Directory,
                size: 0,
                mtime: now,
                manifest_id: None,
                symlink_target: None,
                children: vec![CURRENT_INODE, SNAPSHOTS_INODE, HISTORY_INODE],
            },
        );

        // 2. Current Inode (2)
        nodes.insert(
            CURRENT_INODE,
            VfsNode {
                ino: CURRENT_INODE,
                parent_ino: ROOT_INODE,
                name: "current".to_string(),
                kind: VfsNodeType::Directory,
                size: 0,
                mtime: now,
                manifest_id: None,
                symlink_target: None,
                children: Vec::new(),
            },
        );

        // 3. Snapshots Inode (3)
        nodes.insert(
            SNAPSHOTS_INODE,
            VfsNode {
                ino: SNAPSHOTS_INODE,
                parent_ino: ROOT_INODE,
                name: "snapshots".to_string(),
                kind: VfsNodeType::Directory,
                size: 0,
                mtime: now,
                manifest_id: None,
                symlink_target: None,
                children: Vec::new(),
            },
        );

        // 4. History Inode (4)
        nodes.insert(
            HISTORY_INODE,
            VfsNode {
                ino: HISTORY_INODE,
                parent_ino: ROOT_INODE,
                name: "history".to_string(),
                kind: VfsNodeType::Directory,
                size: 0,
                mtime: now,
                manifest_id: None,
                symlink_target: None,
                children: Vec::new(),
            },
        );

        // Populate /current
        let named_objects = engine.metadata_store().list_named_objects().unwrap_or_default();
        for (name, _obj_id, record) in &named_objects {
            if let Some(latest) = record.latest() {
                let parts: Vec<&str> = name.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
                if !parts.is_empty() {
                    Self::insert_path(
                        &mut nodes,
                        &mut next_ino,
                        CURRENT_INODE,
                        &parts,
                        latest.manifest_id.clone(),
                        latest.size_bytes,
                        latest.created_at,
                    );
                }
            }
        }

        // Populate /snapshots
        let snapshots = engine.metadata_store().list_snapshots().unwrap_or_default();
        for snap in &snapshots {
            let snap_ino = next_ino;
            next_ino += 1;

            nodes.insert(
                snap_ino,
                VfsNode {
                    ino: snap_ino,
                    parent_ino: SNAPSHOTS_INODE,
                    name: snap.label.clone(),
                    kind: VfsNodeType::Directory,
                    size: 0,
                    mtime: snap.created_at,
                    manifest_id: None,
                    symlink_target: None,
                    children: Vec::new(),
                },
            );
            if let Some(parent) = nodes.get_mut(&SNAPSHOTS_INODE) {
                parent.children.push(snap_ino);
            }

            for entry in &snap.entries {
                let parts: Vec<&str> = entry.name.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
                if !parts.is_empty() {
                    Self::insert_path(
                        &mut nodes,
                        &mut next_ino,
                        snap_ino,
                        &parts,
                        entry.manifest_id.clone(),
                        entry.size_bytes,
                        snap.created_at,
                    );
                }
            }
        }

        // Populate /history
        // Formatted as: /history/<dir_path>/<filename>@v1, <filename>@v2, <filename>@latest
        for (name, _obj_id, record) in &named_objects {
            let parts: Vec<&str> = name.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
            if parts.is_empty() {
                continue;
            }

            let (dir_parts, file_name) = parts.split_at(parts.len() - 1);
            let leaf_file = file_name[0];

            let target_dir_ino = if dir_parts.is_empty() {
                HISTORY_INODE
            } else {
                Self::ensure_dir_path(&mut nodes, &mut next_ino, HISTORY_INODE, dir_parts, now)
            };

            let mut latest_ver = 0u32;
            for v in &record.versions {
                if v.version > latest_ver {
                    latest_ver = v.version;
                }
                let version_file_name = format!("{}@v{}", leaf_file, v.version);
                let ver_ino = next_ino;
                next_ino += 1;

                nodes.insert(
                    ver_ino,
                    VfsNode {
                        ino: ver_ino,
                        parent_ino: target_dir_ino,
                        name: version_file_name,
                        kind: VfsNodeType::RegularFile,
                        size: v.size_bytes,
                        mtime: v.created_at,
                        manifest_id: Some(v.manifest_id.clone()),
                        symlink_target: None,
                        children: Vec::new(),
                    },
                );
                if let Some(parent) = nodes.get_mut(&target_dir_ino) {
                    parent.children.push(ver_ino);
                }
            }

            if latest_ver > 0 {
                let latest_symlink_name = format!("{}@latest", leaf_file);
                let target_symlink = format!("{}@v{}", leaf_file, latest_ver);
                let link_ino = next_ino;
                next_ino += 1;

                nodes.insert(
                    link_ino,
                    VfsNode {
                        ino: link_ino,
                        parent_ino: target_dir_ino,
                        name: latest_symlink_name,
                        kind: VfsNodeType::Symlink,
                        size: target_symlink.len() as u64,
                        mtime: now,
                        manifest_id: None,
                        symlink_target: Some(target_symlink),
                        children: Vec::new(),
                    },
                );
                if let Some(parent) = nodes.get_mut(&target_dir_ino) {
                    parent.children.push(link_ino);
                }
            }
        }

        Ok(Self {
            nodes,
            engine,
            chunk_cache: DecompressedChunkCache::new(cache_max_bytes),
        })
    }

    fn ensure_dir_path(
        nodes: &mut HashMap<u64, VfsNode>,
        next_ino: &mut u64,
        parent_ino: u64,
        dir_parts: &[&str],
        now: u64,
    ) -> u64 {
        let mut curr_ino = parent_ino;
        for part in dir_parts {
            let mut existing_child = None;
            if let Some(curr_node) = nodes.get(&curr_ino) {
                for &child_ino in &curr_node.children {
                    if let Some(child_node) = nodes.get(&child_ino) {
                        if child_node.kind == VfsNodeType::Directory && child_node.name == *part {
                            existing_child = Some(child_ino);
                            break;
                        }
                    }
                }
            }

            curr_ino = match existing_child {
                Some(ino) => ino,
                None => {
                    let dir_ino = *next_ino;
                    *next_ino += 1;
                    nodes.insert(
                        dir_ino,
                        VfsNode {
                            ino: dir_ino,
                            parent_ino: curr_ino,
                            name: part.to_string(),
                            kind: VfsNodeType::Directory,
                            size: 0,
                            mtime: now,
                            manifest_id: None,
                            symlink_target: None,
                            children: Vec::new(),
                        },
                    );
                    if let Some(parent) = nodes.get_mut(&curr_ino) {
                        parent.children.push(dir_ino);
                    }
                    dir_ino
                }
            };
        }
        curr_ino
    }

    fn insert_path(
        nodes: &mut HashMap<u64, VfsNode>,
        next_ino: &mut u64,
        parent_ino: u64,
        parts: &[&str],
        manifest_id: String,
        size: u64,
        mtime: u64,
    ) {
        if parts.is_empty() {
            return;
        }

        let dir_parts = &parts[..parts.len() - 1];
        let file_name = parts[parts.len() - 1];

        let dir_ino = Self::ensure_dir_path(nodes, next_ino, parent_ino, dir_parts, mtime);

        let file_ino = *next_ino;
        *next_ino += 1;

        nodes.insert(
            file_ino,
            VfsNode {
                ino: file_ino,
                parent_ino: dir_ino,
                name: file_name.to_string(),
                kind: VfsNodeType::RegularFile,
                size,
                mtime,
                manifest_id: Some(manifest_id),
                symlink_target: None,
                children: Vec::new(),
            },
        );
        if let Some(parent) = nodes.get_mut(&dir_ino) {
            parent.children.push(file_ino);
        }
    }

    /// Looks up a node by its 64-bit inode number.
    pub fn get_node(&self, ino: u64) -> Option<&VfsNode> {
        self.nodes.get(&ino)
    }

    /// Looks up a child node by name inside a parent directory.
    pub fn lookup_child(&self, parent_ino: u64, name: &str) -> Option<&VfsNode> {
        let parent = self.nodes.get(&parent_ino)?;
        if parent.kind != VfsNodeType::Directory {
            return None;
        }
        for &child_ino in &parent.children {
            if let Some(child) = self.nodes.get(&child_ino) {
                if child.name == name {
                    return Some(child);
                }
            }
        }
        None
    }

    /// Returns the list of child nodes inside a directory.
    pub fn readdir(&self, ino: u64) -> Option<Vec<&VfsNode>> {
        let node = self.nodes.get(&ino)?;
        if node.kind != VfsNodeType::Directory {
            return None;
        }
        let mut entries = Vec::with_capacity(node.children.len());
        for &child_ino in &node.children {
            if let Some(child) = self.nodes.get(&child_ino) {
                entries.push(child);
            }
        }
        Some(entries)
    }

    /// Resolves a relative POSIX-style path (e.g. "/current/photos/cat.jpg") to a VfsNode.
    /// Transparently follows symlinks within the hierarchy.
    pub fn resolve_path(&self, path: &str) -> Option<&VfsNode> {
        let clean = path.trim_matches('/');
        if clean.is_empty() {
            return self.get_node(ROOT_INODE);
        }

        let mut curr_node = self.get_node(ROOT_INODE)?;
        for part in clean.split('/') {
            if part.is_empty() || part == "." {
                continue;
            }
            if part == ".." {
                curr_node = self.get_node(curr_node.parent_ino)?;
                continue;
            }

            let child = self.lookup_child(curr_node.ino, part)?;
            if child.kind == VfsNodeType::Symlink {
                if let Some(ref target) = child.symlink_target {
                    curr_node = self.lookup_child(child.parent_ino, target)?;
                } else {
                    curr_node = child;
                }
            } else {
                curr_node = child;
            }
        }

        Some(curr_node)
    }

    /// Reads a slice of bytes [offset, offset + size] from a file identified by manifest_id.
    /// Only the intersecting chunks are read and decompressed into the LRU cache.
    pub fn read_range(&self, manifest_id: &str, offset: u64, size: u64) -> Result<Vec<u8>> {
        if size == 0 {
            return Ok(Vec::new());
        }

        let _op_guard = self.engine.op_lock().read().map_err(|e| {
            OosLiteError::Internal(format!("VFS read_range op_lock poisoned: {e}"))
        })?;

        let manifest = self
            .engine
            .metadata_store()
            .get_manifest(manifest_id)?
            .ok_or_else(|| OosLiteError::ObjectNotFound(manifest_id.to_string()))?;

        if offset >= manifest.total_size {
            return Ok(Vec::new());
        }

        let actual_read_len = std::cmp::min(size, manifest.total_size - offset) as usize;
        let mut result = Vec::with_capacity(actual_read_len);
        let end_offset = offset + (actual_read_len as u64);

        let mut current_offset = 0u64;

        for chunk_id in &manifest.chunks {
            let location = self
                .engine
                .segment_store()
                .get_location(chunk_id)
                .ok_or_else(|| OosLiteError::ChunkNotFound(chunk_id.to_string()))?;

            let chunk_len = location.raw_len as u64;
            let chunk_end = current_offset + chunk_len;

            // Check if chunk intersects with [offset, end_offset)
            if chunk_end > offset && current_offset < end_offset {
                // Fetch from LRU cache or read from segment store
                let chunk_data = if let Some(cached) = self.chunk_cache.get(chunk_id) {
                    cached
                } else {
                    let decompressed = self.engine.segment_store().get_chunk(chunk_id)?;
                    self.chunk_cache.insert(*chunk_id, decompressed)
                };

                let slice_start = if offset > current_offset {
                    (offset - current_offset) as usize
                } else {
                    0
                };

                let slice_end = if end_offset < chunk_end {
                    (end_offset - current_offset) as usize
                } else {
                    chunk_data.len()
                };

                if slice_start < chunk_data.len() && slice_end <= chunk_data.len() && slice_start < slice_end {
                    result.extend_from_slice(&chunk_data[slice_start..slice_end]);
                }
            }

            current_offset = chunk_end;
            if current_offset >= end_offset {
                break;
            }
        }

        Ok(result)
    }

    /// Returns cache statistics.
    pub fn cache_bytes(&self) -> usize {
        self.chunk_cache.current_bytes()
    }

    pub fn cache_chunks(&self) -> usize {
        self.chunk_cache.chunk_count()
    }
}
