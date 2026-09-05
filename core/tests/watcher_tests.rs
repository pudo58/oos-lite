use std::fs;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

use oos_lite_core::watcher::{WatcherConfig, WatcherService};
use oos_lite_core::StorageEngine;

#[test]
fn test_watcher_debounce_and_auto_put() {
    let watch_dir = tempdir().unwrap();
    let store_dir = tempdir().unwrap();

    let engine = Arc::new(StorageEngine::open(store_dir.path()).unwrap());
    let config = WatcherConfig::new(watch_dir.path())
        .with_debounce(Duration::from_millis(400))
        .with_cooldown(Duration::from_millis(800));

    let service = WatcherService::new(Arc::clone(&engine), config);
    let handle = service.start().unwrap();

    // 1. Create a file and rapidly write to it 3 times within 200ms
    let file_path = watch_dir.path().join("notes.txt");
    fs::write(&file_path, b"Line 1\n").unwrap();
    thread::sleep(Duration::from_millis(50));
    fs::write(&file_path, b"Line 1\nLine 2\n").unwrap();
    thread::sleep(Duration::from_millis(50));
    fs::write(&file_path, b"Line 1\nLine 2\nLine 3 final\n").unwrap();

    // 2. Wait for debounce (400ms) + margin
    thread::sleep(Duration::from_millis(700));

    // Verify engine has exactly 1 version with the final content
    let versions = engine.get_versions("notes.txt").unwrap();
    assert_eq!(versions.len(), 1);
    let out = store_dir.path().join("out.txt");
    engine.get_file("notes.txt", &out).unwrap();
    assert_eq!(fs::read(&out).unwrap(), b"Line 1\nLine 2\nLine 3 final\n");

    // 3. Wait until cooldown expires, then modify again
    thread::sleep(Duration::from_millis(900));
    fs::write(&file_path, b"Line 1\nLine 2\nLine 3 final\nLine 4 new version\n").unwrap();
    thread::sleep(Duration::from_millis(700));

    // Verify engine now has version 2
    let versions2 = engine.get_versions("notes.txt").unwrap();
    assert_eq!(versions2.len(), 2);

    handle.stop();
}

#[test]
fn test_watcher_ignore_rules_and_oosignore() {
    let watch_dir = tempdir().unwrap();
    let store_dir = tempdir().unwrap();

    // Write custom .oosignore
    let oosignore = watch_dir.path().join(".oosignore");
    fs::write(
        &oosignore,
        "# Custom project ignores\nbuild_out/\n*.secret_cache\n",
    )
    .unwrap();

    // Create ignored files:
    // Built-in ignores: ~$lock.docx, temp_file.tmp, .git/something
    fs::write(watch_dir.path().join("~$lock.docx"), b"office lock").unwrap();
    fs::write(watch_dir.path().join("temp_file.tmp"), b"temp data").unwrap();

    let build_out = watch_dir.path().join("build_out");
    fs::create_dir_all(&build_out).unwrap();
    fs::write(build_out.join("binary.bin"), b"build artifact").unwrap();

    fs::write(watch_dir.path().join("cache.secret_cache"), b"cache").unwrap();

    // Valid file
    fs::write(watch_dir.path().join("valid_document.pdf"), b"real work content").unwrap();

    let engine = Arc::new(StorageEngine::open(store_dir.path()).unwrap());
    let config = WatcherConfig::new(watch_dir.path());

    let service = WatcherService::new(Arc::clone(&engine), config);
    // Trigger cold-start reconciliation scan
    service.reconciliation_scan().unwrap();

    let stored = engine.list_files().unwrap();
    let names: Vec<String> = stored.into_iter().map(|(n, _, _)| n).collect();

    assert!(names.contains(&"valid_document.pdf".to_string()));
    assert!(!names.contains(&"~$lock.docx".to_string()));
    assert!(!names.contains(&"temp_file.tmp".to_string()));
    assert!(!names.contains(&"cache.secret_cache".to_string()));
    assert!(!names.contains(&"build_out/binary.bin".to_string()));
    assert!(!names.contains(&".oosignore".to_string()));
}

#[test]
fn test_watcher_rename_preserves_version_history() {
    let watch_dir = tempdir().unwrap();
    let store_dir = tempdir().unwrap();

    let engine = Arc::new(StorageEngine::open(store_dir.path()).unwrap());
    let config = WatcherConfig::new(watch_dir.path())
        .with_debounce(Duration::from_millis(300))
        .with_cooldown(Duration::from_millis(400));

    let service = WatcherService::new(Arc::clone(&engine), config);
    let handle = service.start().unwrap();

    let old_file = watch_dir.path().join("draft_report.docx");
    fs::write(&old_file, b"First draft content").unwrap();
    thread::sleep(Duration::from_millis(500));

    // Update draft to get version 2
    thread::sleep(Duration::from_millis(500));
    fs::write(&old_file, b"Second draft updated content").unwrap();
    thread::sleep(Duration::from_millis(500));

    let v_old = engine.get_versions("draft_report.docx").unwrap();
    assert_eq!(v_old.len(), 2);

    // Now rename draft_report.docx -> final_report.docx
    let new_file = watch_dir.path().join("final_report.docx");
    fs::rename(&old_file, &new_file).unwrap();
    thread::sleep(Duration::from_millis(600));

    // Under the new name final_report.docx, both version 1 and 2 must be preserved!
    let v_new = engine.get_versions("final_report.docx");
    assert!(v_new.is_ok(), "Expected final_report.docx to exist");
    let versions = v_new.unwrap();
    assert_eq!(versions.len(), 2, "Must preserve previous 2 versions under new name!");

    // And old name must no longer be directly bound
    assert!(engine.get_versions("draft_report.docx").is_err());

    handle.stop();
}

#[test]
fn test_reconciliation_scanner_cold_start() {
    let watch_dir = tempdir().unwrap();
    let store_dir = tempdir().unwrap();

    // Populate directory with 15 files before starting
    for i in 0..15 {
        let p = watch_dir.path().join(format!("file_{:02}.txt", i));
        fs::write(&p, format!("Content for file {}", i).as_bytes()).unwrap();
    }

    let engine = Arc::new(StorageEngine::open(store_dir.path()).unwrap());
    let config = WatcherConfig::new(watch_dir.path()).with_throttle_ms(5);

    let service = WatcherService::new(Arc::clone(&engine), config);
    service.reconciliation_scan().unwrap();

    let stored = engine.list_files().unwrap();
    assert_eq!(stored.len(), 15);
}

#[test]
fn test_prune_file_versions_and_gc() {
    let store_dir = tempdir().unwrap();
    let engine = StorageEngine::open(store_dir.path()).unwrap();

    let tmp = store_dir.path().join("sample.txt");

    // Put 6 different versions
    for i in 1..=6 {
        fs::write(&tmp, format!("Version {} distinct chunk payload content", i)).unwrap();
        engine.put_file_named("sample.txt", &tmp).unwrap();
    }

    let versions_before = engine.get_versions("sample.txt").unwrap();
    assert_eq!(versions_before.len(), 6);

    // Prune to keep only 2 latest versions
    let pruned = engine.prune_file_versions("sample.txt", 2).unwrap();
    assert_eq!(pruned, 4);

    let versions_after = engine.get_versions("sample.txt").unwrap();
    assert_eq!(versions_after.len(), 2);
    assert_eq!(versions_after[0].version, 5);
    assert_eq!(versions_after[1].version, 6);

    // Run GC to reclaim chunks from pruned versions 1..4
    let gc_stats = engine.gc().unwrap();
    assert!(gc_stats.chunks_reclaimed > 0);
}
