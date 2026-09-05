use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

use oos_lite_core::StorageEngine;

#[test]
fn test_milestone4_name_index_and_versioning() {
    let dir = tempdir().expect("tempdir failed");
    let store_dir = dir.path().join("store");

    // 1. Create 2 distinct files
    let file_v1_path = dir.path().join("v1.txt");
    let file_v2_path = dir.path().join("v2.txt");

    let content_v1 = b"Hello OOS-Lite Version 1 content payload";
    let content_v2 = b"Hello OOS-Lite Version 2 modified content payload with different size and data";

    {
        let mut f = File::create(&file_v1_path).unwrap();
        f.write_all(content_v1).unwrap();
    }
    {
        let mut f = File::create(&file_v2_path).unwrap();
        f.write_all(content_v2).unwrap();
    }

    let logical_name = "a.txt";
    let object_id;
    let manifest_v1_id;
    let manifest_v2_id;

    // 2. First put: "a.txt" -> version 1
    {
        let engine = StorageEngine::open(&store_dir).expect("engine open failed");
        let res1 = engine.put_file_named(logical_name, &file_v1_path).expect("put v1 failed");
        assert_eq!(res1.version, 1);
        object_id = res1.object_id;
        manifest_v1_id = res1.manifest_id;

        // Verify "a.txt" extracts version 1
        let out_v1 = dir.path().join("out_v1.txt");
        engine.get_file(logical_name, &out_v1).expect("get v1 failed");
        let read_v1 = std::fs::read(&out_v1).unwrap();
        assert_eq!(read_v1, content_v1);

        // Verify versions API has 1 version
        let versions = engine.get_versions(logical_name).expect("get_versions failed");
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].version, 1);
        assert_eq!(versions[0].manifest_id, manifest_v1_id);
    }

    // 3. Second put: "a.txt" -> version 2
    {
        let engine = StorageEngine::open(&store_dir).expect("engine reopen failed");
        let res2 = engine.put_file_named(logical_name, &file_v2_path).expect("put v2 failed");
        assert_eq!(res2.version, 2);
        assert_eq!(res2.object_id, object_id, "ObjectID must remain consistent across updates");
        manifest_v2_id = res2.manifest_id;
        assert_ne!(manifest_v1_id, manifest_v2_id, "Manifests must be distinct for different content");

        // Verify Name Index now points to latest version (version 2)
        let out_v2 = dir.path().join("out_v2.txt");
        engine.get_file(logical_name, &out_v2).expect("get latest failed");
        let read_v2 = std::fs::read(&out_v2).unwrap();
        assert_eq!(read_v2, content_v2, "Name Index must point to the latest version (v2)");

        // DoD check: versions API must return BOTH versions
        let versions = engine.get_versions(logical_name).expect("get_versions failed");
        assert_eq!(versions.len(), 2, "DoD: versions API must return both versions");
        assert_eq!(versions[0].version, 1);
        assert_eq!(versions[0].manifest_id, manifest_v1_id);
        assert_eq!(versions[1].version, 2);
        assert_eq!(versions[1].manifest_id, manifest_v2_id);

        // Verify versions query also works by ObjectId string
        let versions_by_id = engine.get_versions(&object_id.to_hex()).expect("get_versions by id failed");
        assert_eq!(versions_by_id.len(), 2);
    }

    // 4. Test Persistence Across Cold Restart
    {
        let engine_restarted = StorageEngine::open(&store_dir).expect("restart open failed");

        // Verify list_files
        let files = engine_restarted.list_files().expect("list_files failed");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, logical_name);
        assert_eq!(files[0].1, object_id);
        assert_eq!(files[0].2.latest_version, 2);

        // Verify get by name still yields latest version (v2)
        let out_restart = dir.path().join("out_restart.txt");
        engine_restarted.get_file(logical_name, &out_restart).expect("get after restart failed");
        assert_eq!(std::fs::read(&out_restart).unwrap(), content_v2);

        // Verify versions after restart
        let versions = engine_restarted.get_versions(logical_name).expect("versions after restart failed");
        assert_eq!(versions.len(), 2);
    }
}

// =========================================================================
// Milestone 6: Snapshot & Restore E2E + 1GB Latency Tests
// =========================================================================

