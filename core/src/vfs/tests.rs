use std::sync::Arc;
use tempfile::tempdir;
use crate::engine::StorageEngine;
use crate::vfs::tree::{VfsNodeType, VfsTree, CURRENT_INODE, HISTORY_INODE, ROOT_INODE, SNAPSHOTS_INODE};

#[test]
fn test_vfs_tree_structure() {
    let dir = tempdir().expect("tempdir failed");
    let store_dir = dir.path().join("store");
    let engine = Arc::new(StorageEngine::open(&store_dir).expect("engine open failed"));

    // Put a file with nested path
    let f1_path = dir.path().join("f1.txt");
    std::fs::write(&f1_path, b"Hello from nested file!").unwrap();
    engine.put_file_named("nested/docs/f1.txt", &f1_path).unwrap();

    let f2_path = dir.path().join("f2.txt");
    std::fs::write(&f2_path, b"Top level file").unwrap();
    engine.put_file_named("top.txt", &f2_path).unwrap();

    // Create snapshot
    engine.create_snapshot("release_v1").unwrap();

    // Build VFS Tree
    let vfs = VfsTree::build(Arc::clone(&engine), 64 * 1024 * 1024).expect("vfs build failed");

    // 1. Root inspection
    let root = vfs.get_node(ROOT_INODE).expect("root exists");
    assert_eq!(root.kind, VfsNodeType::Directory);
    let root_children = vfs.readdir(ROOT_INODE).unwrap();
    let root_names: Vec<&str> = root_children.iter().map(|n| n.name.as_str()).collect();
    assert!(root_names.contains(&"current"));
    assert!(root_names.contains(&"snapshots"));
    assert!(root_names.contains(&"history"));

    // 2. /current inspection
    let current_node = vfs.lookup_child(ROOT_INODE, "current").expect("current child");
    assert_eq!(current_node.ino, CURRENT_INODE);

    let top_file = vfs.lookup_child(CURRENT_INODE, "top.txt").expect("top.txt in current");
    assert_eq!(top_file.kind, VfsNodeType::RegularFile);
    assert_eq!(top_file.size, 14);

    let nested_dir = vfs.lookup_child(CURRENT_INODE, "nested").expect("nested in current");
    assert_eq!(nested_dir.kind, VfsNodeType::Directory);

    let docs_dir = vfs.lookup_child(nested_dir.ino, "docs").expect("docs in nested");
    assert_eq!(docs_dir.kind, VfsNodeType::Directory);

    let f1_file = vfs.lookup_child(docs_dir.ino, "f1.txt").expect("f1.txt in docs");
    assert_eq!(f1_file.kind, VfsNodeType::RegularFile);
    assert_eq!(f1_file.size, 23);

    // 3. /snapshots inspection
    let snap_dir = vfs.lookup_child(SNAPSHOTS_INODE, "release_v1").expect("release_v1 snapshot");
    assert_eq!(snap_dir.kind, VfsNodeType::Directory);
    let snap_top = vfs.lookup_child(snap_dir.ino, "top.txt").expect("top.txt in snapshot");
    assert_eq!(snap_top.size, 14);

    // 4. /history inspection
    let hist_nested = vfs.lookup_child(HISTORY_INODE, "nested").expect("nested in history");
    let hist_docs = vfs.lookup_child(hist_nested.ino, "docs").expect("docs in hist_nested");
    let v1_file = vfs.lookup_child(hist_docs.ino, "f1.txt@v1").expect("f1.txt@v1 in history");
    assert_eq!(v1_file.kind, VfsNodeType::RegularFile);
    let latest_symlink = vfs.lookup_child(hist_docs.ino, "f1.txt@latest").expect("f1.txt@latest symlink");
    assert_eq!(latest_symlink.kind, VfsNodeType::Symlink);
    assert_eq!(latest_symlink.symlink_target.as_deref(), Some("f1.txt@v1"));
}

#[test]
fn test_vfs_read_range_exact() {
    let dir = tempdir().expect("tempdir failed");
    let store_dir = dir.path().join("store");
    let engine = Arc::new(StorageEngine::open(&store_dir).expect("engine open failed"));

    // Create 1 MiB payload (spans ~16 FastCDC chunks)
    let mut original_data = Vec::with_capacity(1024 * 1024);
    let mut x: u32 = 987654321;
    for _ in 0..(1024 * 1024) {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        original_data.push((x & 0xFF) as u8);
    }

    let f_path = dir.path().join("multi_chunk.bin");
    std::fs::write(&f_path, &original_data).unwrap();
    let summary = engine.put_file_named("data.bin", &f_path).unwrap();
    assert!(summary.chunk_count >= 4, "File must span multiple chunks (got {})", summary.chunk_count);

    let vfs = VfsTree::build(Arc::clone(&engine), 128 * 1024 * 1024).unwrap();
    let manifest_id = &summary.manifest_id;

    // Test 1: Full file read via read_range
    let full = vfs.read_range(manifest_id, 0, original_data.len() as u64).unwrap();
    assert_eq!(full, original_data);

    // Test 2: Read sub-slice at beginning
    let head = vfs.read_range(manifest_id, 0, 100).unwrap();
    assert_eq!(head, &original_data[0..100]);

    // Test 3: Read middle range intersecting chunk boundary
    let mid_offset = 120 * 1024u64;
    let mid_size = 140 * 1024u64;
    let mid = vfs.read_range(manifest_id, mid_offset, mid_size).unwrap();
    assert_eq!(mid, &original_data[mid_offset as usize..(mid_offset + mid_size) as usize]);

    // Test 4: Read sub-slice at tail
    let tail_offset = original_data.len() as u64 - 500;
    let tail = vfs.read_range(manifest_id, tail_offset, 500).unwrap();
    assert_eq!(tail, &original_data[tail_offset as usize..]);

    // Test 5: Read past EOF returns empty
    let past = vfs.read_range(manifest_id, original_data.len() as u64 + 100, 50).unwrap();
    assert!(past.is_empty());
}

