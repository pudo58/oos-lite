use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use oos_lite_core::StorageEngine;

mod ui;

#[derive(Parser, Debug)]
#[command(name = "oos-lite", author, version, about = "OOS-Lite Content-Addressed File Storage CLI", long_about = None)]
struct Cli {
    #[arg(short, long, alias = "store", help = "Store directory path", default_value = ".oos-store")]
    store_dir: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(about = "Put a file into the store")]
    Put {
        #[arg(help = "Path to the file to store")]
        path: PathBuf,
        #[arg(short, long, help = "Logical file name to assign (defaults to file basename)")]
        name: Option<String>,
    },
    #[command(about = "Get a file from the store")]
    Get {
        #[arg(help = "File name or ObjectID or ManifestID")]
        target: String,
        #[arg(help = "Output destination path")]
        out: PathBuf,
        #[arg(short, long, help = "Specific version number to extract (defaults to latest)")]
        version: Option<u32>,
    },
    #[command(about = "List stored files")]
    List,
    #[command(about = "Show version history of a file")]
    Versions {
        #[arg(help = "File name or ObjectID")]
        target: String,
    },
    #[command(about = "Delete a file by logical name", alias = "delete")]
    Rm {
        #[arg(help = "Logical file name to delete")]
        name: String,
    },
    #[command(about = "Manage snapshots")]
    Snapshot {
        #[command(subcommand)]
        action: SnapshotCommands,
    },
    #[command(about = "Display store statistics")]
    Stats,
    #[command(about = "Garbage collect unreferenced chunks")]
    Gc,
    #[command(about = "Verify data integrity")]
    Fsck,
    #[command(about = "Launch embedded Web UI Dashboard")]
    Ui {
        #[arg(long, help = "Host address to listen on", default_value = "127.0.0.1")]
        host: String,
        #[arg(short, long, help = "Port to listen on", default_value_t = 3000)]
        port: u16,
        #[arg(long, help = "Do not automatically open browser")]
        no_open: bool,
    },
}

