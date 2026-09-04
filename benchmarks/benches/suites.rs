use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use tempfile::tempdir;

use oos_lite_core::chunk::Chunker;
use oos_lite_core::StorageEngine;

fn bench_chunker_and_crypto(c: &mut Criterion) {
    let size = 4 * 1024 * 1024; // 4 MiB
    let mut data = vec![0u8; size];
    for (i, b) in data.iter_mut().enumerate() {
        *b = (i % 256) as u8;
    }

    let mut group = c.benchmark_group("micro_primitives");
    group.throughput(Throughput::Bytes(size as u64));

    group.bench_function("fastcdc_chunking_4mb", |b| {
        b.iter(|| {
            let chunker = Chunker::new(black_box(&data));
            let chunks = chunker.chunks();
            black_box(chunks.len());
        });
    });

    group.bench_function("blake3_4mb", |b| {
        b.iter(|| {
            let hash = blake3::hash(black_box(&data));
            black_box(hash);
        });
    });

    group.bench_function("crc32c_4mb", |b| {
        b.iter(|| {
            let crc = crc32fast::hash(black_box(&data));
            black_box(crc);
        });
    });

    group.finish();
}

fn bench_snapshot_latency(c: &mut Criterion) {
    let dir = tempdir().expect("tempdir failed");
    let store_dir = dir.path().join("bench_store");
    let engine = StorageEngine::open(&store_dir).expect("engine open failed");

    // Populate with 50 files
    for i in 1..=50 {
        let f_path = dir.path().join(format!("file_{}.txt", i));
        std::fs::write(&f_path, format!("File number {} test content", i)).unwrap();
        engine.put_file_named(&format!("file_{}.txt", i), &f_path).unwrap();
    }

    let mut count = 0u64;
    c.bench_function("engine_snapshot_o1_50_files", |b| {
        b.iter(|| {
            count += 1;
            let snap = engine.create_snapshot(&format!("snap_{}", count)).unwrap();
            black_box(snap);
        });
    });
}

criterion_group!(benches, bench_chunker_and_crypto, bench_snapshot_latency);
criterion_main!(benches);
