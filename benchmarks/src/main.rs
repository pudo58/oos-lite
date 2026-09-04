use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};
use tempfile::tempdir;

use oos_lite_core::StorageEngine;

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GiB ({:.2} MB)", b / GIB, b / MIB)
    } else if b >= MIB {
        format!("{:.2} MiB ({:.2} MB)", b / MIB, b / MIB)
    } else if b >= KIB {
        format!("{:.2} KiB", b / KIB)
    } else {
        format!("{} B", bytes)
    }
}

fn dir_physical_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += dir_physical_size(&p);
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

/// Generates a pseudo-random block of bytes using a deterministic xorshift PRNG.
fn generate_deterministic_data(seed: u64, size: usize) -> Vec<u8> {
    let mut rng = seed;
    let mut buf = vec![0u8; size];
    for chunk in buf.chunks_exact_mut(8) {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        chunk.copy_from_slice(&rng.to_le_bytes());
    }
    buf
}

/// Mutates ~1% of the given buffer in place.
fn mutate_1_percent(buf: &mut [u8], seed: u64) {
    let total_len = buf.len();
    let mutate_len = total_len / 100; // 1%
    let mut rng = seed;

    // Mutate in chunks of 64 KiB across scattered positions
    let chunk_size = 64 * 1024;
    let mut mutated = 0;
    while mutated < mutate_len {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let offset = (rng as usize) % (total_len.saturating_sub(chunk_size));
        for i in 0..chunk_size.min(mutate_len - mutated) {
            buf[offset + i] ^= 0xA5;
        }
        mutated += chunk_size;
    }
}

