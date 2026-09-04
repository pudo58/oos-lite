use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use tempfile::tempdir;

use oos_lite_core::StorageEngine;

#[test]
fn test_milestone8_fsck_clean_store() {
    let dir = tempdir().expect("tempdir failed");
    let store_dir = dir.path().join("store");
    let engine = StorageEngine::open(&store_dir).expect("engine open failed");

    // Put 3 distinct files
    let f1_path = dir.path().join("file1.txt");
    let f2_path = dir.path().join("file2.txt");
    let f3_path = dir.path().join("file3.txt");

    std::fs::write(&f1_path, b"Alpha file content 1234567890").unwrap();
    std::fs::write(&f2_path, b"Beta file content ABCDEFGHIJKLMNOP").unwrap();
    std::fs::write(&f3_path, b"Gamma file content !@#$%^&*()_+").unwrap();

    engine.put_file_named("file1.txt", &f1_path).unwrap();
    engine.put_file_named("file2.txt", &f2_path).unwrap();
    engine.put_file_named("file3.txt", &f3_path).unwrap();

    // Create a snapshot
    engine.create_snapshot("snap_v1").unwrap();

    // Run FSCK
    let report = engine.fsck().expect("fsck run failed");
    println!("FSCK Report on clean store: {:?}", report);

    assert!(report.is_healthy, "Clean store must be 100% healthy");
    assert_eq!(report.corrupted_chunks, 0);
    assert_eq!(report.missing_chunks, 0);
    assert!(report.errors.is_empty());
    assert!(report.segments_checked >= 1);
    assert!(report.chunks_checked >= 3);
    assert!(report.manifests_checked >= 3);
    assert_eq!(report.objects_checked, 3);
}

#[test]
fn test_milestone8_fsck_detects_corrupted_chunk() {
    let dir = tempdir().expect("tempdir failed");
    let store_dir = dir.path().join("store");
    let engine = StorageEngine::open(&store_dir).expect("engine open failed");

    let f_path = dir.path().join("target.bin");
    let data = vec![0xAB; 64 * 1024]; // 64 KiB
    std::fs::write(&f_path, &data).unwrap();

    engine.put_file_named("target.bin", &f_path).unwrap();

    // Verify initial clean fsck
    let report_clean = engine.fsck().unwrap();
    assert!(report_clean.is_healthy);

    // Deliberately corrupt 1 byte in the middle of segment_00000001.seg
    let seg_path = store_dir.join("segments").join("segment_00000001.seg");
    assert!(seg_path.exists(), "segment_00000001.seg must exist");

    {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&seg_path)
            .expect("open seg for corruption failed");
        // Seek into chunk payload (past 32-byte segment header and 48-byte record header)
        let corrupt_offset = 32 + 48 + 10;
        file.seek(SeekFrom::Start(corrupt_offset)).unwrap();
        let mut byte = [0u8; 1];
        file.read_exact(&mut byte).unwrap();
        byte[0] ^= 0xFF; // Flip all bits
        file.seek(SeekFrom::Start(corrupt_offset)).unwrap();
        file.write_all(&byte).unwrap();
        file.sync_all().unwrap();
    }

    // Run FSCK again - must detect corruption!
    let report_corrupted = engine.fsck().unwrap();
    println!("FSCK Report on corrupted store: {:?}", report_corrupted);

    assert!(!report_corrupted.is_healthy, "FSCK must detect corruption");
    assert!(report_corrupted.corrupted_chunks > 0, "Corrupted chunk count must be > 0");
    assert!(!report_corrupted.errors.is_empty(), "Errors list must contain corruption details");
}

#[test]
fn test_milestone8_stats_dedup_ratio_calculation() {
    let dir = tempdir().expect("tempdir failed");
    let store_dir = dir.path().join("store");
    let engine = StorageEngine::open(&store_dir).expect("engine open failed");

    // Create 128 KiB identical content for two files
    let data = vec![0x42; 128 * 1024];
    let path_a = dir.path().join("file_a.bin");
    let path_b = dir.path().join("file_b.bin");
    std::fs::write(&path_a, &data).unwrap();
    std::fs::write(&path_b, &data).unwrap();

    engine.put_file_named("file_a.bin", &path_a).unwrap();
    engine.put_file_named("file_b.bin", &path_b).unwrap();

    let stats = engine.stats();
    println!("Stats after dedup files: {:?}", stats);

    assert_eq!(stats.total_objects, 2);
    assert_eq!(stats.logical_bytes, 256 * 1024);
    assert_eq!(stats.latest_logical_bytes, 256 * 1024);
    assert_eq!(stats.unique_chunks_bytes, 128 * 1024);
    assert!(stats.dedup_ratio >= 1.99 && stats.dedup_ratio <= 2.01, "Dedup ratio should be ~2.0x");
    assert!(stats.space_savings_pct >= 49.0 && stats.space_savings_pct <= 51.0, "Space savings should be ~50%");
    assert!(stats.physical_disk_bytes > 0, "Physical disk usage should be non-zero");
}

#[test]
fn test_milestone8_get_specific_version() {
    let dir = tempdir().expect("tempdir failed");
    let store_dir = dir.path().join("store");
    let engine = StorageEngine::open(&store_dir).expect("engine open failed");

    let f_path = dir.path().join("versioned.txt");

    // Version 1
    std::fs::write(&f_path, b"Version 1 content here").unwrap();
    engine.put_file_named("doc.txt", &f_path).unwrap();

    // Version 2
    std::fs::write(&f_path, b"Version 2 updated content!").unwrap();
    engine.put_file_named("doc.txt", &f_path).unwrap();

    // Extract version 1 explicitly
    let out_v1 = dir.path().join("out_v1.txt");
    let b1 = engine.get_file_version("doc.txt", Some(1), &out_v1).unwrap();
    assert_eq!(b1, b"Version 1 content here".len() as u64);
    assert_eq!(std::fs::read(&out_v1).unwrap(), b"Version 1 content here");

    // Extract version 2 explicitly
    let out_v2 = dir.path().join("out_v2.txt");
    let b2 = engine.get_file_version("doc.txt", Some(2), &out_v2).unwrap();
    assert_eq!(b2, b"Version 2 updated content!".len() as u64);
    assert_eq!(std::fs::read(&out_v2).unwrap(), b"Version 2 updated content!");

    // Extract latest (None)
    let out_latest = dir.path().join("out_latest.txt");
    let b_latest = engine.get_file("doc.txt", &out_latest).unwrap();
    assert_eq!(b_latest, b"Version 2 updated content!".len() as u64);
    assert_eq!(std::fs::read(&out_latest).unwrap(), b"Version 2 updated content!");

    // Attempt to extract non-existent version 99 -> must error cleanly
    let out_v99 = dir.path().join("out_v99.txt");
    let err = engine.get_file_version("doc.txt", Some(99), &out_v99);
    assert!(err.is_err());
}