#[test]
fn test_milestone6_e2e_snapshot_and_restore() {
    let dir = tempdir().expect("tempdir failed");
    let store_dir = dir.path().join("snap_store");
    let engine = StorageEngine::open(&store_dir).expect("engine open failed");

    let f1_v1 = dir.path().join("f1_v1.txt");
    let f1_v2 = dir.path().join("f1_v2.txt");
    let f2 = dir.path().join("f2.txt");

    let content_v1 = b"Milestone 6 Snapshot Test: Original Version 1 content of file1";
    let content_v2 = b"Milestone 6 Snapshot Test: Overwritten Version 2 content of file1 with different length";
    let content_f2 = b"Content of file 2 added after snapshot A was taken";

    std::fs::write(&f1_v1, content_v1).unwrap();
    std::fs::write(&f1_v2, content_v2).unwrap();
    std::fs::write(&f2, content_f2).unwrap();

    // 1. Put file1 v1
    engine.put_file_named("file1.txt", &f1_v1).unwrap();

    // 2. Take Snapshot A
    let snap_a = engine.create_snapshot("snapshot_A").expect("create snapshot A failed");
    assert_eq!(snap_a.label, "snapshot_A");
    assert_eq!(snap_a.entries.len(), 1);
    assert_eq!(snap_a.entries[0].name, "file1.txt");
    assert_eq!(snap_a.entries[0].version, 1);

    // 3. Put file1 v2 and put file2
    engine.put_file_named("file1.txt", &f1_v2).unwrap();
    engine.put_file_named("sub/file2.txt", &f2).unwrap();

    // Verify current state is v2 for file1 and has 2 files
    let current_out = dir.path().join("current_f1.txt");
    engine.get_file("file1.txt", &current_out).unwrap();
    assert_eq!(std::fs::read(&current_out).unwrap(), content_v2);

    let all_files = engine.list_files().unwrap();
    assert_eq!(all_files.len(), 2);

    // 4. Restore Snapshot A into a fresh directory
    let restore_dir = dir.path().join("restored_snapshot_A");
    let restored_count = engine.restore_snapshot("snapshot_A", &restore_dir).expect("restore failed");
    assert_eq!(restored_count, 1, "Snapshot A only contained 1 file");

    // Verify restored file1.txt matches v1 100% byte-for-byte!
    let restored_f1_path = restore_dir.join("file1.txt");
    assert!(restored_f1_path.exists(), "Restored file1.txt must exist");
    let restored_f1_bytes = std::fs::read(&restored_f1_path).unwrap();
    assert_eq!(restored_f1_bytes, content_v1, "Restored content must match version 1 byte-for-byte!");

    // Verify file2.txt was NOT restored in snapshot A
    assert!(!restore_dir.join("sub/file2.txt").exists(), "file2 must not exist in snapshot A");

    // 5. Test persistence across engine restart
    drop(engine);
    let engine2 = StorageEngine::open(&store_dir).unwrap();
    let snapshots = engine2.list_snapshots().unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].label, "snapshot_A");
    assert_eq!(engine2.stats().total_snapshots, 1);
}

#[test]
fn test_milestone6_snapshot_1gb_latency() {
    use std::time::Instant;
    use oos_lite_core::{ChunkId, Manifest, ObjectId, ObjectRecord};

    let dir = tempdir().expect("tempdir failed");
    let store_dir = dir.path().join("1gb_store");
    let engine = StorageEngine::open(&store_dir).expect("engine open failed");

    let one_gb: u64 = 1024 * 1024 * 1024; // 1 GiB
    let num_chunks = 16384; // 16,384 chunks * 64 KiB = 1 GiB

    // Generate 16,384 distinct random chunk IDs representing 1 GiB of high-entropy data
    let mut rng: u64 = 0xDEADBEEFCAFEBABE;
    let mut chunk_ids = Vec::with_capacity(num_chunks);
    for _ in 0..num_chunks {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let mut raw_hash = [0u8; 32];
        raw_hash[0..8].copy_from_slice(&rng.to_le_bytes());
        raw_hash[8..16].copy_from_slice(&(!rng).to_le_bytes());
        chunk_ids.push(ChunkId::from_raw(raw_hash));
    }

    let manifest = Manifest::new(chunk_ids, one_gb, [0x5A; 32]);
    let manifest_id = engine.metadata_store().save_manifest(&manifest).unwrap();

    let object_id = ObjectId::generate();
    let record = ObjectRecord::new(object_id, manifest_id, one_gb);

    engine.metadata_store().bind_name("large_file_1gb.bin", &object_id).unwrap();
    engine.metadata_store().put_object(&record).unwrap();
    engine.metadata_store().flush().unwrap();

    // DoD Requirement: Measure snapshot latency on 1 GB file, MUST BE < 10ms!
    let start = Instant::now();
    let snap = engine.create_snapshot("1gb_snapshot").expect("create snapshot on 1gb file failed");
    let elapsed = start.elapsed();

    println!("==> 1 GiB Snapshot creation latency: {:.2?}", elapsed);
    assert_eq!(snap.entries.len(), 1);
    assert_eq!(snap.entries[0].name, "large_file_1gb.bin");
    assert_eq!(snap.entries[0].size_bytes, one_gb);

    // DoD Verification: latency must strictly be < 10ms
    assert!(
        elapsed < std::time::Duration::from_millis(10),
        "Snapshot latency exceeded 10ms: {:?}",
        elapsed
    );
}

