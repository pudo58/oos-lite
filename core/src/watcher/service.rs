use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

const PHASE_STARTING: u8 = 0;
const PHASE_SCANNING: u8 = 1;
const PHASE_WATCHING: u8 = 2;
const PHASE_STOPPING: u8 = 3;
const PHASE_STOPPED: u8 = 4;

#[derive(Debug, Clone)]
pub struct WatcherStatusSnapshot {
    pub phase: &'static str,
    pub scanned_files: u64,
    pub ingested_files: u64,
    pub removed_files: u64,
    pub pending_files: u64,
    pub error_count: u64,
    pub last_error: Option<String>,
    pub last_sync_unix: Option<u64>,
}

struct WatcherMetrics {
    phase: AtomicU8,
    scanned_files: AtomicU64,
    ingested_files: AtomicU64,
    removed_files: AtomicU64,
    pending_files: AtomicU64,
    error_count: AtomicU64,
    last_error: Mutex<Option<String>>,
    last_sync_unix: AtomicU64,
}

impl WatcherMetrics {
    fn new() -> Self {
        Self {
            phase: AtomicU8::new(PHASE_STARTING),
            scanned_files: AtomicU64::new(0),
            ingested_files: AtomicU64::new(0),
            removed_files: AtomicU64::new(0),
            pending_files: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            last_error: Mutex::new(None),
            last_sync_unix: AtomicU64::new(0),
        }
    }

    fn record_error(&self, error: impl ToString) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
        *self.last_error.lock().unwrap_or_else(|p| p.into_inner()) = Some(error.to_string());
    }

    fn record_sync(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .max(1);
        self.last_sync_unix.store(now, Ordering::Relaxed);
    }

    fn snapshot(&self) -> WatcherStatusSnapshot {
        let phase = match self.phase.load(Ordering::Relaxed) {
            PHASE_STARTING => "starting",
            PHASE_SCANNING => "scanning",
            PHASE_WATCHING => "watching",
            PHASE_STOPPING => "stopping",
            _ => "stopped",
        };
        let last_sync = self.last_sync_unix.load(Ordering::Relaxed);
        WatcherStatusSnapshot {
            phase,
            scanned_files: self.scanned_files.load(Ordering::Relaxed),
            ingested_files: self.ingested_files.load(Ordering::Relaxed),
            removed_files: self.removed_files.load(Ordering::Relaxed),
            pending_files: self.pending_files.load(Ordering::Relaxed),
            error_count: self.error_count.load(Ordering::Relaxed),
            last_error: self
                .last_error
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone(),
            last_sync_unix: (last_sync != 0).then_some(last_sync),
        }
    }
}

pub struct WatcherHandle {
    running: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    metrics: Arc<WatcherMetrics>,
}

impl WatcherHandle {
    pub fn stop(self) {
        self.metrics.phase.store(PHASE_STOPPING, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
        for t in self.threads {
            let _ = t.join();
        }
        self.metrics.phase.store(PHASE_STOPPED, Ordering::SeqCst);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst) && self.threads.iter().all(|t| !t.is_finished())
    }