#[derive(Subcommand, Debug)]
enum SnapshotCommands {
    #[command(about = "Create a new snapshot")]
    Create {
        #[arg(help = "Snapshot label")]
        label: String,
    },
    #[command(about = "List all snapshots")]
    List,
    #[command(about = "Restore a snapshot to a directory")]
    Restore {
        #[arg(help = "Snapshot label")]
        label: String,
        #[arg(help = "Destination directory")]
        dir: PathBuf,
    },
    #[command(about = "Delete a snapshot")]
    Delete {
        #[arg(help = "Snapshot label to delete")]
        label: String,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    info!("OOS-Lite CLI initialized with store at: {}", cli.store_dir.display());

    let engine = Arc::new(StorageEngine::open(&cli.store_dir)?);

    match cli.command {
        Commands::Put { path, name } => {
            let logical_name = name.unwrap_or_else(|| {
                path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unnamed_file")
                    .to_string()
            });

            println!("==> Storing file: {} as '{}'", path.display(), logical_name);
            let summary = engine.put_file_named(&logical_name, &path)?;
            println!("✓ Successfully stored file!");
            println!("  Logical Name: {}", logical_name);
            println!("  Object ID   : {}", summary.object_id);
            println!("  Version     : #{}", summary.version);
            println!("  Manifest ID : {}", summary.manifest_id);
            println!("  Total size  : {} bytes ({:.2} KiB)", summary.total_bytes, summary.total_bytes as f64 / 1024.0);
            println!("  Total chunks: {}", summary.chunk_count);
            println!("  New chunks  : {} (written to segments)", summary.new_chunks);
            println!("  Dedup chunks: {} (reused existing chunks)", summary.dedup_chunks);
        }
        Commands::Get { target, out, version } => {
            if let Some(v) = version {
                println!("==> Extracting [{}] (version #{}) to {}", target, v, out.display());
            } else {
                println!("==> Extracting [{}] to {}", target, out.display());
            }
            let bytes = engine.get_file_version(&target, version, &out)?;
            println!("✓ Extracted {} bytes to {}", bytes, out.display());
        }
        Commands::List => {
            let list = engine.list_files()?;
            println!("==> Stored Files ({} items):", list.len());
            println!("{:<25} {:<34} {:<10} {:<12}", "NAME", "OBJECT ID", "VERSION", "SIZE");
            println!("{}", "-".repeat(85));
            for (name, id, record) in list {
                let latest = record.versions.last();
                let size = latest.map(|v| v.size_bytes).unwrap_or(0);
                println!("{:<25} {:<34} #{:<9} {:<12} bytes", name, id.to_hex(), record.latest_version, size);
            }
        }
        Commands::Versions { target } => {
            let versions = engine.get_versions(&target)?;
            println!("==> Version History for [{}]:", target);
            println!("{:<10} {:<20} {:<14} {:<64}", "VERSION", "TIMESTAMP", "SIZE", "MANIFEST ID");
            println!("{}", "-".repeat(110));
            for v in versions {
                let dt = chrono_format(v.created_at);
                println!("#{:<9} {:<20} {:<14} bytes {:<64}", v.version, dt, v.size_bytes, v.manifest_id);
            }
        }
        Commands::Stats => {
            let stats = engine.stats();
            println!("============================================================");
            println!("                OOS-LITE STORE STATISTICS                   ");
            println!("============================================================");
            println!("  Store path           : {}", cli.store_dir.display());
            println!("  Total Named Objects  : {}", stats.total_objects);
            println!("  Total Snapshots      : {}", stats.total_snapshots);
            println!("  Total Manifests      : {}", stats.total_manifests);
            println!("  Total Unique Chunks  : {}", stats.total_chunks);
            println!("------------------------------------------------------------");
            println!("  Logical Size (Latest): {}", format_bytes(stats.latest_logical_bytes));
            println!("  Logical Size (All)   : {}", format_bytes(stats.logical_bytes));
            println!("  Unique Chunks Payload: {}", format_bytes(stats.unique_chunks_bytes));
            println!("  Physical Disk Usage  : {}", format_bytes(stats.physical_disk_bytes));
            println!("------------------------------------------------------------");
            println!("  Deduplication Ratio  : {:.2}x", stats.dedup_ratio);
            println!("  Storage Space Savings: {:.1}%", stats.space_savings_pct);
            println!("============================================================");
        }
        Commands::Snapshot { action } => match action {
            SnapshotCommands::Create { label } => {
                let snap = engine.create_snapshot(&label)?;
                println!("✓ Successfully created snapshot '{}' ({} files)", snap.label, snap.entries.len());
            }
            SnapshotCommands::List => {
                let snapshots = engine.list_snapshots()?;
                if snapshots.is_empty() {
                    println!("No snapshots found.");
                } else {
                    println!("==> Snapshots ({} items):", snapshots.len());
                    println!("{:<20} {:<20} {:<10}", "LABEL", "CREATED", "FILES");
                    println!("{}", "-".repeat(50));
                    for s in snapshots {
                        println!("{:<20} {:<20} {:<10}", s.label, chrono_format(s.created_at), s.entries.len());
                    }
                }
            }
            SnapshotCommands::Restore { label, dir } => {
                let count = engine.restore_snapshot(&label, &dir)?;
                println!("✓ Restored {} files from snapshot '{}' to {}", count, label, dir.display());
            }
            SnapshotCommands::Delete { label } => {
                let deleted = engine.delete_snapshot(&label)?;
                if deleted {
                    println!("✓ Successfully deleted snapshot '{}'", label);
                } else {
                    println!("Snapshot '{}' not found.", label);
                }
            }
        },
        Commands::Rm { name } => {
            let deleted = engine.delete_file(&name)?;
            if deleted {
                println!("✓ Successfully deleted file '{}'", name);
            } else {
                println!("File '{}' not found in store.", name);
            }
        }
        Commands::Gc => {
            println!("==> Running Mark-and-Sweep Garbage Collection...");
            let stats = engine.gc()?;
            println!("==> Garbage Collection Results:");
            println!("  Live roots scanned    : {}", stats.live_roots);
            println!("  Reachable chunks      : {}", stats.reachable_chunks);
            println!("  Chunks reclaimed      : {}", stats.chunks_reclaimed);
            println!("  Manifests reclaimed   : {}", stats.manifests_reclaimed);
            println!("  Active chunks retained: {}", stats.active_chunks_retained);
            println!("✓ Garbage collection completed successfully.");
        }
        Commands::Fsck => {
            println!("==> Running OOS-Lite File System Consistency Check (fsck)...");
            let report = engine.fsck()?;
            println!("============================================================");
            println!("                     FSCK REPORT                            ");
            println!("============================================================");
            println!("  Segments checked     : {}", report.segments_checked);
            println!("  Chunks verified      : {}", report.chunks_checked);
            println!("  Manifests verified   : {}", report.manifests_checked);
            println!("  Named objects checked: {}", report.objects_checked);
            println!("  Corrupted chunks     : {}", report.corrupted_chunks);
            println!("  Missing chunks       : {}", report.missing_chunks);
            println!("------------------------------------------------------------");
            if report.is_healthy {
                println!("✓ Health Status: STORE IS 100% HEALTHY AND CONSISTENT");
            } else {
                println!("✗ Health Status: CORRUPTION OR INCONSISTENCIES DETECTED!");
                println!("Errors encountered ({}):", report.errors.len());
                for (i, err) in report.errors.iter().enumerate() {
                    println!("  [{}] {}", i + 1, err);
                }
                anyhow::bail!("FSCK check failed with {} error(s)", report.errors.len());
            }
            println!("============================================================");
        }
        Commands::Ui { host, port, no_open } => {
            ui::start_ui_server(engine, &host, port, no_open)?;
        }
    }

    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GiB ({} bytes)", b / GIB, bytes)
    } else if b >= MIB {
        format!("{:.2} MiB ({} bytes)", b / MIB, bytes)
    } else if b >= KIB {
        format!("{:.2} KiB ({} bytes)", b / KIB, bytes)
    } else {
        format!("{} bytes", bytes)
    }
}

fn chrono_format(ts_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let diff = now.saturating_sub(ts_secs);
    if diff < 60 {
        format!("{}s ago", diff)
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}
