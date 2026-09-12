use std::fs;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tempfile::tempdir;

use oos_lite_core::watcher::{WatcherConfig, WatcherPhase, WatcherService};
use oos_lite_core::StorageEngine;

fn wait_until(message: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "Timed out: {message}");
        thread::sleep(Duration::from_millis(25));
    }
}

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

    wait_until("initial scan", || {
        handle.status().phase == WatcherPhase::Watching
    });

    // 1. Create a file and rapidly write to it 3 times within 200ms
    let file_path = watch_dir.path().join("notes.txt");
    fs::write(&file_path, b"Line 1\n").unwrap();
    thread::sleep(Duration::from_millis(50));
    fs::write(&file_path, b"Line 1\nLine 2\n").unwrap();
    thread::sleep(Duration::from_millis(50));
    fs::write(&file_path, b"Line 1\nLine 2\nLine 3 final\n").unwrap();

    wait_until("debounced ingest", || {
        engine.get_versions("notes.txt").is_ok()
    });

    // Verify engine has exactly 1 version with the final content
    let versions = engine.get_versions("notes.txt").unwrap();
    assert_eq!(versions.len(), 1);
    let out = store_dir.path().join("out.txt");
    engine.get_file("notes.txt", &out).unwrap();
    assert_eq!(fs::read(&out).unwrap(), b"Line 1\nLine 2\nLine 3 final\n");

    // A change during cooldown must eventually become a new version.
    fs::write(
        &file_path,
        b"Line 1\nLine 2\nLine 3 final\nLine 4 new version\n",
    )
    .unwrap();
    wait_until("second version after cooldown", || {
        engine
            .get_versions("notes.txt")
            .is_ok_and(|versions| versions.len() >= 2)
    });

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
    fs::write(
        watch_dir.path().join("valid_document.pdf"),
        b"real work content",
    )
    .unwrap();

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
    wait_until("initial scan", || {
        handle.status().phase == WatcherPhase::Watching
    });
    fs::write(&old_file, b"First draft content").unwrap();
    wait_until("first draft", || {
        engine.get_versions("draft_report.docx").is_ok()
    });

    // Update draft to get version 2
    fs::write(&old_file, b"Second draft updated content").unwrap();
    wait_until("second draft", || {
        engine
            .get_versions("draft_report.docx")
            .is_ok_and(|versions| versions.len() == 2)
    });

    let v_old = engine.get_versions("draft_report.docx").unwrap();
    assert_eq!(v_old.len(), 2);

    // Now rename draft_report.docx -> final_report.docx
    let new_file = watch_dir.path().join("final_report.docx");
    fs::rename(&old_file, &new_file).unwrap();
    wait_until("rename preserves history", || {
        engine
            .get_versions("final_report.docx")
            .is_ok_and(|versions| versions.len() >= 2)
    });

    // Under the new name final_report.docx, both version 1 and 2 must be preserved!
    let v_new = engine.get_versions("final_report.docx");
    assert!(v_new.is_ok(), "Expected final_report.docx to exist");
    let versions = v_new.unwrap();
    assert_eq!(
        versions.len(),
        2,
        "Must preserve previous 2 versions under new name!"
    );

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
fn test_reconciliation_detects_same_size_content_change() {
    let watch_dir = tempdir().unwrap();
    let store_dir = tempdir().unwrap();
    let file_path = watch_dir.path().join("same-size.txt");
    fs::write(&file_path, b"AAAA").unwrap();

    let engine = Arc::new(StorageEngine::open(store_dir.path()).unwrap());
    let config = WatcherConfig::new(watch_dir.path());
    let service = WatcherService::new(Arc::clone(&engine), config);
    service.reconciliation_scan().unwrap();

    fs::write(&file_path, b"BBBB").unwrap();
    service.reconciliation_scan().unwrap();

    let versions = engine.get_versions("same-size.txt").unwrap();
    assert_eq!(versions.len(), 2);
    let restored = store_dir.path().join("same-size-restored.txt");
    engine.get_file("same-size.txt", &restored).unwrap();
    assert_eq!(fs::read(restored).unwrap(), b"BBBB");
}

#[test]
fn test_reconciliation_removes_only_missing_watcher_managed_files() {
    let watch_dir = tempdir().unwrap();
    let store_dir = tempdir().unwrap();
    let external_dir = tempdir().unwrap();
    let watched_file = watch_dir.path().join("watched.txt");
    let manual_source = external_dir.path().join("manual.txt");
    fs::write(&watched_file, b"watched content").unwrap();
    fs::write(&manual_source, b"manual content").unwrap();

    let engine = Arc::new(StorageEngine::open(store_dir.path()).unwrap());
    engine.put_file_named("manual.txt", &manual_source).unwrap();
    let service = WatcherService::new(Arc::clone(&engine), WatcherConfig::new(watch_dir.path()));
    service.reconciliation_scan().unwrap();

    fs::remove_file(&watched_file).unwrap();
    service.reconciliation_scan().unwrap();

    let names: Vec<String> = engine
        .list_files()
        .unwrap()
        .into_iter()
        .map(|(name, _, _)| name)
        .collect();
    assert!(!names.contains(&"watched.txt".to_string()));
    assert!(names.contains(&"manual.txt".to_string()));
}

#[test]
fn test_watcher_rejects_store_overlap() {
    let watch_dir = tempdir().unwrap();
    let store_path = watch_dir.path().join("vault");
    let engine = Arc::new(StorageEngine::open(&store_path).unwrap());
    let service = WatcherService::new(Arc::clone(&engine), WatcherConfig::new(watch_dir.path()));

    let error = service.start().err().expect("overlap must be rejected");
    assert!(error.to_string().contains("must not overlap"));
}

