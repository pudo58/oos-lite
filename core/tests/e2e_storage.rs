use std::fs::File;
use std::io::{Read, Write};
use tempfile::tempdir;

use oos_lite_core::chunk::Chunker;
use oos_lite_core::StorageEngine;

fn generate_pseudo_random_data(size: usize, seed: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);
    let mut state = seed;
    for _ in 0..size {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        data.push((state & 0xFF) as u8);
    }
    data
}

#[test]
fn test_same_input_same_chunks() {
    let data = generate_pseudo_random_data(1024 * 1024, 0x123456789abcdef0);

    let chunks1 = Chunker::new(&data).chunks();
    let chunks2 = Chunker::new(&data).chunks();

    assert_eq!(chunks1.len(), chunks2.len());
    for (c1, c2) in chunks1.iter().zip(chunks2.iter()) {
        assert_eq!(c1, c2);
    }
}

#[test]
fn test_modified_middle_section_deduplication() {
    let dir = tempdir().expect("tempdir failed");
    let engine = StorageEngine::open(dir.path()).expect("engine open failed");

    // Create Base 1 MiB File using non-repeating pseudo random data
    let base_data = generate_pseudo_random_data(1024 * 1024, 0xcafe_babe_dead_beef);
    let base_file_path = dir.path().join("base.bin");
    {
        let mut f = File::create(&base_file_path).unwrap();
        f.write_all(&base_data).unwrap();
    }

    // First put: all chunks in this random file must be unique and newly created
    let summary1 = engine.put_file(&base_file_path).expect("put base failed");
    assert_eq!(summary1.new_chunks, summary1.chunk_count);
    assert_eq!(summary1.dedup_chunks, 0);

    // Create Modified 1 MiB File (only a small middle portion is changed)
    let mut mod_data = base_data.clone();
    let mid = mod_data.len() / 2;
    for i in 0..64 {
        mod_data[mid + i] ^= 0xFF;
    }
    let mod_file_path = dir.path().join("mod.bin");
    {
        let mut f = File::create(&mod_file_path).unwrap();
        f.write_all(&mod_data).unwrap();
    }

    // Second put: unmodified chunks MUST be deduplicated!
    let summary2 = engine.put_file(&mod_file_path).expect("put mod failed");
    assert!(
        summary2.dedup_chunks > 0,
        "Expected deduplication of unmodified chunks, but got 0 dedup chunks"
    );
    assert!(
        summary2.new_chunks < summary2.chunk_count,
        "New chunks {} must be strictly less than total chunks {}",
        summary2.new_chunks,
        summary2.chunk_count
    );
}

#[test]
fn test_e2e_restart_and_byte_for_byte_5_consecutive_runs() {
    for run in 1..=5 {
        let dir = tempdir().expect("tempdir failed");
        let store_dir = dir.path().join("store");
        let input_file = dir.path().join("original.dat");
        let output_file = dir.path().join("restored.dat");

        // 1. Create a 1.25 MiB random binary file
        let original_bytes = generate_pseudo_random_data(1280 * 1024, 0xfeed_beef_1234_5678 + run);
        {
            let mut f = File::create(&input_file).unwrap();
            f.write_all(&original_bytes).unwrap();
        }

        let original_hash = blake3::hash(&original_bytes);

        // 2. Put file into StorageEngine instance 1
        let manifest_id = {
            let engine = StorageEngine::open(&store_dir).expect("engine open 1 failed");
            let summary = engine.put_file(&input_file).expect("put_file failed");
            assert_eq!(summary.total_bytes, original_bytes.len() as u64);
            summary.manifest_id
        };

        // 3. Simulate process exit / cold restart: open new StorageEngine instance 2
        {
            let engine_reloaded = StorageEngine::open(&store_dir).expect("engine open 2 failed");
            let written_bytes = engine_reloaded
                .get_file(&manifest_id, &output_file)
                .expect("get_file failed");
            assert_eq!(written_bytes, original_bytes.len() as u64);
        }

        // 4. Verify byte-for-byte exact match and hash match
        let mut restored_bytes = Vec::new();
        {
            let mut f = File::open(&output_file).unwrap();
            f.read_to_end(&mut restored_bytes).unwrap();
        }

        assert_eq!(
            restored_bytes.len(),
            original_bytes.len(),
            "Run {}: File length mismatch",
            run
        );
        assert_eq!(
            restored_bytes,
            original_bytes,
            "Run {}: Byte-for-byte content mismatch",
            run
        );
        let restored_hash = blake3::hash(&restored_bytes);
        assert_eq!(
            restored_hash,
            original_hash,
            "Run {}: BLAKE3 hash mismatch",
            run
        );
    }
}
