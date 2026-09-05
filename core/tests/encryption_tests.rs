use std::fs;
use std::path::Path;
use oos_lite_core::error::OosLiteError;
use oos_lite_core::StorageEngine;
use tempfile::tempdir;

#[test]
fn test_encrypted_store_put_get_roundtrip() {
    let dir = tempdir().unwrap();
    let store_path = dir.path().join("store");
    let password = "SuperSecretMasterKey!2026";

    // Initialize and store data
    {
        let engine = StorageEngine::open_with_password(&store_path, password).unwrap();
        assert!(engine.is_encrypted());

        let test_file = dir.path().join("sample.txt");
        let sample_data = b"Hello, encrypted OOS-Lite world! Testing XChaCha20-Poly1305.".repeat(50);
        fs::write(&test_file, &sample_data).unwrap();

        let summary = engine.put_file_named("sample.txt", &test_file).unwrap();
        assert_eq!(summary.total_bytes, sample_data.len() as u64);
        assert!(summary.new_chunks > 0);

        // Immediate extraction check
        let out_file = dir.path().join("out.txt");
        let bytes = engine.get_file("sample.txt", &out_file).unwrap();
        assert_eq!(bytes, sample_data.len() as u64);
        let extracted = fs::read(&out_file).unwrap();
        assert_eq!(extracted, sample_data);
    }

    // Reopen with valid password and check again
    {
        let engine = StorageEngine::open_with_password(&store_path, password).unwrap();
        assert!(engine.is_encrypted());

        let out_file = dir.path().join("out2.txt");
        let bytes = engine.get_file("sample.txt", &out_file).unwrap();
        assert_eq!(fs::read(&out_file).unwrap().len() as u64, bytes);
    }
}

#[test]
fn test_encrypted_store_wrong_password_fails() {
    let dir = tempdir().unwrap();
    let store_path = dir.path().join("store");
    let password = "CorrectPassword123";

    {
        let engine = StorageEngine::open_with_password(&store_path, password).unwrap();
        let test_file = dir.path().join("data.bin");
        fs::write(&test_file, b"some confidential secret payload").unwrap();
        engine.put_file_named("secret.txt", &test_file).unwrap();
    }

    // Try to open with wrong password
    let result = StorageEngine::open_with_password(&store_path, "WrongPassword456");
    assert!(result.is_err());
    match result.err().unwrap() {
        OosLiteError::AuthenticationFailed(msg) => {
            assert!(msg.contains("Incorrect passphrase"));
        }
        other => panic!("Expected AuthenticationFailed error, got: {:?}", other),
    }

    // Try to open without password
    let no_pass_result = StorageEngine::open(&store_path);
    assert!(no_pass_result.is_err());
    match no_pass_result.err().unwrap() {
        OosLiteError::PasswordRequired => {}
        other => panic!("Expected PasswordRequired error, got: {:?}", other),
    }
}

#[test]
fn test_deduplication_preserved_under_encryption() {
    let dir = tempdir().unwrap();
    let store_path = dir.path().join("store");
    let password = "DedupEncryptionTestPassword#1";

    let engine = StorageEngine::open_with_password(&store_path, password).unwrap();

    let content = b"Identical repeated chunk content that must be deduplicated across encrypted objects.".repeat(200);
    let file1 = dir.path().join("file1.bin");
    let file2 = dir.path().join("file2.bin");
    fs::write(&file1, &content).unwrap();
    fs::write(&file2, &content).unwrap();

    let sum1 = engine.put_file_named("copy1.bin", &file1).unwrap();
    assert!(sum1.new_chunks > 0);
    assert_eq!(sum1.dedup_chunks, 0);

    let sum2 = engine.put_file_named("copy2.bin", &file2).unwrap();
    // Identical content must be 100% deduplicated!
    assert_eq!(sum2.new_chunks, 0);
    assert_eq!(sum2.dedup_chunks, sum1.chunk_count);

    let stats = engine.stats();
    assert!(stats.dedup_ratio >= 1.9);
}

