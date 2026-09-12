use std::fs;
use std::io::Write;

use oos_lite_core::segment::SegmentStore;
use oos_lite_core::{OosLiteError, StorageEngine};
use tempfile::tempdir;

#[test]
fn recovery_rejects_corruption_without_changing_segment_bytes() {
    for corruption in [
        "middle-payload",
        "tail-payload",
        "middle-header",
        "tail-header",
        "magic",
    ] {
        let dir = tempdir().unwrap();
        let store = SegmentStore::new(dir.path()).unwrap();
        let (first, _) = store.put_chunk(b"first independent content").unwrap();
        let (second, _) = store.put_chunk(b"healthy later content").unwrap();
        store.sync().unwrap();
        let first_location = store.get_location(&first).unwrap();
        let second_location = store.get_location(&second).unwrap();
        drop(store);
        let path = dir.path().join("segment_00000001.seg");
        let mut bytes = fs::read(&path).unwrap();
        let offset = match corruption {
            "middle-payload" => first_location.payload_offset,
            "tail-payload" => second_location.payload_offset,
            "middle-header" => first_location.record_offset + 10,
            "tail-header" => second_location.record_offset + 10,
            "magic" => first_location.record_offset,
            _ => unreachable!(),
        };
        bytes[offset as usize] ^= 0xff;
        fs::write(&path, &bytes).unwrap();
        let error = SegmentStore::new(dir.path())
            .err()
            .expect("corruption must fail closed");
        assert!(
            matches!(error, OosLiteError::CorruptedSegment { .. }),
            "{error}"
        );
        assert!(error.to_string().contains("segment_00000001.seg"));
        assert_eq!(fs::read(&path).unwrap(), bytes, "{corruption}");
    }
}

#[test]
fn recovery_repairs_only_incomplete_active_tail() {
    for tail_kind in [
        "one-byte",
        "two-bytes",
        "three-bytes",
        "v2-header",
        "v3-header",
        "payload",
    ] {
        let dir = tempdir().unwrap();
        let store = SegmentStore::new(dir.path()).unwrap();
        let (first, _) = store.put_chunk(b"committed content").unwrap();
        let (second, _) = store.put_chunk(b"uncommitted content").unwrap();
        store.sync().unwrap();
        let tail = store.get_location(&second).unwrap();
        drop(store);
        let path = dir.path().join("segment_00000001.seg");
        let mut bytes = fs::read(&path).unwrap();
        let keep = match tail_kind {
            "one-byte" => tail.record_offset + 1,
            "two-bytes" => tail.record_offset + 2,
            "three-bytes" => tail.record_offset + 3,
            "v2-header" | "v3-header" => tail.record_offset + 15,
            "payload" => tail.payload_offset + tail.payload_len as u64 - 1,
            _ => unreachable!(),
        };
        bytes.truncate(keep as usize);
        if tail_kind == "v2-header" {
            bytes[tail.record_offset as usize..tail.record_offset as usize + 4]
                .copy_from_slice(b"OOSR");
        }
        fs::write(&path, bytes).unwrap();
        let reopened = SegmentStore::new(dir.path()).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().len(),
            tail.record_offset,
            "{tail_kind}"
        );
        assert_eq!(reopened.get_chunk(&first).unwrap(), b"committed content");
        let (new, _) = reopened.put_chunk(b"after recovery").unwrap();
        reopened.sync().unwrap();
        assert_eq!(reopened.get_chunk(&new).unwrap(), b"after recovery");
    }
}

