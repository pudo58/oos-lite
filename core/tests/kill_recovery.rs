use std::env;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;
use tempfile::tempdir;

use oos_lite_core::chunk::ChunkId;
use oos_lite_core::segment::SegmentStore;

#[test]
fn test_real_subprocess_kill_and_recovery() {
    // If running as the spawned child worker:
    if let Ok(dir) = env::var("OOS_KILL_WORKER_DIR") {
        let store = SegmentStore::new(&dir).expect("child store init failed");

        // Step 1: Write 5 confirmed chunks and fsync
        for i in 0..5 {
            let data = format!("CRITICAL_FSYNCED_CHUNK_NUMBER_{}", i).into_bytes();
            let (id, _) = store.put_chunk(&data).expect("child put failed");
            // Print chunk id to stdout so parent knows it is fsynced
            println!("COMMITTED:{}", id);
        }
        store.sync().expect("child sync failed");
        println!("SYNC_DONE");
        std::io::stdout().flush().unwrap();

        // Step 2: Now continuously write garbage or partial writes in a loop
        // until parent brutally kills this process
        let mut counter = 100;
        loop {
            let data = format!("UNCOMMITTED_VOLATILE_DATA_{}", counter).into_bytes();
            let _ = store.put_chunk(&data);
            counter += 1;
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    // Otherwise, we are the parent test runner:
    let temp = tempdir().expect("tempdir failed");
    let temp_dir_str = temp.path().to_string_lossy().to_string();

    let exe = env::current_exe().expect("failed to get current test binary path");
    let mut child = Command::new(&exe)
        .arg("test_real_subprocess_kill_and_recovery")
        .arg("--exact")
        .arg("--nocapture")
        .env("OOS_KILL_WORKER_DIR", &temp_dir_str)
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn child process");

    let stdout = child.stdout.take().expect("failed to capture child stdout");
    let reader = BufReader::new(stdout);

    let mut committed_ids = Vec::new();
    let mut sync_confirmed = false;

    for line in reader.lines() {
        if let Ok(line) = line {
            if let Some(id_str) = line.strip_prefix("COMMITTED:") {
                let id: ChunkId = id_str.trim().parse().expect("parse chunk id failed");
                committed_ids.push(id);
            } else if line.contains("SYNC_DONE") {
                sync_confirmed = true;
                break;
            }
        }
    }

    assert!(sync_confirmed, "Child failed to confirm sync");
    assert_eq!(committed_ids.len(), 5, "Expected 5 committed chunks");

    // Brutally kill the child process (simulating kill -9 / SIGKILL / TerminateProcess)
    child.kill().expect("Failed to kill child process");
    let _ = child.wait();

    // Now restart the store from the exact same directory
    let store = SegmentStore::new(temp.path()).expect("Store restart after kill failed");

    // Verify all 5 fsynced chunks are intact and recoverable
    for (i, id) in committed_ids.iter().enumerate() {
        assert!(store.has_chunk(id), "Fsynced chunk {} was lost after crash!", id);
        let data = store.get_chunk(id).expect("Failed to read fsynced chunk after recovery");
        let expected = format!("CRITICAL_FSYNCED_CHUNK_NUMBER_{}", i).into_bytes();
        assert_eq!(data, expected, "Data corrupted for chunk {}", id);
    }

    // Verify store can continue writing without errors after crash recovery
    let resume_data = b"Clean chunk written after crash recovery";
    let (new_id, is_new) = store.put_chunk(resume_data).expect("put after crash failed");
    assert!(is_new);
    assert_eq!(store.get_chunk(&new_id).unwrap(), resume_data);
}

// =========================================================================
// Milestone 5: Full Write Path Crash Consistency & WAL Tests
// =========================================================================

use std::fs;
use oos_lite_core::wal::{Wal, WalPutPayload};
use oos_lite_core::{Manifest, ObjectId, StorageEngine};

/// Helper: run subprocess to simulate abrupt crash (kill -9) at specified crash point
fn run_subprocess_put(store_path: &str, file_path: &str, name: &str, crash_at: Option<&str>) -> bool {
    let current_exe = std::env::current_exe().expect("Failed to get current exe");
    let mut cmd = Command::new(current_exe);
    cmd.arg("--exact")
        .arg("test_milestone5_subprocess_worker")
        .arg("--nocapture")
        .env("OOS_M5_WORKER", "1")
        .env("OOS_STORE_PATH", store_path)
        .env("OOS_INPUT_FILE", file_path)
        .env("OOS_LOGICAL_NAME", name);

    if let Some(cp) = crash_at {
        cmd.env("OOS_CRASH_AT", cp);
    }

    let status = cmd.status().expect("Failed to run worker subprocess");
    status.success()
}

#[test]
fn test_milestone5_subprocess_worker() {
    if std::env::var("OOS_M5_WORKER").is_err() {
        return; // Only run when explicitly invoked as worker
    }

    let store_path = std::env::var("OOS_STORE_PATH").unwrap();
    let input_file = std::env::var("OOS_INPUT_FILE").unwrap();
    let logical_name = std::env::var("OOS_LOGICAL_NAME").unwrap();

    let engine = StorageEngine::open(&store_path).expect("Failed to open StorageEngine in worker");
    let _ = engine.put_file_named(&logical_name, &input_file);
}

#[test]
fn test_milestone5_a_normal_shutdown() {
    let dir = tempdir().unwrap();
    let store_path = dir.path().join("store");
    let test_file = dir.path().join("sample.txt");
    let content = b"Milestone 5 Normal Shutdown: data must persist cleanly across restarts.";
    fs::write(&test_file, content).unwrap();

    // 1. First run: Put file and cleanly drop engine
    {
        let engine = StorageEngine::open(&store_path).unwrap();
        let summary = engine.put_file_named("sample.txt", &test_file).unwrap();
        assert_eq!(summary.version, 1);
        assert_eq!(summary.total_bytes, content.len() as u64);
    }

    // 2. Restart engine: verify file is read back byte-for-byte
    {
        let engine = StorageEngine::open(&store_path).unwrap();
        let out_file = dir.path().join("restored.txt");
        let restored_bytes = engine.get_file("sample.txt", &out_file).unwrap();
        assert_eq!(restored_bytes, content.len() as u64);

        let read_back = fs::read(&out_file).unwrap();
        assert_eq!(read_back, content);
    }
}

#[test]
fn test_milestone5_b_kill_at_3_different_points_in_write_path() {
    let dir = tempdir().unwrap();
    let store_path = dir.path().join("crash_store");
    let store_str = store_path.to_str().unwrap();

    // -------------------------------------------------------------
    // Point 1: Kill immediately after WAL fsync (before chunk write)
    // -------------------------------------------------------------
    let f1 = dir.path().join("point1.txt");
    let content1 = b"Crash Point 1 Data: Aborted after WAL fsync, before chunk write to segment.";
    fs::write(&f1, content1).unwrap();
    let f1_str = f1.to_str().unwrap();

    // Subprocess should crash / abort
    let success = run_subprocess_put(store_str, f1_str, "file1.txt", Some("after_wal_fsync"));
    assert!(!success, "Subprocess should have aborted at after_wal_fsync");

    // Reopening engine should replay WAL and recover the write completely
    {
        let engine = StorageEngine::open(&store_path).expect("Engine should recover from WAL");
        let out1 = dir.path().join("out1.txt");
        let len = engine.get_file("file1.txt", &out1).expect("file1.txt must be recovered");
        assert_eq!(len, content1.len() as u64);
        assert_eq!(fs::read(&out1).unwrap(), content1);
    }

    // -------------------------------------------------------------
    // Point 2: Kill after chunk write (before metadata update)
    // -------------------------------------------------------------
    let f2 = dir.path().join("point2.txt");
    let content2 = b"Crash Point 2 Data: Aborted after chunk write, before sled metadata update.";
    fs::write(&f2, content2).unwrap();
    let f2_str = f2.to_str().unwrap();

    let success = run_subprocess_put(store_str, f2_str, "file2.txt", Some("after_chunk_write"));
    assert!(!success, "Subprocess should have aborted at after_chunk_write");

    // Reopening engine should recover file2.txt from WAL
    {
        let engine = StorageEngine::open(&store_path).expect("Engine should recover from WAL");
        let out2 = dir.path().join("out2.txt");
        let len = engine.get_file("file2.txt", &out2).expect("file2.txt must be recovered");
        assert_eq!(len, content2.len() as u64);
        assert_eq!(fs::read(&out2).unwrap(), content2);
    }

    // -------------------------------------------------------------
    // Point 3: Kill after metadata update (before WAL checkpoint)
    // -------------------------------------------------------------
    let f3 = dir.path().join("point3.txt");
    let content3 = b"Crash Point 3 Data: Aborted after metadata update, before WAL checkpoint.";
    fs::write(&f3, content3).unwrap();
    let f3_str = f3.to_str().unwrap();

    let success = run_subprocess_put(store_str, f3_str, "file3.txt", Some("after_metadata_update"));
    assert!(!success, "Subprocess should have aborted at after_metadata_update");

    // Reopening engine should replay idempotently and checkpoint
    {
        let engine = StorageEngine::open(&store_path).expect("Engine should recover cleanly");
        let out3 = dir.path().join("out3.txt");
        let len = engine.get_file("file3.txt", &out3).expect("file3.txt must be readable");
        assert_eq!(len, content3.len() as u64);
        assert_eq!(fs::read(&out3).unwrap(), content3);

        // Verify version history is not duplicated
        let versions = engine.get_versions("file3.txt").unwrap();
        assert_eq!(versions.len(), 1, "Should not duplicate version after idempotent replay");
    }

    // Verify criterion: 0 writes that fsynced WAL were lost
    {
        let engine = StorageEngine::open(&store_path).unwrap();
        let list = engine.list_files().unwrap();
        assert_eq!(list.len(), 3, "All 3 files must be present and accounted for");
    }
}

#[test]
fn test_milestone5_c_corrupted_wal_record_detected() {
    let dir = tempdir().unwrap();
    let wal_dir = dir.path().join("wal");

    let mut wal = Wal::open(&wal_dir).unwrap();
    let chunk_data = b"Corrupted record test chunk".to_vec();
    let cid = ChunkId::from_data(&chunk_data);
    let manifest = Manifest::new(vec![cid], chunk_data.len() as u64, [0u8; 32]);
    let payload = WalPutPayload {
        name: "corrupted.txt".into(),
        object_id: ObjectId::generate(),
        version: 1,
        manifest,
        chunks: vec![(cid, chunk_data)],
    };

    wal.append_put_and_sync(payload).unwrap();
    drop(wal);

    // Corrupt 1 byte in the WAL payload on disk
    let log_file = wal_dir.join("wal.log");
    let mut bytes = fs::read(&log_file).unwrap();
    let last = bytes.len() - 2;
    bytes[last] ^= 0xAA; // Flip bits
    fs::write(&log_file, &bytes).unwrap();

    // Reopen WAL: must detect corruption via CRC32C and safely isolate corrupted record
    let wal = Wal::open(&wal_dir).unwrap();
    let uncheckpointed = wal.read_uncheckpointed_records().unwrap();
    assert_eq!(
        uncheckpointed.len(),
        0,
        "Corrupted WAL record must be rejected and not replayed"
    );
}

#[test]
fn test_milestone5_d_idempotent_recovery_repeated_twice() {
    let dir = tempdir().unwrap();
    let store_path = dir.path().join("idempotent_store");
    let store_str = store_path.to_str().unwrap();

    let f = dir.path().join("idem.txt");
    let content = b"Idempotent Recovery Test Data: repeating recovery must yield identical state.";
    fs::write(&f, content).unwrap();

    // Crash after WAL fsync so that WAL replay is required
    let success = run_subprocess_put(store_str, f.to_str().unwrap(), "idem.txt", Some("after_wal_fsync"));
    assert!(!success);

    // Recovery Run 1
    {
        let engine1 = StorageEngine::open(&store_path).unwrap();
        let out1 = dir.path().join("run1.txt");
        engine1.get_file("idem.txt", &out1).unwrap();
        assert_eq!(fs::read(&out1).unwrap(), content);
        let versions1 = engine1.get_versions("idem.txt").unwrap();
        assert_eq!(versions1.len(), 1);
    }

    // Recovery Run 2 (repeated on same store)
    {
        let engine2 = StorageEngine::open(&store_path).unwrap();
        let out2 = dir.path().join("run2.txt");
        engine2.get_file("idem.txt", &out2).unwrap();
        assert_eq!(fs::read(&out2).unwrap(), content);
        let versions2 = engine2.get_versions("idem.txt").unwrap();
        assert_eq!(versions2.len(), 1);
    }
}