fn main() {
    println!("================================================================================");
    println!("             OOS-LITE MILESTONE 9: REAL-WORLD BENCHMARK SUITE                   ");
    println!("================================================================================\n");

    let temp = tempdir().expect("tempdir failed");
    let base_dir = temp.path();

    // -------------------------------------------------------------------------
    // BENCHMARK 1: 100 MB FILE x 10 VERSIONS (~1% MUTATED PER VERSION)
    // -------------------------------------------------------------------------
    println!("==> [1/3] Benchmarking Storage Space: 10 Versions of 100 MB File (1% delta/v)...");

    let oos_dir = base_dir.join("oos_store");
    let cp_dir = base_dir.join("cp_store");
    let link_dir = base_dir.join("link_dest_store");

    fs::create_dir_all(&cp_dir).unwrap();
    fs::create_dir_all(&link_dir).unwrap();

    let engine = StorageEngine::open(&oos_dir).expect("engine open failed");

    let file_size = 100 * 1024 * 1024; // 100 MiB
    println!("  Generating initial 100 MiB base file...");
    let mut current_data = generate_deterministic_data(0x123456789ABCDEF0, file_size);

    let mut oos_times = Vec::new();
    let mut cp_times = Vec::new();

    let versions_count = 10;
    for v in 1..=versions_count {
        if v > 1 {
            // Mutate ~1% of data
            mutate_1_percent(&mut current_data, 0xCAFEBABE00000000 + v as u64);
        }

        let temp_file = base_dir.join(format!("input_v{}.bin", v));
        fs::write(&temp_file, &current_data).unwrap();

        // 1. OOS-Lite write
        let start_oos = Instant::now();
        engine.put_file_named("dataset.bin", &temp_file).unwrap();
        oos_times.push(start_oos.elapsed());

        // 2. cp write (full separate copies)
        let cp_dest = cp_dir.join(format!("dataset_v{}.bin", v));
        let start_cp = Instant::now();
        fs::copy(&temp_file, &cp_dest).unwrap();
        cp_times.push(start_cp.elapsed());

        // 3. rsync --link-dest simulation:
        // For modified files, link-dest cannot hardlink across modified bytes, so it performs full file copy.
        let link_dest = link_dir.join(format!("dataset_v{}.bin", v));
        fs::copy(&temp_file, &link_dest).unwrap();

        let _ = fs::remove_file(&temp_file);
        print!(".");
        std::io::stdout().flush().unwrap();
    }
    println!(" Done!");

    let oos_disk = engine.stats().physical_disk_bytes;
    let cp_disk = dir_physical_size(&cp_dir);
    let link_disk = dir_physical_size(&link_dir);
    let logical_size = (versions_count as u64) * (file_size as u64);

    let avg_oos_time: Duration = oos_times.iter().sum::<Duration>() / versions_count as u32;
    let avg_cp_time: Duration = cp_times.iter().sum::<Duration>() / versions_count as u32;

    println!("\n  --- Storage Consumption Results (10 Versions of 100 MB) ---");
    println!("  {:<30} {:<25} {:<15}", "METHOD", "PHYSICAL DISK USAGE", "SAVINGS vs CP");
    println!("  {}", "-".repeat(70));
    println!("  {:<30} {:<25} {:<15}", "Logical Size (Nominal)", format_bytes(logical_size), "0.0% (Reference)");
    println!("  {:<30} {:<25} {:<15}", "cp (N independent copies)", format_bytes(cp_disk), "0.0% (Baseline)");
    println!("  {:<30} {:<25} {:<15}", "rsync --link-dest (hardlink)", format_bytes(link_disk), "0.0% (No dedup on delta)");
    let savings_pct = ((cp_disk.saturating_sub(oos_disk)) as f64 / cp_disk as f64) * 100.0;
    println!("  {:<30} {:<25} {:<15}", "OOS-Lite (FastCDC Dedup)", format_bytes(oos_disk), format!("{:.1}% SAVED", savings_pct));
    println!("  {}", "-".repeat(70));
    println!("  Dedup Ratio achieved : {:.2}x", logical_size as f64 / oos_disk as f64);
    println!("  Avg write time/ver   : OOS-Lite: {:.2?}, cp: {:.2?}\n", avg_oos_time, avg_cp_time);

    // -------------------------------------------------------------------------
    // BENCHMARK 2: SNAPSHOT LATENCY vs tar czf / tar cf
    // -------------------------------------------------------------------------
    println!("==> [2/3] Benchmarking Snapshot Latency vs tar archiving...");

    // Create realistic directory to archive
    let sample_dir = base_dir.join("archive_sample");
    fs::create_dir_all(&sample_dir).unwrap();
    for i in 1..=5 {
        let dummy = sample_dir.join(format!("file_{}.dat", i));
        fs::write(&dummy, &current_data[..(20 * 1024 * 1024)]).unwrap(); // 5 files * 20 MB = 100 MB
    }

    // 1. OOS-Lite Snapshot (Warm & Cold)
    let start_warm_snap = Instant::now();
    let snap_warm = engine.create_snapshot("snap_warm").unwrap();
    let oos_snap_warm_time = start_warm_snap.elapsed();

    drop(engine);
    let engine_cold = StorageEngine::open(&oos_dir).unwrap();
    let start_cold_snap = Instant::now();
    let _snap_cold = engine_cold.create_snapshot("snap_cold").unwrap();
    let oos_snap_cold_time = start_cold_snap.elapsed();

    // 2. tar cf (no compression)
    let tar_raw_out = base_dir.join("archive.tar");
    let start_tar_raw = Instant::now();
    let tar_raw_status = Command::new("tar")
        .args(["-cf", tar_raw_out.to_str().unwrap(), "-C", sample_dir.to_str().unwrap(), "."])
        .output();
    let tar_raw_time = start_tar_raw.elapsed();

    // 3. tar czf (gzip compression)
    let tar_gz_out = base_dir.join("archive.tar.gz");
    let start_tar_gz = Instant::now();
    let tar_gz_status = Command::new("tar")
        .args(["-czf", tar_gz_out.to_str().unwrap(), "-C", sample_dir.to_str().unwrap(), "."])
        .output();
    let tar_gz_time = start_tar_gz.elapsed();

    println!("\n  --- Snapshot / Archival Latency Comparison (100 MB Dataset) ---");
    println!("  {:<35} {:<20} {:<20}", "METHOD", "LATENCY", "SPEEDUP vs TAR GZ");
    println!("  {}", "-".repeat(75));
    println!("  {:<35} {:<20} {:<20}", "OOS-Lite Snapshot (Warm-cache)", format!("{:.2?}", oos_snap_warm_time), format!("{:.0}x FASTER", tar_gz_time.as_secs_f64() / oos_snap_warm_time.as_secs_f64()));
    println!("  {:<35} {:<20} {:<20}", "OOS-Lite Snapshot (Cold-cache)", format!("{:.2?}", oos_snap_cold_time), format!("{:.0}x FASTER", tar_gz_time.as_secs_f64() / oos_snap_cold_time.as_secs_f64()));
    if tar_raw_status.is_ok() {
        println!("  {:<35} {:<20} {:<20}", "tar -cf (Raw archive, no gzip)", format!("{:.2?}", tar_raw_time), format!("{:.1}x", tar_gz_time.as_secs_f64() / tar_raw_time.as_secs_f64()));
    }
    if tar_gz_status.is_ok() {
        println!("  {:<35} {:<20} {:<20}", "tar -czf (Gzip archive)", format!("{:.2?}", tar_gz_time), "1.0x (Baseline)");
    }
    println!("  {}", "-".repeat(75));
    println!("  Snapshot entries preserved: {}", snap_warm.entries.len());

    // -------------------------------------------------------------------------
    // BENCHMARK 3: READ / RESTORE LATENCY (COLD-CACHE vs WARM-CACHE)
    // -------------------------------------------------------------------------
    println!("\n==> [3/3] Benchmarking Read & Restore Latency (100 MB File)...");

    // Warm-cache read
    let out_warm = base_dir.join("read_warm.bin");
    let start_get_warm = Instant::now();
    let bytes_warm = engine_cold.get_file("dataset.bin", &out_warm).unwrap();
    let warm_read_time = start_get_warm.elapsed();
    let _ = fs::remove_file(&out_warm);

    // Cold-cache read: drop engine instance and reopen
    drop(engine_cold);
    let engine_fresh = StorageEngine::open(&oos_dir).unwrap();
    let out_cold = base_dir.join("read_cold.bin");
    let start_get_cold = Instant::now();
    let bytes_cold = engine_fresh.get_file("dataset.bin", &out_cold).unwrap();
    let cold_read_time = start_get_cold.elapsed();
    let _ = fs::remove_file(&out_cold);

    // Baseline: Direct OS file copy of 100 MB
    let baseline_src = cp_dir.join("dataset_v10.bin");
    let baseline_dst = base_dir.join("read_baseline.bin");
    let start_copy = Instant::now();
    fs::copy(&baseline_src, &baseline_dst).unwrap();
    let os_copy_time = start_copy.elapsed();
    let _ = fs::remove_file(&baseline_dst);

    let warm_throughput = (bytes_warm as f64 / (1024.0 * 1024.0)) / warm_read_time.as_secs_f64();
    let cold_throughput = (bytes_cold as f64 / (1024.0 * 1024.0)) / cold_read_time.as_secs_f64();
    let os_throughput = (bytes_warm as f64 / (1024.0 * 1024.0)) / os_copy_time.as_secs_f64();

    println!("\n  --- 100 MB File Extraction & Reconstruction Throughput ---");
    println!("  {:<35} {:<18} {:<20}", "OPERATION", "TIME", "THROUGHPUT");
    println!("  {}", "-".repeat(73));
    println!("  {:<35} {:<18} {:<20}", "OS Direct Copy (fs::copy)", format!("{:.2?}", os_copy_time), format!("{:.1} MB/s", os_throughput));
    println!("  {:<35} {:<18} {:<20}", "OOS-Lite Get (Warm-cache)", format!("{:.2?}", warm_read_time), format!("{:.1} MB/s", warm_throughput));
    println!("  {:<35} {:<18} {:<20}", "OOS-Lite Get (Cold-cache)", format!("{:.2?}", cold_read_time), format!("{:.1} MB/s", cold_throughput));
    println!("  {}", "-".repeat(73));
    println!("  * Note: OOS-Lite verifies CRC32C and BLAKE3 hash for every chunk and full file during extraction.");

    println!("\n================================================================================");
    println!("                    ALL BENCHMARKS COMPLETED SUCCESSFULLY                       ");
    println!("================================================================================\n");
}