    pub fn status(&self) -> WatcherStatusSnapshot {
        self.metrics.snapshot()
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
    pending_rename_from: Arc<Mutex<VecDeque<(PathBuf, Instant)>>>,
    metrics: Arc<WatcherMetrics>,
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
            pending_rename_from: Arc::new(Mutex::new(VecDeque::new())),
            metrics: Arc::new(WatcherMetrics::new()),
        }
    }

    pub fn config(&self) -> &WatcherConfig {
        &self.config
    }

    /// Starts the watcher service and background worker threads, returning a WatcherHandle.
    pub fn start(&self) -> Result<WatcherHandle> {
        let watch_dir_canonical = self.config.watch_dir.canonicalize()?;
        let store_dir_canonical = self.engine.root_dir().canonicalize()?;
        if watch_dir_canonical.starts_with(&store_dir_canonical)
            || store_dir_canonical.starts_with(&watch_dir_canonical)
        {
            return Err(OosLiteError::InvalidWatchScope(format!(
                "watched folder must not overlap the OOS store ({})",
                store_dir_canonical.display()
            )));
        }

        // Register the OS watcher before scanning so changes made during the
        // initial scan are captured in the pending queue.
        let (tx, rx) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(tx, Config::default()).map_err(|e| {
            OosLiteError::Internal(format!("Failed to initialize notify watcher: {e}"))
        })?;

        watcher
            .watch(&self.config.watch_dir, RecursiveMode::Recursive)
            .map_err(|e| OosLiteError::Internal(format!("Failed to watch directory: {e}")))?;

        self.running.store(true, Ordering::SeqCst);
        self.metrics.phase.store(PHASE_SCANNING, Ordering::SeqCst);

        let running = Arc::clone(&self.running);
        let pending = Arc::clone(&self.pending_changes);
        let needs_reconcile = Arc::clone(&self.needs_reconcile);
        let watch_dir = self.config.watch_dir.clone();
        let ignore_rules = Arc::clone(&self.ignore_rules);
        let pending_rename = Arc::clone(&self.pending_rename_from);
        let event_metrics = Arc::clone(&self.metrics);

        // Thread 1: Event receiver
        let t1_running = Arc::clone(&running);
        let t1 = thread::Builder::new()
            .name("oos-watcher-events".to_string())
            .spawn(move || {
                // Keep watcher alive inside thread
                let _watcher = watcher;
                while t1_running.load(Ordering::Relaxed) {
                    match rx.recv_timeout(Duration::from_millis(50)) {
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
                            event_metrics.record_error(e);
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
        let t2_metrics = Arc::clone(&self.metrics);

        let t2 = match thread::Builder::new()
            .name("oos-watcher-worker".to_string())
            .spawn(move || {
                info!(
                    watch_dir = %t2_config.watch_dir.display(),
                    "Starting initial cold-start reconciliation scan..."
                );
                let debounce_started = Instant::now();
                while t2_running.load(Ordering::Relaxed)
                    && debounce_started.elapsed() < t2_config.debounce_duration
                {
                    thread::sleep(Duration::from_millis(50));
                }
                Self::run_reconciliation(
                    &t2_engine,
                    &t2_config,
                    &t2_ignore,
                    &t2_last_synced,
                    Some(&t2_running),
                    &t2_metrics,
                    Some(&t2_pending),
                );
                if t2_running.load(Ordering::Relaxed) {
                    t2_metrics.phase.store(PHASE_WATCHING, Ordering::SeqCst);
                }
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
                            Some(&t2_running),
                            &t2_metrics,
                            Some(&t2_pending),
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
                        &t2_metrics,
                    );

                    thread::sleep(Duration::from_millis(50));
                }
                t2_metrics.phase.store(PHASE_STOPPED, Ordering::SeqCst);
            }) {
            Ok(thread) => thread,
            Err(e) => {
                self.running.store(false, Ordering::SeqCst);
                let _ = t1.join();
                return Err(OosLiteError::Internal(format!(
                    "Failed to spawn watcher worker thread: {e}"
                )));
            }
        };

        Ok(WatcherHandle {
            running,
            threads: vec![t1, t2],
            metrics: Arc::clone(&self.metrics),
        })
    }

    fn process_notify_event(
        event: Event,
        watch_dir: &Path,
        ignore_rules: &Arc<RwLock<IgnoreRules>>,
        pending: &Arc<Mutex<HashMap<PathBuf, (Instant, PendingAction)>>>,
        pending_rename_from: &Arc<Mutex<VecDeque<(PathBuf, Instant)>>>,
    ) {
        let rules = ignore_rules.read().unwrap();

        // Check for Rename
        match event.kind {
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
                if event.paths.len() == 2 {
                    Self::queue_rename(
                        event.paths[0].clone(),
                        event.paths[1].clone(),
                        watch_dir,
                        &rules,
                        pending,
                    );
                    return;
                }
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                if let Some(from) = event.paths.first() {
                    let mut lock = pending_rename_from.lock().unwrap();
                    lock.push_back((from.clone(), Instant::now()));
                    return;
                }
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                if let Some(to) = event.paths.first() {
                    let mut lock = pending_rename_from.lock().unwrap();
                    if let Some((from, ts)) = lock.pop_front() {
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

        let mut lock = pending.lock().unwrap();
        lock.remove(&from);
        match (from_ignored, to_ignored) {
            (false, false) => {
                lock.insert(
                    to.clone(),
                    (Instant::now(), PendingAction::Rename { from, to }),
                );
            }
            (false, true) => {
                lock.insert(from, (Instant::now(), PendingAction::Delete));
            }
            (true, false) => {
                lock.insert(to, (Instant::now(), PendingAction::CreateOrModify));
            }
            (true, true) => {}
        }
    }

    fn watch_root_key(path: &Path) -> String {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let key = canonical.to_string_lossy().to_string();
        #[cfg(windows)]
        return key.to_lowercase();
        #[cfg(not(windows))]
        key
    }

    fn ingest_if_changed(engine: &StorageEngine, logical_name: &str, path: &Path) -> Result<bool> {
        for attempt in 0..3 {
            match engine.source_matches_latest(logical_name, path) {
                Ok(true) => return Ok(false),
                Ok(false) => match engine.put_file_named(logical_name, path) {
                    Ok(_) => return Ok(true),
                    Err(OosLiteError::SourceChanged(_)) if attempt < 2 => {
                        thread::sleep(Duration::from_millis(75));
                    }
                    Err(error) => return Err(error),
                },
                Err(OosLiteError::SourceChanged(_)) if attempt < 2 => {
                    thread::sleep(Duration::from_millis(75));
                }
                Err(error) => return Err(error),
            }
        }
        Err(OosLiteError::SourceChanged(path.display().to_string()))
    }

    fn process_pending_queue(
        engine: &Arc<StorageEngine>,
        config: &WatcherConfig,
        pending: &Arc<Mutex<HashMap<PathBuf, (Instant, PendingAction)>>>,
        last_synced: &Arc<Mutex<HashMap<String, Instant>>>,
        pending_rename_from: &Arc<Mutex<VecDeque<(PathBuf, Instant)>>>,
        metrics: &Arc<WatcherMetrics>,
    ) {
        let watch_root = Self::watch_root_key(&config.watch_dir);
        // Expire lone RenameMode::From after 2s
        {
            let mut r_lock = pending_rename_from.lock().unwrap();
            while r_lock
                .front()
                .is_some_and(|(_, ts)| ts.elapsed() >= Duration::from_secs(2))
            {
                let (from_path, _) = r_lock.pop_front().unwrap();
                let mut p_lock = pending.lock().unwrap();
                p_lock.insert(from_path, (Instant::now(), PendingAction::Delete));
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
        metrics
            .pending_files
            .store(pending.lock().unwrap().len() as u64, Ordering::Relaxed);

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
                                p_lock
                                    .entry(path)
                                    .or_insert((Instant::now(), PendingAction::CreateOrModify));
                                continue;
                            }
                        }
                    }

                    // Attempt safe ingest
                    match Self::ingest_if_changed(engine, &logical_name, &path) {
                        Ok(changed) => {
                            info!(
                                name = %logical_name,
                                changed,
                                "Auto-Vault: Successfully committed file version"
                            );
                            if let Err(error) = engine
                                .metadata_store()
                                .mark_watcher_file(&watch_root, &logical_name)
                            {
                                metrics.record_error(error);
                            }
                            if changed {
                                metrics.ingested_files.fetch_add(1, Ordering::Relaxed);
                            }
                            metrics.record_sync();
                            let mut sync_lock = last_synced.lock().unwrap();
                            sync_lock.insert(logical_name, Instant::now());
                        }
                        Err(err) => {
                            if Self::is_retryable_ingest_error(&err) {
                                // File is actively locked by another program (e.g. Photoshop).
                                // Defer and retry in the next debounce cycle.
                                info!(
                                    name = %logical_name,
                                    "File locked by editor (SharingViolation); deferring retry..."
                                );
                                let mut p_lock = pending.lock().unwrap();
                                p_lock
                                    .insert(path, (Instant::now(), PendingAction::CreateOrModify));
                            } else {
                                metrics.record_error(&err);
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

                    match engine.unbind_file(&logical_name) {
                        Ok(unbound) => {
                            if unbound {
                                info!(name = %logical_name, "Auto-Vault: Unbound deleted file, preserving version history");
                                let mut sync_lock = last_synced.lock().unwrap();
                                sync_lock.remove(&logical_name);
                                metrics.removed_files.fetch_add(1, Ordering::Relaxed);
                                metrics.record_sync();
                            }
                            let _ = engine
                                .metadata_store()
                                .unmark_watcher_file(&watch_root, &logical_name);
                        }
                        Err(e) => {
                            metrics.record_error(&e);
                            error!(name = %logical_name, error = %e, "Failed to unbind file mapping");
                        }
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
                                let _ = engine.metadata_store().rename_watcher_file(
                                    &watch_root,
                                    &f_name,
                                    &t_name,
                                );
                                metrics.record_sync();
                            }
                            Ok(false) => {
                                // Old name didn't exist in store, just ingest the new file
                                if to.exists() && to.is_file() {
                                    match Self::ingest_if_changed(engine, &t_name, &to) {
                                        Ok(changed) => {
                                            let _ = engine
                                                .metadata_store()
                                                .mark_watcher_file(&watch_root, &t_name);
                                            if changed {
                                                metrics
                                                    .ingested_files
                                                    .fetch_add(1, Ordering::Relaxed);
                                            }
                                            metrics.record_sync();
                                        }
                                        Err(error) => metrics.record_error(error),
                                    }
                                }
                            }
                            Err(e) => {
                                metrics.record_error(&e);
                                error!(from = %f_name, to = %t_name, error = %e, "Failed to rename file");
                            }
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
            None,
            &self.metrics,
            None,
        );
        Ok(())
    }

    fn run_reconciliation(
        engine: &Arc<StorageEngine>,
        config: &WatcherConfig,
        ignore_rules: &Arc<RwLock<IgnoreRules>>,
        last_synced: &Arc<Mutex<HashMap<String, Instant>>>,
        running: Option<&AtomicBool>,
        metrics: &Arc<WatcherMetrics>,
        pending: Option<&Arc<Mutex<HashMap<PathBuf, (Instant, PendingAction)>>>>,
    ) {
        if !config.watch_dir.exists() {
            metrics.record_error(format!(
                "Watched folder is unavailable: {}",
                config.watch_dir.display()
            ));
            return;
        }

        *ignore_rules.write().unwrap_or_else(|p| p.into_inner()) =
            IgnoreRules::load(&config.watch_dir);
        let rules = ignore_rules.read().unwrap();
        let watch_root = Self::watch_root_key(&config.watch_dir);
        let mut current_names = HashSet::new();
        let mut scanned_files = 0u64;
        let mut ingested_files = 0u64;
        let mut removed_files = 0u64;
        let mut complete_scan = true;

        metrics.scanned_files.store(0, Ordering::Relaxed);
        metrics.ingested_files.store(0, Ordering::Relaxed);
        metrics.removed_files.store(0, Ordering::Relaxed);

        let entries = WalkDir::new(&config.watch_dir)
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
            });

        for entry_result in entries {
            if running.is_some_and(|flag| !flag.load(Ordering::Relaxed)) {
                complete_scan = false;
                break;
            }
            let entry = match entry_result {
                Ok(entry) => entry,
                Err(error) => {
                    complete_scan = false;
                    metrics.record_error(&error);
                    continue;
                }
            };
            let p = entry.path();
            if !p.is_file() {
                continue;
            }

            scanned_files += 1;
            metrics
                .scanned_files
                .store(scanned_files, Ordering::Relaxed);
            let rel = match p.strip_prefix(&config.watch_dir) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let logical_name = rel.to_string_lossy().replace('\\', "/");
            current_names.insert(logical_name.clone());

            if pending.is_some_and(|queue| queue.lock().unwrap().contains_key(p)) {
                continue;
            }

            match Self::ingest_if_changed(engine, &logical_name, p) {
                Ok(changed) => {
                    if changed {
                        ingested_files += 1;
                        metrics
                            .ingested_files
                            .store(ingested_files, Ordering::Relaxed);
                    }
                    if let Err(error) = engine
                        .metadata_store()
                        .mark_watcher_file(&watch_root, &logical_name)
                    {
                        metrics.record_error(error);
                    }
                    last_synced
                        .lock()
                        .unwrap()
                        .insert(logical_name, Instant::now());
                    metrics.record_sync();
                }
                Err(error) => {
                    metrics.record_error(&error);
                    warn!(file = %logical_name, error = %error, "Reconciliation ingest failed");
                }
            }

            // Cold start rate limiting: cooperative throttling to avoid freezing user I/O
            if config.throttle_ms > 0 {
                thread::sleep(Duration::from_millis(config.throttle_ms));
            }
        }

        if complete_scan {
            match engine.metadata_store().list_watcher_files(&watch_root) {
                Ok(tracked_names) => {
                    for name in tracked_names {
                        if current_names.contains(&name) {
                            continue;
                        }
                        match engine.unbind_file(&name) {
                            Ok(_) => {
                                let _ = engine
                                    .metadata_store()
                                    .unmark_watcher_file(&watch_root, &name);
                                last_synced.lock().unwrap().remove(&name);
                                removed_files += 1;
                                metrics
                                    .removed_files
                                    .store(removed_files, Ordering::Relaxed);
                                metrics.record_sync();
                            }
                            Err(error) => metrics.record_error(error),
                        }
                    }
                }
                Err(error) => metrics.record_error(error),
            }
            if let Err(error) = engine.metadata_store().flush() {
                metrics.record_error(error);
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

    fn is_retryable_ingest_error(err: &OosLiteError) -> bool {
        Self::is_sharing_violation(err)
            || matches!(err, OosLiteError::SourceChanged(_))
            || matches!(err, OosLiteError::Io(io_err) if matches!(io_err.kind(), io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock))
    }
}