#[test]
fn test_ciphertext_on_disk_is_never_plaintext() {
    let dir = tempdir().unwrap();
    let store_path = dir.path().join("store");
    let password = "ZeroPlaintextOnDiskTestPassword!";

    let engine = StorageEngine::open_with_password(&store_path, password).unwrap();

    let secret_phrase = "THIS_IS_A_HIGHLY_SENSITIVE_SECRET_PHRASE_THAT_MUST_NEVER_APPEAR_ON_DISK";
    let test_file = dir.path().join("confidential.txt");
    fs::write(&test_file, secret_phrase.as_bytes()).unwrap();

    engine.put_file_named("confidential.txt", &test_file).unwrap();
    drop(engine);

    // Search every file in the store directory recursively
    fn search_dir(dir: &Path, target: &[u8]) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let p = entry.path();
            if p.is_dir() {
                search_dir(&p, target);
            } else if p.is_file() {
                let bytes = fs::read(&p).unwrap();
                assert!(
                    !bytes.windows(target.len()).any(|w| w == target),
                    "Found plaintext secret in file: {}",
                    p.display()
                );
            }
        }
    }

    search_dir(&store_path, secret_phrase.as_bytes());
}

#[test]
fn test_encrypted_tampered_segment_detected() {
    let dir = tempdir().unwrap();
    let store_path = dir.path().join("store");
    let password = "TamperDetectionPassword!";

    {
        let engine = StorageEngine::open_with_password(&store_path, password).unwrap();
        let test_file = dir.path().join("important.txt");
        fs::write(&test_file, b"Integrity check payload for Poly1305 MAC tampering test.").unwrap();
        engine.put_file_named("important.txt", &test_file).unwrap();
    }

    // Tamper with segment file bytes
    let seg_file = store_path.join("segments").join("segment_00000001.seg");
    let mut bytes = fs::read(&seg_file).unwrap();
    // Tamper with the last byte
    let last = bytes.len() - 1;
    bytes[last] ^= 0xAA;
    fs::write(&seg_file, &bytes).unwrap();

    // Reopen engine
    let engine = StorageEngine::open_with_password(&store_path, password).unwrap();

    // Extracting tampered file must fail (ChecksumMismatch or DecryptionFailed)
    let out = dir.path().join("tampered_out.txt");
    let get_res = engine.get_file("important.txt", &out);
    assert!(get_res.is_err());

    // FSCK must flag corruption
    let report = engine.fsck().unwrap();
    assert!(!report.is_healthy);
    assert!(report.corrupted_chunks > 0 || !report.errors.is_empty());
}

#[test]
fn test_encrypted_gc_and_fsck() {
    let dir = tempdir().unwrap();
    let store_path = dir.path().join("store");
    let password = "GcAndFsckEncryptedPassword!";

    let engine = StorageEngine::open_with_password(&store_path, password).unwrap();

    let f1 = dir.path().join("f1.txt");
    fs::write(&f1, b"Temporary chunk data that will be unreferenced after deletion.").unwrap();
    engine.put_file_named("f1.txt", &f1).unwrap();

    let f2 = dir.path().join("f2.txt");
    fs::write(&f2, b"Persistent chunk data that stays alive across GC.").unwrap();
    engine.put_file_named("f2.txt", &f2).unwrap();

    // Delete f1
    let deleted = engine.delete_file("f1.txt").unwrap();
    assert!(deleted);

    // Run GC
    let gc_stats = engine.gc().unwrap();
    assert!(gc_stats.chunks_reclaimed > 0);

    // FSCK must report 100% clean and healthy
    let report = engine.fsck().unwrap();
    assert!(report.is_healthy);
    assert_eq!(report.corrupted_chunks, 0);
    assert_eq!(report.missing_chunks, 0);

    // Read f2 to verify it is intact
    let out2 = dir.path().join("out2.txt");
    engine.get_file("f2.txt", &out2).unwrap();
    assert_eq!(fs::read(&out2).unwrap(), b"Persistent chunk data that stays alive across GC.");
}