// =========================================================================
// Milestone 7: Garbage Collection (GC) Shared Chunks Graph Tests
// =========================================================================

#[test]
fn test_milestone7_gc_shared_chunks_graph() {
    let dir = tempdir().expect("tempdir failed");
    let store_dir = dir.path().join("gc_store");
    let engine = StorageEngine::open(&store_dir).expect("engine open failed");

    // 1. Create distinct data blocks + shared data block (each 64 KiB)
    let make_block = |seed: u64, size: usize| -> Vec<u8> {
        let mut rng = seed;
        let mut block = vec![0u8; size];
        for chunk in block.chunks_exact_mut(8) {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            chunk.copy_from_slice(&rng.to_le_bytes());
        }
        block
    };

    let block_shared = make_block(0xAAAAAAAA11111111, 1024 * 1024);
    let block_a = make_block(0xBBBBBBBB22222222, 512 * 1024);
    let block_b = make_block(0xCCCCCCCC33333333, 512 * 1024);

    let mut file_a_data = Vec::new();
    file_a_data.extend_from_slice(&block_shared);
    file_a_data.extend_from_slice(&block_a);

    let mut file_b_data = Vec::new();
    file_b_data.extend_from_slice(&block_shared);
    file_b_data.extend_from_slice(&block_b);

    let path_a = dir.path().join("file_A.bin");
    let path_b = dir.path().join("file_B.bin");
    std::fs::write(&path_a, &file_a_data).unwrap();
    std::fs::write(&path_b, &file_b_data).unwrap();

    // 2. Put File A and File B
    let put_a = engine.put_file_named("file_A.bin", &path_a).unwrap();
    let put_b = engine.put_file_named("file_B.bin", &path_b).unwrap();

    let manifest_a = engine.metadata_store().get_manifest(&put_a.manifest_id).unwrap().unwrap();
    let manifest_b = engine.metadata_store().get_manifest(&put_b.manifest_id).unwrap().unwrap();

    // Identify shared chunk and exclusive chunks
    let chunks_a_set: std::collections::HashSet<_> = manifest_a.chunks.iter().copied().collect();
    let chunks_b_set: std::collections::HashSet<_> = manifest_b.chunks.iter().copied().collect();

    let shared_chunks: Vec<_> = chunks_a_set.intersection(&chunks_b_set).copied().collect();
    let exclusive_a_chunks: Vec<_> = chunks_a_set.difference(&chunks_b_set).copied().collect();
    let exclusive_b_chunks: Vec<_> = chunks_b_set.difference(&chunks_a_set).copied().collect();

    assert!(!shared_chunks.is_empty(), "Must have shared chunk(s) between A and B");
    assert!(!exclusive_a_chunks.is_empty(), "Must have exclusive chunk(s) in A");
    assert!(!exclusive_b_chunks.is_empty(), "Must have exclusive chunk(s) in B");

    let shared_chunk_id = shared_chunks[0];
    let exclusive_a_chunk_id = exclusive_a_chunks[0];
    let exclusive_b_chunk_id = exclusive_b_chunks[0];

    assert!(engine.segment_store().has_chunk(&shared_chunk_id));
    assert!(engine.segment_store().has_chunk(&exclusive_a_chunk_id));
    assert!(engine.segment_store().has_chunk(&exclusive_b_chunk_id));

    // 3. Delete File A
    let deleted_a = engine.delete_file("file_A.bin").unwrap();
    assert!(deleted_a, "File A must be successfully unlinked");

    // 4. Run GC cycle 1
    let gc_stats1 = engine.gc().unwrap();
    println!("GC Run 1 after deleting A: {:?}", gc_stats1);
    assert!(gc_stats1.chunks_reclaimed >= exclusive_a_chunks.len());

    // === CRITICAL DoD CHECK 1 ===
    // Verify shared chunk is NOT GC'd because File B is still referencing it!
    assert!(
        engine.segment_store().has_chunk(&shared_chunk_id),
        "CRITICAL: Shared chunk MUST NOT be GC'd while still referenced by File B!"
    );

    // Verify exclusive chunk of A IS reclaimed
    assert!(
        !engine.segment_store().has_chunk(&exclusive_a_chunk_id),
        "Exclusive chunk of deleted File A must be reclaimed"
    );

    // Verify File B is still 100% intact and extractable
    let out_b = dir.path().join("extracted_file_B.bin");
    let bytes_b = engine.get_file("file_B.bin", &out_b).unwrap();
    assert_eq!(bytes_b, file_b_data.len() as u64);
    assert_eq!(std::fs::read(&out_b).unwrap(), file_b_data);

    // Verify disk compaction survives process restart
    drop(engine);
    let engine = StorageEngine::open(&store_dir).expect("reopen after GC 1 failed");
    assert!(
        engine.segment_store().has_chunk(&shared_chunk_id),
        "Shared chunk must survive restart"
    );
    assert!(
        !engine.segment_store().has_chunk(&exclusive_a_chunk_id),
        "Exclusive chunk of deleted File A must stay deleted across restart"
    );
    let out_b_restart = dir.path().join("extracted_file_B_restart.bin");
    let bytes_b_restart = engine.get_file("file_B.bin", &out_b_restart).unwrap();
    assert_eq!(bytes_b_restart, file_b_data.len() as u64);
    assert_eq!(std::fs::read(&out_b_restart).unwrap(), file_b_data);

    // 5. Now delete File B
    let deleted_b = engine.delete_file("file_B.bin").unwrap();
    assert!(deleted_b, "File B must be successfully unlinked");

    // 6. Run GC cycle 2
    let gc_stats2 = engine.gc().unwrap();
    println!("GC Run 2 after deleting B: {:?}", gc_stats2);
    assert!(gc_stats2.chunks_reclaimed >= 1);

    // === CRITICAL DoD CHECK 2 ===
    // Verify shared chunk IS NOW RECLAIMED because no file is referencing it anymore!
    assert!(
        !engine.segment_store().has_chunk(&shared_chunk_id),
        "CRITICAL: Shared chunk MUST be reclaimed now that both A and B are deleted!"
    );
    assert!(!engine.segment_store().has_chunk(&exclusive_b_chunk_id));

    // Verify all chunks cleared on disk across restart
    drop(engine);
    let engine2 = StorageEngine::open(&store_dir).expect("reopen after GC 2 failed");
    assert!(!engine2.segment_store().has_chunk(&shared_chunk_id));
    assert!(!engine2.segment_store().has_chunk(&exclusive_b_chunk_id));
    assert_eq!(engine2.segment_store().chunk_count(), 0);
}

