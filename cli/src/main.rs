use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use oos_lite_core::StorageEngine;

mod ui;
mod mount;
mod tray;

#[derive(Parser)]
#[command(name = "oos-lite", author, version, about = "OOS-Lite Content-Addressed File Storage CLI", long_about = None)]
struct Cli {
    #[arg(short, long, alias = "store", help = "Store directory path", default_value = ".oos-store")]
    store_dir: PathBuf,

    #[arg(long, help = "Encryption passphrase for store (or set OOS_PASSWORD env var)")]
    password: Option<String>,

    #[arg(long, help = "Path to file containing encryption passphrase")]
    password_file: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

impl std::fmt::Debug for Cli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cli")
            .field("store_dir", &self.store_dir)
            .field("password", &self.password.as_ref().map(|_| "***REDACTED***"))
            .field("password_file", &self.password_file)
            .field("command", &self.command)
            .finish()
    }
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(about = "Initialize a new store (optionally encrypted with --password or --password-file)")]
    Init,
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
    #[command(about = "Launch OOS-Lite Native Desktop Application", alias = "desktop", alias = "gui")]
    App,
    #[command(about = "Launch embedded Web UI Dashboard")]
    Ui {
        #[arg(long, help = "Host address to listen on", default_value = "127.0.0.1")]
        host: String,
        #[arg(short, long, help = "Port to listen on", default_value_t = 3000)]
        port: u16,
        #[arg(long, help = "Do not automatically open browser")]
        no_open: bool,
    },
    #[command(about = "Mount OOS-Lite store as a read-only filesystem (FUSE / WebDAV)")]
    Mount {
        #[arg(help = "Mount point directory path (Linux/macOS) or Drive letter like Z: (Windows)")]
        target: Option<String>,
        #[arg(long, help = "Force WebDAV mode", default_value_t = false)]
        webdav: bool,
        #[arg(long, help = "WebDAV server port", default_value_t = 8080)]
        port: u16,
        #[arg(
            long,
            help = "Memory cache limit in MiB for decompressed chunks",
            default_value_t = 128
        )]
        cache_mb: usize,
    },
    #[command(about = "Watch a directory and automatically version modified files (Auto-Vault)")]
    Watch {
        #[arg(help = "Directory path to watch")]
        dir: PathBuf,
        #[arg(long, help = "Debounce duration in seconds before commit", default_value_t = 3)]
        debounce_secs: u64,
        #[arg(long, help = "Cooldown window in seconds between versions of the same file", default_value_t = 60)]
        cooldown_secs: u64,
        #[arg(long, help = "I/O throttle sleep in ms per file during cold-start scan", default_value_t = 10)]
        throttle_ms: u64,
    },
    #[command(about = "Prune historical versions of stored files")]
    Prune {
        #[arg(short, long, help = "Number of latest versions to keep", default_value_t = 10)]
        keep: usize,
        #[arg(help = "Optional specific file name to prune (defaults to all files)")]
        name: Option<String>,
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

fn resolve_password(cli: &Cli) -> anyhow::Result<Option<String>> {
    if let Some(ref p) = cli.password {
        return Ok(Some(p.clone()));
    }
    if let Some(ref path) = cli.password_file {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read password file '{}': {}", path.display(), e))?;
        let trimmed = content.trim_end_matches(&['\r', '\n'][..]).to_string();
        return Ok(Some(trimmed));
    }
    if let Ok(p) = std::env::var("OOS_PASSWORD") {
        return Ok(Some(p));
    }
    Ok(None)
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    info!("OOS-Lite CLI initialized with store at: {}", cli.store_dir.display());

    let password = resolve_password(&cli)?;

    if let Commands::Init = cli.command {
        if let Some(ref pwd) = password {
            StorageEngine::init_encrypted(&cli.store_dir, pwd)?;
            println!("✓ Initialized encrypted OOS-Lite store at {}", cli.store_dir.display());
        } else {
            StorageEngine::open(&cli.store_dir)?;
            println!("✓ Initialized unencrypted OOS-Lite store at {}", cli.store_dir.display());
        }
        return Ok(());
    }

    let engine = if let Some(ref pwd) = password {
        Arc::new(StorageEngine::open_with_password(&cli.store_dir, pwd)?)
    } else {
        match StorageEngine::open(&cli.store_dir) {
            Ok(eng) => Arc::new(eng),
            Err(oos_lite_core::error::OosLiteError::PasswordRequired) => {
                eprintln!("Error: This store is encrypted. Please provide --password, --password-file, or set the OOS_PASSWORD environment variable.");
                std::process::exit(1);
            }
            Err(e) => return Err(e.into()),
        }
    };

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
        Commands::App => {
            #[cfg(windows)]
            tray::windows::spawn_system_tray();
            ui::start_ui_server(engine, "127.0.0.1", 3000, false, true)?;
        }
        Commands::Ui { host, port, no_open } => {
            ui::start_ui_server(engine, &host, port, no_open, false)?;
        }
        Commands::Mount { target, webdav, port, cache_mb } => {
            mount::mount(engine, target.as_deref(), webdav, port, cache_mb)?;
        }
        Commands::Watch {
            dir,
            debounce_secs,
            cooldown_secs,
            throttle_ms,
        } => {
            use std::time::Duration;
            use oos_lite_core::watcher::{WatcherConfig, WatcherService};

            let config = WatcherConfig::new(&dir)
                .with_debounce(Duration::from_secs(debounce_secs))
                .with_cooldown(Duration::from_secs(cooldown_secs))
                .with_throttle_ms(throttle_ms);

            println!("============================================================");
            println!("            OOS-LITE AUTO-VAULT DIRECTORY WATCHER           ");
            println!("============================================================");
            println!("  Watched Directory  : {}", dir.display());
            println!("  Store Directory    : {}", cli.store_dir.display());
            println!("  Debounce Window    : {}s", debounce_secs);
            println!("  Cooldown Window    : {}s", cooldown_secs);
            println!("  Cold-Start Throttle: {}ms/file", throttle_ms);
            println!("============================================================");
            println!("==> Watching for file changes. Press Ctrl+C to stop...");

            let service = WatcherService::new(Arc::clone(&engine), config);
            let handle = service.start()?;

            let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
            let r_ctrlc = Arc::clone(&running);
            ctrlc::set_handler(move || {
                println!("\n==> Stopping directory watcher...");
                r_ctrlc.store(false, std::sync::atomic::Ordering::SeqCst);
            })
            .ok();

            while running.load(std::sync::atomic::Ordering::SeqCst) && handle.is_running() {
                std::thread::sleep(Duration::from_millis(500));
            }

            handle.stop();
            println!("✓ Auto-Vault watcher stopped cleanly.");
        }
        Commands::Prune { keep, name } => {
            if let Some(target) = name {
                let pruned = engine.prune_file_versions(&target, keep)?;
                println!("✓ Pruned {} older version(s) for '{}' (keeping latest {})", pruned, target, keep);
            } else {
                let pruned = engine.prune_all(keep)?;
                println!("✓ Pruned {} older version(s) across all files (keeping latest {} per file)", pruned, keep);
            }
            println!("  Run 'oos-lite gc' to reclaim disk space from pruned versions.");
        }
        Commands::Init => unreachable!(),
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