#[test]
fn test_encrypted_wal_crash_recovery() {
    let dir = tempdir().unwrap();
    let store_path = dir.path().join("store");
    let password = "WalRecoveryEncryptedPassword!";

    // Create file with WAL recovery check
    {
        let engine = StorageEngine::open_with_password(&store_path, password).unwrap();
        let f = dir.path().join("crash_test.txt");
        let content = b"Content to be safely replayed through encrypted WAL after simulated crash.".to_vec();
        fs::write(&f, &content).unwrap();

        // Write directly to WAL without putting into segments to simulate crash before segment write
        let wal_dir = store_path.join("wal");
        let mut wal = oos_lite_core::wal::Wal::open_with_vault(
            &wal_dir,
            engine.vault_key().cloned(),
        ).unwrap();

        let cid = oos_lite_core::chunk::ChunkId::from_data(&content);
        let content_hash = *blake3::hash(&content).as_bytes();
        let manifest = oos_lite_core::manifest::Manifest::new(vec![cid], content.len() as u64, content_hash);
        let wal_payload = oos_lite_core::wal::WalPutPayload {
            name: "uncheckpointed.txt".to_string(),
            object_id: oos_lite_core::object::ObjectId::generate(),
            version: 1,
            manifest,
            chunks: vec![(cid, content.clone())],
        };

        wal.append_put_and_sync(wal_payload).unwrap();
        // Drop without checkpointing
    }

    // Reopen store: WAL replay should recover uncheckpointed.txt
    {
        let engine = StorageEngine::open_with_password(&store_path, password).unwrap();
        let out = dir.path().join("recovered.txt");
        let bytes = engine.get_file("uncheckpointed.txt", &out).unwrap();
        assert!(bytes > 0);
        assert_eq!(
            fs::read(&out).unwrap(),
            b"Content to be safely replayed through encrypted WAL after simulated crash."
        );
    }
}

#[test]
fn test_existing_unencrypted_store_rejects_password() {
    let dir = tempdir().unwrap();
    let store_path = dir.path().join("store");

    // Create an unencrypted store and write data
    {
        let engine = StorageEngine::open(&store_path).unwrap();
        let f = dir.path().join("plain.txt");
        fs::write(&f, b"plaintext store contents").unwrap();
        engine.put_file_named("plain.txt", &f).unwrap();
    }

    // Attempting to open with password on existing plaintext store must fail (no hybrid store!)
    let result = StorageEngine::open_with_password(&store_path, "SecretPassword123!");
    assert!(result.is_err());
    let err_msg = format!("{:?}", result.err().unwrap());
    assert!(err_msg.contains("store already contains unencrypted data"));

    // init_encrypted must also fail on existing plaintext store
    let init_result = StorageEngine::init_encrypted(&store_path, "SecretPassword123!");
    assert!(init_result.is_err());

    // Original unencrypted store must still be readable
    let plain_engine = StorageEngine::open(&store_path).unwrap();
    let out = dir.path().join("plain_out.txt");
    plain_engine.get_file("plain.txt", &out).unwrap();
    assert_eq!(fs::read(&out).unwrap(), b"plaintext store contents");
}

#[test]
fn test_short_and_empty_passwords_rejected() {
    let dir = tempdir().unwrap();
    let store_path = dir.path().join("store");

    // Empty password rejected
    let empty_res = StorageEngine::open_with_password(&store_path, "");
    assert!(empty_res.is_err());

    let spaces_res = StorageEngine::open_with_password(&store_path, "   ");
    assert!(spaces_res.is_err());

    // Short password (< 8 chars) rejected
    let short_res = StorageEngine::open_with_password(&store_path, "1234567");
    assert!(short_res.is_err());

    // Exactly 8 chars accepted
    let valid_res = StorageEngine::open_with_password(&store_path, "12345678");
    assert!(valid_res.is_ok());
}

#[test]
fn test_atomic_vault_key_creation() {
    let dir = tempdir().unwrap();
    let store_path = dir.path().join("store");

    let engine = StorageEngine::open_with_password(&store_path, "AtomicVaultPassword1!").unwrap();
    assert!(engine.is_encrypted());

    let vault_file = store_path.join("vault.key");
    assert!(vault_file.exists());
    let vault_meta = fs::metadata(&vault_file).unwrap();
    assert_eq!(vault_meta.len(), 100);

    // Verify no temporary files left in root directory
    for entry in fs::read_dir(&store_path).unwrap().flatten() {
        let name = entry.file_name();
        let s = name.to_string_lossy();
        assert!(!s.starts_with(".vault.key.tmp"), "Temporary vault file remained: {}", s);
    }
}