#[test]
fn recovery_does_not_repair_a_sealed_segment() {
    for corruption in ["fragment", "crc", "payload"] {
        let dir = tempdir().unwrap();
        let store = SegmentStore::with_max_segment_size(dir.path(), 180).unwrap();
        let first: Vec<_> = (0..64).map(|n| (n * 37) as u8).collect();
        let second: Vec<_> = (0..64).map(|n| (n * 53 + 11) as u8).collect();
        let (first, _) = store.put_chunk(&first).unwrap();
        let (second, _) = store.put_chunk(&second).unwrap();
        store.sync().unwrap();
        let first = store.get_location(&first).unwrap();
        let second = store.get_location(&second).unwrap();
        assert_ne!(first.segment_id, second.segment_id);
        drop(store);
        let path = dir
            .path()
            .join(format!("segment_{:08}.seg", first.segment_id));
        let mut bytes = fs::read(&path).unwrap();
        match corruption {
            "fragment" => bytes.extend_from_slice(b"OS"),
            "crc" => bytes[first.payload_offset as usize] ^= 0xff,
            "payload" => {
                bytes.pop();
            }
            _ => unreachable!(),
        }
        fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            SegmentStore::new(dir.path()),
            Err(OosLiteError::CorruptedSegment { .. })
        ));
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn invalid_tail_magic_is_not_treated_as_a_partial_write() {
    let dir = tempdir().unwrap();
    let store = SegmentStore::new(dir.path()).unwrap();
    store.put_chunk(b"committed").unwrap();
    store.sync().unwrap();
    drop(store);
    let path = dir.path().join("segment_00000001.seg");
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"BAD!")
        .unwrap();
    let bytes = fs::read(&path).unwrap();
    assert!(SegmentStore::new(dir.path()).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn unbound_versions_survive_gc_and_restart_with_or_without_snapshot() {
    for snapshot in [false, true] {
        let dir = tempdir().unwrap();
        let source = dir.path().join("source.txt");
        let root = dir.path().join("store");
        let engine = StorageEngine::open(&root).unwrap();
        fs::write(&source, b"version one").unwrap();
        let object_id = engine.put_file_named("doc.txt", &source).unwrap().object_id;
        fs::write(&source, b"version two").unwrap();
        engine.put_file_named("doc.txt", &source).unwrap();
        if snapshot {
            engine.create_snapshot("backup").unwrap();
        }
        engine.unbind_file("doc.txt").unwrap();
        engine.gc().unwrap();
        drop(engine);
        let engine = StorageEngine::open(&root).unwrap();
        assert!(engine.list_files().unwrap().is_empty());
        let target = object_id.to_string();
        for (version, expected) in [(1, b"version one"), (2, b"version two")] {
            let output = dir.path().join(format!("restore-{version}"));
            engine
                .get_file_version(&target, Some(version), &output)
                .unwrap();
            assert_eq!(fs::read(output).unwrap(), expected);
        }
        assert!(engine.fsck().unwrap().is_healthy);
        engine.metadata_store().delete_object(&object_id).unwrap();
        engine.metadata_store().flush().unwrap();
        if snapshot {
            engine.delete_snapshot("backup").unwrap();
        }
        assert!(engine.gc().unwrap().chunks_reclaimed > 0);
    }
}

#[test]
fn gc_aborts_before_sweep_when_a_root_manifest_is_missing() {
    for snapshot_only in [false, true] {
        let dir = tempdir().unwrap();
        let engine = StorageEngine::open(dir.path().join("store")).unwrap();
        let source = dir.path().join("source");
        fs::write(&source, b"live").unwrap();
        let summary = engine.put_file_named("live.txt", &source).unwrap();
        if snapshot_only {
            engine.create_snapshot("backup").unwrap();
            engine.delete_file("live.txt").unwrap();
        } else {
            engine.unbind_file("live.txt").unwrap();
        }
        fs::write(&source, b"unreferenced").unwrap();
        let orphan = engine.put_file_named("orphan.txt", &source).unwrap();
        engine.delete_file("orphan.txt").unwrap();
        engine
            .metadata_store()
            .delete_manifest(&summary.manifest_id)
            .unwrap();
        let before = engine.segment_store().chunk_count();
        assert!(engine.gc().is_err());
        assert_eq!(engine.segment_store().chunk_count(), before);
        assert!(engine
            .metadata_store()
            .get_manifest(&orphan.manifest_id)
            .unwrap()
            .is_some());
        let report = engine.fsck().unwrap();
        assert!(!report.is_healthy);
        if !snapshot_only {
            assert_eq!(report.objects_checked, 1);
        }
    }
}