#[test]
fn test_cold_scan_starts_in_background_and_is_cancellable() {
    let watch_dir = tempdir().unwrap();
    let store_dir = tempdir().unwrap();
    for i in 0..30 {
        fs::write(
            watch_dir.path().join(format!("file-{i:02}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }

    let engine = Arc::new(StorageEngine::open(store_dir.path()).unwrap());
    let service = WatcherService::new(
        Arc::clone(&engine),
        WatcherConfig::new(watch_dir.path()).with_throttle_ms(50),
    );

    let start = Instant::now();
    let handle = service.start().unwrap();
    assert!(start.elapsed() < Duration::from_millis(500));
    assert_eq!(handle.status().phase, WatcherPhase::Scanning);

    let stop = Instant::now();
    handle.stop();
    assert!(stop.elapsed() < Duration::from_millis(500));
}

#[test]
fn test_prune_file_versions_and_gc() {
    let store_dir = tempdir().unwrap();
    let engine = StorageEngine::open(store_dir.path()).unwrap();

    let tmp = store_dir.path().join("sample.txt");

    // Put 6 different versions
    for i in 1..=6 {
        fs::write(
            &tmp,
            format!("Version {} distinct chunk payload content", i),
        )
        .unwrap();
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

#[test]
fn relative_root_receives_absolute_filesystem_events() {
    let cwd = std::env::current_dir().unwrap();
    let dir = tempfile::tempdir_in(&cwd).unwrap();
    let root = dir.path().join("watch");
    fs::create_dir(&root).unwrap();
    let engine = Arc::new(StorageEngine::open(dir.path().join("store")).unwrap());
    let relative = root.strip_prefix(&cwd).unwrap();
    let service = WatcherService::new(
        Arc::clone(&engine),
        WatcherConfig::new(relative)
            .with_debounce(Duration::from_millis(50))
            .with_throttle_ms(0),
    );
    service.reconciliation_scan().unwrap();
    let handle = service.start().unwrap();
    wait_until("relative root initial scan", || {
        handle.status().phase == WatcherPhase::Watching
    });
    fs::write(root.join("created.txt"), b"absolute event").unwrap();
    wait_until("absolute event under relative root", || {
        engine.get_versions("created.txt").is_ok()
    });
    let output = dir.path().join("restored");
    engine.get_file("created.txt", &output).unwrap();
    assert_eq!(fs::read(output).unwrap(), b"absolute event");
    handle.stop();
}

#[test]
fn manual_scan_validates_root_and_returns_real_errors() {
    let dir = tempdir().unwrap();
    let engine = Arc::new(StorageEngine::open(dir.path().join("store")).unwrap());
    let source = dir.path().join("not-a-directory");
    fs::write(&source, b"file").unwrap();
    for path in [
        source,
        dir.path().join("absent"),
        dir.path().to_path_buf(),
        engine.root_dir().to_path_buf(),
    ] {
        let service = WatcherService::new(Arc::clone(&engine), WatcherConfig::new(path));
        assert!(service.reconciliation_scan().is_err());
    }
}

#[cfg(windows)]
#[test]
fn startup_locked_file_retries_until_unlocked_without_another_write() {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;

    let dir = tempdir().unwrap();
    let root = dir.path().join("watch");
    fs::create_dir(&root).unwrap();
    let file = root.join("locked.txt");
    fs::write(&file, b"locked at startup").unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(&file)
        .unwrap();
    let engine = Arc::new(StorageEngine::open(dir.path().join("store")).unwrap());
    let service = WatcherService::new(
        Arc::clone(&engine),
        WatcherConfig::new(&root)
            .with_debounce(Duration::from_millis(50))
            .with_throttle_ms(0),
    );
    let handle = service.start().unwrap();
    wait_until("locked startup retry", || {
        let status = handle.status();
        status.phase == WatcherPhase::Degraded
            && status.pending_files > 0
            && status.last_error.is_some()
    });
    assert!(handle.status().last_sync_at.is_none());
    assert!(engine.get_versions("locked.txt").is_err());
    drop(lock);
    wait_until("unlock retry completes", || {
        let status = handle.status();
        engine.get_versions("locked.txt").is_ok()
            && status.phase == WatcherPhase::Watching
            && status.pending_files == 0
            && status.last_error.is_none()
            && status.last_sync_at.is_some()
    });
    assert_eq!(engine.get_versions("locked.txt").unwrap().len(), 1);
    let out = dir.path().join("restored");
    engine.get_file("locked.txt", &out).unwrap();
    assert_eq!(fs::read(out).unwrap(), b"locked at startup");
    handle.stop();
}

#[cfg(windows)]
#[test]
fn manual_scan_reports_repeated_sharing_errors_and_recovers_on_rescan() {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempdir().unwrap();
    let root = dir.path().join("watch");
    fs::create_dir(&root).unwrap();
    let path = root.join("locked.txt");
    fs::write(&path, b"locked").unwrap();
    let engine = Arc::new(StorageEngine::open(dir.path().join("store")).unwrap());
    let service = WatcherService::new(Arc::clone(&engine), WatcherConfig::new(&root));
    let lock = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    for _ in 0..2 {
        let error = service.reconciliation_scan().unwrap_err();
        assert!(WatcherService::is_sharing_violation(&error), "{error}");
    }
    drop(lock);
    service.reconciliation_scan().unwrap();
    assert_eq!(engine.get_versions("locked.txt").unwrap().len(), 1);
}