#[test]
fn test_vfs_lru_cache_memory_cap() {
    let dir = tempdir().expect("tempdir failed");
    let store_dir = dir.path().join("store");
    let engine = Arc::new(StorageEngine::open(&store_dir).expect("engine open failed"));

    // Write a 1 MiB file spanning ~16 chunks
    let mut data = Vec::with_capacity(1024 * 1024);
    let mut x: u32 = 123456789;
    for _ in 0..(1024 * 1024) {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        data.push((x & 0xFF) as u8);
    }

    let f_path = dir.path().join("stream.bin");
    std::fs::write(&f_path, &data).unwrap();
    let summary = engine.put_file_named("stream.bin", &f_path).unwrap();
    assert!(summary.chunk_count >= 4, "Must have multiple chunks for LRU test");

    // Set cache cap to 128 KiB (holds at most ~2 chunks)
    let cache_cap = 128 * 1024;
    let vfs = VfsTree::build(Arc::clone(&engine), cache_cap).unwrap();

    // Read through the entire 1 MiB file
    let _ = vfs.read_range(&summary.manifest_id, 0, data.len() as u64).unwrap();

    let current_cache = vfs.cache_bytes();
    println!("Total file read: 1 MB, Current cache bytes retained: {}", current_cache);

    // Cache must have evicted older chunks and stayed strictly bounded
    assert!(current_cache < 512 * 1024, "Cache memory must be strictly bounded below 512 KiB (got {})", current_cache);
    assert!(current_cache < data.len() / 2, "Cache must not retain the whole 1 MB file");
}


#[test]
fn test_vfs_history_naming_and_symlink() {
    let dir = tempdir().expect("tempdir failed");
    let store_dir = dir.path().join("store");
    let engine = Arc::new(StorageEngine::open(&store_dir).expect("engine open failed"));

    let f_path = dir.path().join("doc.txt");

    // Version 1
    std::fs::write(&f_path, b"Version 1 content").unwrap();
    engine.put_file_named("sub/doc.txt", &f_path).unwrap();

    // Version 2
    std::fs::write(&f_path, b"Version 2 updated content!!").unwrap();
    engine.put_file_named("sub/doc.txt", &f_path).unwrap();

    // Build VFS Tree
    let vfs = VfsTree::build(Arc::clone(&engine), 64 * 1024 * 1024).unwrap();

    let sub_dir = vfs.lookup_child(HISTORY_INODE, "sub").expect("sub in history");
    let v1_node = vfs.lookup_child(sub_dir.ino, "doc.txt@v1").expect("v1 node");
    let v2_node = vfs.lookup_child(sub_dir.ino, "doc.txt@v2").expect("v2 node");
    let latest_node = vfs.lookup_child(sub_dir.ino, "doc.txt@latest").expect("latest symlink");

    assert_eq!(v1_node.kind, VfsNodeType::RegularFile);
    assert_eq!(v2_node.kind, VfsNodeType::RegularFile);
    assert_eq!(latest_node.kind, VfsNodeType::Symlink);
    assert_eq!(latest_node.symlink_target.as_deref(), Some("doc.txt@v2"));

    // Verify reading v1 vs v2
    let v1_data = vfs.read_range(v1_node.manifest_id.as_ref().unwrap(), 0, 100).unwrap();
    assert_eq!(v1_data, b"Version 1 content");

    let v2_data = vfs.read_range(v2_node.manifest_id.as_ref().unwrap(), 0, 100).unwrap();
    assert_eq!(v2_data, b"Version 2 updated content!!");

    // Test resolve_path
    let resolved_root = vfs.resolve_path("/").expect("root resolved");
    assert_eq!(resolved_root.ino, 1);

    let resolved_current = vfs.resolve_path("/current/sub/doc.txt").expect("current resolved");
    assert_eq!(resolved_current.manifest_id, v2_node.manifest_id);

    let resolved_v1 = vfs.resolve_path("/history/sub/doc.txt@v1").expect("v1 resolved");
    assert_eq!(resolved_v1.ino, v1_node.ino);

    let resolved_latest = vfs.resolve_path("/history/sub/doc.txt@latest").expect("latest symlink resolved");
    assert_eq!(resolved_latest.ino, v2_node.ino);
}
