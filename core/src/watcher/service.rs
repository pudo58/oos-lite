use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::{error, info, warn};
use walkdir::WalkDir;

use crate::error::{OosLiteError, Result};
use crate::watcher::config::WatcherConfig;
use crate::watcher::ignore::IgnoreRules;
use crate::StorageEngine;

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingAction {
    CreateOrModify,
    Delete,
    Rename { from: PathBuf, to: PathBuf },
}

pub struct WatcherHandle {
    running: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl WatcherHandle {
    pub fn stop(self) {
        self.running.store(false, Ordering::SeqCst);
        for t in self.threads {
            let _ = t.join();
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

pub struct WatcherService {
    engine: Arc<StorageEngine>,
    config: WatcherConfig,
    ignore_rules: Arc<RwLock<IgnoreRules>>,
    running: Arc<AtomicBool>,
    pending_changes: Arc<Mutex<HashMap<PathBuf, (Instant, PendingAction)>>>,
    last_synced: Arc<Mutex<HashMap<String, Instant>>>,
    needs_reconcile: Arc<AtomicBool>,
    pending_rename_from: Arc<Mutex<Option<(PathBuf, Instant)>>>,
}

impl WatcherService {
    pub fn new(engine: Arc<StorageEngine>, config: WatcherConfig) -> Self {
        let ignore_rules = Arc::new(RwLock::new(IgnoreRules::load(&config.watch_dir)));
        Self {
            engine,
            config,
            ignore_rules,
            running: Arc::new(AtomicBool::new(false)),
            pending_changes: Arc::new(Mutex::new(HashMap::new())),
            last_synced: Arc::new(Mutex::new(HashMap::new())),
            needs_reconcile: Arc::new(AtomicBool::new(false)),
            pending_rename_from: Arc::new(Mutex::new(None)),
        }
    }

    pub fn config(&self) -> &WatcherConfig {
        &self.config
    }

    /// Starts the watcher service and background worker threads, returning a WatcherHandle.
    pub fn start(&self) -> Result<WatcherHandle> {
        self.running.store(true, Ordering::SeqCst);

        // 1. Initial Cold-Start Reconciliation Scan
        info!(
            watch_dir = %self.config.watch_dir.display(),
            "Starting initial cold-start reconciliation scan..."
        );
        if let Err(e) = self.reconciliation_scan() {
            warn!("Cold-start scan encountered error: {}", e);
        }

        // 2. Setup notify channel & watcher
        let (tx, rx) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(tx, Config::default())
            .map_err(|e| OosLiteError::Internal(format!("Failed to initialize notify watcher: {e}")))?;

        watcher
            .watch(&self.config.watch_dir, RecursiveMode::Recursive)
            .map_err(|e| OosLiteError::Internal(format!("Failed to watch directory: {e}")))?;

        let running = Arc::clone(&self.running);
        let pending = Arc::clone(&self.pending_changes);
        let needs_reconcile = Arc::clone(&self.needs_reconcile);
        let watch_dir = self.config.watch_dir.clone();
        let ignore_rules = Arc::clone(&self.ignore_rules);
        let pending_rename = Arc::clone(&self.pending_rename_from);

        // Thread 1: Event receiver
        let t1_running = Arc::clone(&running);
        let t1 = thread::Builder::new()
            .name("oos-watcher-events".to_string())
            .spawn(move || {
                // Keep watcher alive inside thread
                let _watcher = watcher;
                while t1_running.load(Ordering::Relaxed) {
                    match rx.recv_timeout(Duration::from_millis(300)) {
                        Ok(Ok(event)) => {
                            Self::process_notify_event(
                                event,
                                &watch_dir,
                                &ignore_rules,
                                &pending,
                                &pending_rename,
                            );
                        }
                        Ok(Err(e)) => {
                            warn!("Notify watcher reported buffer error: {e}; triggering full reconciliation");
                            needs_reconcile.store(true, Ordering::SeqCst);
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .map_err(|e| OosLiteError::Internal(format!("Failed to spawn event receiver thread: {e}")))?;

        // Thread 2: Debounce queue processor and reconciliation scheduler
        let t2_running = Arc::clone(&running);
        let t2_pending = Arc::clone(&self.pending_changes);
        let t2_last_synced = Arc::clone(&self.last_synced);
        let t2_engine = Arc::clone(&self.engine);
        let t2_config = self.config.clone();
        let t2_ignore = Arc::clone(&self.ignore_rules);
        let t2_reconcile = Arc::clone(&self.needs_reconcile);
        let t2_pending_rename = Arc::clone(&self.pending_rename_from);

        let t2 = thread::Builder::new()
            .name("oos-watcher-worker".to_string())
            .spawn(move || {
                let mut last_reconcile = Instant::now();

                while t2_running.load(Ordering::Relaxed) {
                    // Check if reconciliation is needed (buffer overflow or interval)
                    if t2_reconcile.swap(false, Ordering::SeqCst)
                        || last_reconcile.elapsed() >= t2_config.reconcile_interval
                    {
                        info!("Triggering scheduled/overflow reconciliation scan...");
                        Self::run_reconciliation(
                            &t2_engine,
                            &t2_config,
                            &t2_ignore,
                            &t2_last_synced,
                        );
                        last_reconcile = Instant::now();
                    }

                    // Process pending debounce queue
                    Self::process_pending_queue(
                        &t2_engine,
                        &t2_config,
                        &t2_pending,
                        &t2_last_synced,
                        &t2_pending_rename,
                    );

                    thread::sleep(Duration::from_millis(200));
                }
            })
            .map_err(|e| OosLiteError::Internal(format!("Failed to spawn watcher worker thread: {e}")))?;

        Ok(WatcherHandle {
            running,
            threads: vec![t1, t2],
        })
    }

    fn process_notify_event(
        event: Event,
        watch_dir: &Path,
        ignore_rules: &Arc<RwLock<IgnoreRules>>,
        pending: &Arc<Mutex<HashMap<PathBuf, (Instant, PendingAction)>>>,
        pending_rename_from: &Arc<Mutex<Option<(PathBuf, Instant)>>>,
    ) {
        let rules = ignore_rules.read().unwrap();

        // Check for Rename
        match event.kind {
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
                if event.paths.len() == 2 {
                    Self::queue_rename(event.paths[0].clone(), event.paths[1].clone(), watch_dir, &rules, pending);
                    return;
                }
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                if let Some(from) = event.paths.first() {
                    let mut lock = pending_rename_from.lock().unwrap();
                    *lock = Some((from.clone(), Instant::now()));
                    return;
                }
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                if let Some(to) = event.paths.first() {
                    let mut lock = pending_rename_from.lock().unwrap();
                    if let Some((from, ts)) = lock.take() {
                        if ts.elapsed() < Duration::from_secs(3) {
                            Self::queue_rename(from, to.clone(), watch_dir, &rules, pending);
                            return;
                        }
                    }
                }
            }
            _ => {}
        }

        for path in event.paths {
            let rel = match path.strip_prefix(watch_dir) {
                Ok(r) => r,
                Err(_) => continue,
            };

            if rules.is_ignored(rel) {
                continue;
            }

            let action = match event.kind {
                EventKind::Remove(_) => PendingAction::Delete,
                EventKind::Create(_) | EventKind::Modify(_) => PendingAction::CreateOrModify,
                _ => continue,
            };

            let mut lock = pending.lock().unwrap();
            lock.insert(path, (Instant::now(), action));
        }
    }

    fn queue_rename(
        from: PathBuf,
        to: PathBuf,
        watch_dir: &Path,
        rules: &IgnoreRules,
        pending: &Arc<Mutex<HashMap<PathBuf, (Instant, PendingAction)>>>,
    ) {
        let from_ignored = from
            .strip_prefix(watch_dir)
            .map(|r| rules.is_ignored(r))
            .unwrap_or(true);
        let to_ignored = to
            .strip_prefix(watch_dir)
            .map(|r| rules.is_ignored(r))
            .unwrap_or(true);

        if !from_ignored || !to_ignored {
            let mut lock = pending.lock().unwrap();
            lock.remove(&from);
            lock.insert(
                to.clone(),
                (
                    Instant::now(),
                    PendingAction::Rename { from, to },
                ),
            );
        }
    }

    fn process_pending_queue(
        engine: &Arc<StorageEngine>,
        config: &WatcherConfig,
        pending: &Arc<Mutex<HashMap<PathBuf, (Instant, PendingAction)>>>,
        last_synced: &Arc<Mutex<HashMap<String, Instant>>>,
        pending_rename_from: &Arc<Mutex<Option<(PathBuf, Instant)>>>,
    ) {
        // Expire lone RenameMode::From after 2s
        {
            let mut r_lock = pending_rename_from.lock().unwrap();
            if let Some((_, ts)) = r_lock.as_ref() {
                if ts.elapsed() >= Duration::from_secs(2) {
                    let (from_path, _) = r_lock.take().unwrap();
                    let mut p_lock = pending.lock().unwrap();
                    p_lock.insert(from_path, (Instant::now(), PendingAction::Delete));
                }
            }
        }
        let ready_items: Vec<(PathBuf, PendingAction)> = {
            let mut lock = pending.lock().unwrap();
            let mut ready = Vec::new();
            let now = Instant::now();

            let ready_keys: Vec<PathBuf> = lock
                .iter()
                .filter(|(_, (ts, _))| now.duration_since(*ts) >= config.debounce_duration)
                .map(|(p, _)| p.clone())
                .collect();

            for k in ready_keys {
                if let Some((_, action)) = lock.remove(&k) {
                    ready.push((k, action));
                }
            }

            ready
        };

        for (path, action) in ready_items {
            match action {
                PendingAction::CreateOrModify => {
                    if !path.exists() || path.is_dir() {
                        continue;
                    }

                    let rel = match path.strip_prefix(&config.watch_dir) {
                        Ok(r) => r,
                        Err(_) => continue,
                    };
                    let logical_name = rel.to_string_lossy().replace('\\', "/");

                    // Cooldown check
                    {
                        let sync_lock = last_synced.lock().unwrap();
                        if let Some(last) = sync_lock.get(&logical_name) {
                            if last.elapsed() < config.cooldown_window {
                                // Still inside cooldown window; defer action
                                drop(sync_lock);
                                let mut p_lock = pending.lock().unwrap();
                                p_lock.entry(path).or_insert((Instant::now(), PendingAction::CreateOrModify));
                                continue;
                            }
                        }
                    }

                    // Attempt safe ingest
                    match engine.put_file_named(&logical_name, &path) {
                        Ok(summary) => {
                            info!(
                                name = %logical_name,
                                version = summary.version,
                                size = summary.total_bytes,
                                "Auto-Vault: Successfully committed file version"
                            );
                            let mut sync_lock = last_synced.lock().unwrap();
                            sync_lock.insert(logical_name, Instant::now());
                        }
                        Err(err) => {
                            if Self::is_sharing_violation(&err) {
                                // File is actively locked by another program (e.g. Photoshop).
                                // Defer and retry in the next debounce cycle.
                                info!(
                                    name = %logical_name,
                                    "File locked by editor (SharingViolation); deferring retry..."
                                );
                                let mut p_lock = pending.lock().unwrap();
                                p_lock.insert(path, (Instant::now(), PendingAction::CreateOrModify));
                            } else {
                                error!(name = %logical_name, error = %err, "Auto-Vault ingest error");
                            }
                        }
                    }
                }
                PendingAction::Delete => {
                    let rel = match path.strip_prefix(&config.watch_dir) {
                        Ok(r) => r,
                        Err(_) => continue,
                    };
                    let logical_name = rel.to_string_lossy().replace('\\', "/");

                    match engine.delete_file(&logical_name) {
                        Ok(deleted) => {
                            if deleted {
                                info!(name = %logical_name, "Auto-Vault: Removed deleted file mapping");
                                let mut sync_lock = last_synced.lock().unwrap();
                                sync_lock.remove(&logical_name);
                            }
                        }
                        Err(e) => error!(name = %logical_name, error = %e, "Failed to delete file mapping"),
                    }
                }
                PendingAction::Rename { from, to } => {
                    let from_rel = from.strip_prefix(&config.watch_dir).ok();
                    let to_rel = to.strip_prefix(&config.watch_dir).ok();

                    if let (Some(f_rel), Some(t_rel)) = (from_rel, to_rel) {
                        let f_name = f_rel.to_string_lossy().replace('\\', "/");
                        let t_name = t_rel.to_string_lossy().replace('\\', "/");

                        match engine.rename_file(&f_name, &t_name) {
                            Ok(true) => {
                                info!(
                                    from = %f_name,
                                    to = %t_name,
                                    "Auto-Vault: Renamed file preserving version history"
                                );
                                let mut sync_lock = last_synced.lock().unwrap();
                                if let Some(ts) = sync_lock.remove(&f_name) {
                                    sync_lock.insert(t_name.clone(), ts);
                                }
                            }
                            Ok(false) => {
                                // Old name didn't exist in store, just ingest the new file
                                if to.exists() && to.is_file() {
                                    let _ = engine.put_file_named(&t_name, &to);
                                }
                            }
                            Err(e) => error!(from = %f_name, to = %t_name, error = %e, "Failed to rename file"),
                        }
                    }
                }
            }
        }
    }

    /// Performs a full directory scan, ingesting missing or modified files and pruning removed files.
    pub fn reconciliation_scan(&self) -> Result<()> {
        Self::run_reconciliation(
            &self.engine,
            &self.config,
            &self.ignore_rules,
            &self.last_synced,
        );
        Ok(())
    }

    fn run_reconciliation(
        engine: &Arc<StorageEngine>,
        config: &WatcherConfig,
        ignore_rules: &Arc<RwLock<IgnoreRules>>,
        last_synced: &Arc<Mutex<HashMap<String, Instant>>>,
    ) {
        if !config.watch_dir.exists() {
            return;
        }

        let rules = ignore_rules.read().unwrap();
        let mut scanned_files = 0usize;
        let mut ingested_files = 0usize;

        for entry in WalkDir::new(&config.watch_dir)
            .into_iter()
            .filter_entry(|e| {
                if let Ok(rel) = e.path().strip_prefix(&config.watch_dir) {
                    if rel.as_os_str().is_empty() {
                        return true;
                    }
                    !rules.is_ignored(rel)
                } else {
                    false
                }
            })
            .flatten()
        {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }

            scanned_files += 1;
            let rel = match p.strip_prefix(&config.watch_dir) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let logical_name = rel.to_string_lossy().replace('\\', "/");

            // Check if file is already present with same size/hash
            let needs_put = match engine.metadata_store().resolve_name(&logical_name) {
                Ok(Some(id)) => match engine.metadata_store().get_object(&id) {
                    Ok(Some(rec)) => {
                        let latest_m = rec.latest_manifest_id();
                        match engine.metadata_store().get_manifest(latest_m) {
                            Ok(Some(manifest)) => {
                                let meta_len = entry.metadata().map(|m| m.len()).unwrap_or(0);
                                meta_len != manifest.total_size
                            }
                            _ => true,
                        }
                    }
                    _ => true,
                },
                _ => true,
            };

            if needs_put {
                match engine.put_file_named(&logical_name, p) {
                    Ok(_) => {
                        ingested_files += 1;
                        let mut sync_lock = last_synced.lock().unwrap();
                        sync_lock.insert(logical_name, Instant::now());
                    }
                    Err(e) => {
                        if !Self::is_sharing_violation(&e) {
                            warn!(file = %logical_name, error = %e, "Reconciliation ingest failed");
                        }
                    }
                }
            }

            // Cold start rate limiting: cooperative throttling to avoid freezing user I/O
            if config.throttle_ms > 0 {
                thread::sleep(Duration::from_millis(config.throttle_ms));
            }
        }

        info!(
            scanned = scanned_files,
            ingested = ingested_files,
            "Reconciliation scan completed"
        );
    }

    /// Detects Windows SharingViolation (os error 32) or LockViolation (os error 33)
    pub fn is_sharing_violation(err: &OosLiteError) -> bool {
        if let OosLiteError::Io(ref io_err) = err {
            if let Some(code) = io_err.raw_os_error() {
                return code == 32 || code == 33;
            }
        }
        false
    }
}