#[test]
fn test_concurrent_put_and_gc_no_data_loss() {
    use std::sync::Arc;
    use std::thread;

    let dir = tempdir().expect("tempdir failed");
    let store_dir = dir.path().join("store");
    let engine = Arc::new(StorageEngine::open(&store_dir).expect("engine open failed"));

    // Prepare 10 distinct files
    let mut file_paths = Vec::new();
    let mut file_payloads = Vec::new();
    for i in 0..10 {
        let p = dir.path().join(format!("test_input_{}.bin", i));
        let mut data = Vec::with_capacity(128 * 1024);
        for j in 0..(128 * 1024) {
            data.push(((i * 73 + j * 17) % 256) as u8);
        }
        std::fs::write(&p, &data).unwrap();
        file_paths.push(p);
        file_payloads.push(data);
    }

    let file_paths = Arc::new(file_paths);
    let mut handles = Vec::new();

    // Spawn 4 concurrent writers
    for worker_id in 0..4 {
        let engine_clone = Arc::clone(&engine);
        let paths_clone = Arc::clone(&file_paths);
        handles.push(thread::spawn(move || {
            for round in 0..5 {
                let file_idx = (worker_id * 2 + round) % paths_clone.len();
                let name = format!("worker_{}_round_{}.bin", worker_id, round);
                let _ = engine_clone.put_file_named(&name, &paths_clone[file_idx]).unwrap();
            }
        }));
    }

    // Spawn concurrent GC thread
    let gc_engine = Arc::clone(&engine);
    let gc_handle = thread::spawn(move || {
        for _ in 0..4 {
            thread::sleep(std::time::Duration::from_millis(10));
            let _ = gc_engine.gc();
        }
    });

    for h in handles {
        h.join().unwrap();
    }
    gc_handle.join().unwrap();

    // Verify all stored files can be extracted with 100% integrity
    for worker_id in 0..4 {
        for round in 0..5 {
            let file_idx = (worker_id * 2 + round) % 10;
            let name = format!("worker_{}_round_{}.bin", worker_id, round);
            let out_path = dir.path().join(format!("extracted_{}", name));
            let bytes = engine.get_file(&name, &out_path).unwrap();
            assert_eq!(bytes, file_payloads[file_idx].len() as u64);
            assert_eq!(std::fs::read(&out_path).unwrap(), file_payloads[file_idx]);
        }
    }

    // Run fsck to prove store is 100% healthy
    let report = engine.fsck().unwrap();
    assert!(report.is_healthy, "Store must be completely healthy after concurrent put and gc");
    assert_eq!(report.corrupted_chunks, 0);
    assert_eq!(report.missing_chunks, 0);
}



