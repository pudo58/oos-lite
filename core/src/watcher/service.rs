use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use walkdir::WalkDir;

use crate::engine::open_safe_read;
use crate::error::{OosLiteError, Result};
use crate::watcher::{IgnoreRules, WatcherConfig};
use crate::StorageEngine;

const RENAME_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatcherPhase {
    Scanning,
    Watching,
    Stopping,
    Stopped,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatcherStatus {
    pub phase: WatcherPhase,
    pub scanned_files: u64,
    pub saved_files: u64,
    pub removed_files: u64,
    pub pending_files: usize,
    pub last_sync_at: Option<u64>,
    pub last_error: Option<String>,
}

struct Progress {
    status: Mutex<WatcherStatus>,
    errors: Mutex<BTreeMap<String, String>>,
}

impl Progress {
    fn new() -> Self {
        Self {
            status: Mutex::new(WatcherStatus {
                phase: WatcherPhase::Stopped,
                scanned_files: 0,
                saved_files: 0,
                removed_files: 0,
                pending_files: 0,
                last_sync_at: None,
                last_error: None,
            }),
            errors: Mutex::new(BTreeMap::new()),
        }
    }

    fn phase(&self, phase: WatcherPhase) {
        self.status.lock().unwrap().phase = phase;
    }

    fn error(&self, key: String, message: String) {
        self.errors.lock().unwrap().insert(key, message);
        self.refresh();
    }

    fn clear_error(&self, key: &str) {
        self.errors.lock().unwrap().remove(key);
        self.refresh();
    }

    fn refresh(&self) {
        let error = self.errors.lock().unwrap().values().next().cloned();
        let mut status = self.status.lock().unwrap();
        status.last_error = error;
        if status.last_error.is_some()
            && !matches!(status.phase, WatcherPhase::Stopping | WatcherPhase::Stopped)
        {
            status.phase = WatcherPhase::Degraded;
        }
    }

    fn has_errors(&self) -> bool {
        !self.errors.lock().unwrap().is_empty()
    }
}

#[derive(Clone)]
struct PendingUpdate {
    due: Instant,
    retry_delay: Duration,
}

#[derive(Clone)]
struct Rename {
    from: PathBuf,
    to: Option<PathBuf>,
}

#[derive(Default)]
struct Pending {
    updates: HashMap<PathBuf, PendingUpdate>,
    renames: VecDeque<Rename>,
    tracked_from: HashMap<usize, (PathBuf, Instant)>,
    untracked_from: VecDeque<(PathBuf, Instant)>,
    generation: u64,
}

impl Pending {
    fn has_renames(&self) -> bool {
        !self.renames.is_empty() || !self.tracked_from.is_empty() || !self.untracked_from.is_empty()
    }

    fn len(&self) -> usize {
        self.updates.len()
            + self.renames.len()
            + self.tracked_from.len()
            + self.untracked_from.len()
    }

    fn enqueue(&mut self, path: PathBuf, delay: Duration) {
        self.updates.insert(
            path,
            PendingUpdate {
                due: Instant::now() + delay,
                retry_delay: Duration::ZERO,
            },
        );
    }

    fn expire_renames(&mut self, now: Instant) {
        let mut expired = Vec::new();
        self.tracked_from.retain(|_, (path, time)| {
            if now.duration_since(*time) >= RENAME_TIMEOUT {
                expired.push(path.clone());
                false
            } else {
                true
            }
        });
        while self
            .untracked_from
            .front()
            .is_some_and(|(_, time)| now.duration_since(*time) >= RENAME_TIMEOUT)
        {
            expired.push(self.untracked_from.pop_front().unwrap().0);
        }
        for from in expired {
            self.renames.push_back(Rename { from, to: None });
        }
    }
}

pub struct WatcherHandle {
    running: Arc<AtomicBool>,
    pending: Arc<Mutex<Pending>>,
    progress: Arc<Progress>,
    threads: Vec<JoinHandle<()>>,
}

impl WatcherHandle {
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.progress.phase(WatcherPhase::Stopping);
        self.running.store(false, Ordering::SeqCst);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
        self.progress.phase(WatcherPhase::Stopped);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn status(&self) -> WatcherStatus {
        let mut status = self.progress.status.lock().unwrap().clone();
        status.pending_files = self.pending.lock().unwrap().len();
        status
    }
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        if !self.threads.is_empty() {
            self.shutdown();
        }
    }
}

pub struct WatcherService {
    engine: Arc<StorageEngine>,
    config: WatcherConfig,
    running: Arc<AtomicBool>,
    pending: Arc<Mutex<Pending>>,
    progress: Arc<Progress>,
}

impl WatcherService {
    pub fn new(engine: Arc<StorageEngine>, config: WatcherConfig) -> Self {
        Self {
            engine,
            config,
            running: Arc::new(AtomicBool::new(false)),
            pending: Arc::new(Mutex::new(Pending::default())),
            progress: Arc::new(Progress::new()),
        }
    }

    pub fn config(&self) -> &WatcherConfig {
        &self.config
    }

    fn worker(&self, cancellable: bool) -> Result<Worker> {
        let mut config = self.config.clone();
        config.watch_dir = normalized_root(&self.engine, &config.watch_dir)?;
        let watch_key = config
            .watch_dir
            .canonicalize()?
            .to_string_lossy()
            .into_owned();
        Ok(Worker {
            engine: Arc::clone(&self.engine),
            rules: IgnoreRules::load(&config.watch_dir),
            config,
            watch_key,
            running: Arc::clone(&self.running),
            pending: Arc::clone(&self.pending),
            progress: Arc::clone(&self.progress),
            last_synced: HashMap::new(),
            cancellable,
            dirty: true,
            #[cfg(test)]
            faults: ScanFaults::default(),
        })
    }

    pub fn start(&self) -> Result<WatcherHandle> {
        let mut worker = self.worker(true)?;
        let (tx, rx) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(tx, Config::default()).map_err(|e| {
            OosLiteError::Internal(format!("Failed to initialize notify watcher: {e}"))
        })?;
        watcher
            .watch(&worker.config.watch_dir, RecursiveMode::Recursive)
            .map_err(|e| OosLiteError::Internal(format!("Failed to watch directory: {e}")))?;
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(OosLiteError::Internal(
                "Watcher service is already running".into(),
            ));
        }
        *self.pending.lock().unwrap() = Pending::default();
        self.progress.errors.lock().unwrap().clear();
        self.progress.refresh();
        self.progress.phase(WatcherPhase::Scanning);

        let running = Arc::clone(&self.running);
        let pending = Arc::clone(&self.pending);
        let progress = Arc::clone(&self.progress);
        let root = worker.config.watch_dir.clone();
        let debounce = worker.config.debounce_duration;
        let reconcile = Arc::new(AtomicBool::new(false));
        let event_reconcile = Arc::clone(&reconcile);
        let event_thread = match thread::Builder::new()
            .name("oos-watcher-events".into())
            .spawn(move || {
                let _watcher = watcher;
                while running.load(Ordering::Relaxed) {
                    match rx.recv_timeout(Duration::from_millis(50)) {
                        Ok(Ok(event)) => receive_event(event, &root, debounce, &pending),
                        Ok(Err(error)) => {
                            progress.error("events".into(), error.to_string());
                            event_reconcile.store(true, Ordering::SeqCst);
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            progress.error(
                                "events".into(),
                                "Filesystem event stream disconnected".into(),
                            );
                            running.store(false, Ordering::SeqCst);
                            break;
                        }
                    }
                }
            }) {
            Ok(thread) => thread,
            Err(error) => {
                self.running.store(false, Ordering::SeqCst);
                self.progress.phase(WatcherPhase::Stopped);
                return Err(error.into());
            }
        };

        let worker_thread = match thread::Builder::new()
            .name("oos-watcher-worker".into())
            .spawn(move || {
                // Files changed while the initial scan starts are handled through debounce.
                worker.pause(Duration::from_millis(100));
                let _ = worker.process_renames();
                let _ = worker.scan();
                let mut last_scan = Instant::now();
                while !worker.cancelled() {
                    if let Err(error) = worker.process_renames() {
                        worker.progress.error("rename".into(), error.to_string());
                    }
                    if reconcile.swap(false, Ordering::SeqCst)
                        || last_scan.elapsed() >= worker.config.reconcile_interval
                    {
                        if worker.scan().is_ok() {
                            worker.progress.clear_error("events");
                        }
                        last_scan = Instant::now();
                    }
                    worker.process_updates();
                    worker.finish_work();
                    worker.pause(Duration::from_millis(50));
                }
            }) {
            Ok(thread) => thread,
            Err(error) => {
                self.running.store(false, Ordering::SeqCst);
                let _ = event_thread.join();
                self.progress.phase(WatcherPhase::Stopped);
                return Err(error.into());
            }
        };

        Ok(WatcherHandle {
            running: Arc::clone(&self.running),
            pending: Arc::clone(&self.pending),
            progress: Arc::clone(&self.progress),
            threads: vec![event_thread, worker_thread],
        })
    }

    pub fn reconciliation_scan(&self) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            return Err(OosLiteError::Internal(
                "Cannot run a manual scan while the watcher is running".into(),
            ));
        }
        let mut worker = self.worker(false)?;
        // A manual rescan also retries work left by a previous failed scan or stop.
        for update in self.pending.lock().unwrap().updates.values_mut() {
            update.due = Instant::now();
        }
        worker.process_renames()?;
        worker.process_updates();
        let result = worker.scan();
        if result.is_ok() {
            worker.finish_work();
            if !worker.progress.has_errors() {
                worker.progress.phase(WatcherPhase::Stopped);
            }
        }
        result
    }

    pub fn is_sharing_violation(err: &OosLiteError) -> bool {
        #[cfg(windows)]
        if let OosLiteError::Io(err) = err {
            return matches!(err.raw_os_error(), Some(32 | 33));
        }
        let _ = err;
        false
    }
}

// Normalize deleted event paths without querying their metadata.
fn lexical_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            #[cfg(windows)]
            Component::Prefix(prefix) => {
                use std::path::Prefix;
                match prefix.kind() {
                    Prefix::VerbatimDisk(drive) => out.push(format!("{}:", drive as char)),
                    Prefix::VerbatimUNC(server, share) => out.push(format!(
                        "\\\\{}\\{}",
                        server.to_string_lossy(),
                        share.to_string_lossy()
                    )),
                    _ => out.push(prefix.as_os_str()),
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn normalized_root(engine: &StorageEngine, path: &Path) -> Result<PathBuf> {
    let root = lexical_path(&path.canonicalize()?);
    if !root.is_dir() {
        return Err(OosLiteError::Internal(format!(
            "Watched path is not a directory: {}",
            root.display()
        )));
    }
    let store = lexical_path(&engine.root_dir().canonicalize()?);
    if root.starts_with(&store) || store.starts_with(&root) {
        return Err(OosLiteError::Internal(format!(
            "Watched directory '{}' must not overlap OOS-Lite store '{}'",
            root.display(),
            store.display()
        )));
    }
    Ok(root)
}

fn receive_event(mut event: Event, root: &Path, debounce: Duration, shared: &Mutex<Pending>) {
    if !matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) {
        return;
    }
    for path in &mut event.paths {
        *path = lexical_path(&if path.is_absolute() {
            path.clone()
        } else {
            root.join(&path)
        });
    }
    let mut pending = shared.lock().unwrap();
    pending.generation = pending.generation.wrapping_add(1);
    let now = Instant::now();
    pending.expire_renames(now);
    match event.kind {
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if event.paths.len() == 2 => {
            pending.renames.push_back(Rename {
                from: event.paths[0].clone(),
                to: Some(event.paths[1].clone()),
            });
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            if let Some(path) = event.paths.first() {
                if let Some(tracker) = event.attrs.tracker() {
                    if let Some((old, _)) =
                        pending.tracked_from.insert(tracker, (path.clone(), now))
                    {
                        pending.renames.push_back(Rename {
                            from: old,
                            to: None,
                        });
                    }
                } else {
                    pending.untracked_from.push_back((path.clone(), now));
                }
            }
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            if let Some(to) = event.paths.first() {
                let from = match event.attrs.tracker() {
                    Some(tracker) => pending.tracked_from.remove(&tracker),
                    None => pending.untracked_from.pop_front(),
                };
                if let Some((from, _)) = from {
                    pending.renames.push_back(Rename {
                        from,
                        to: Some(to.clone()),
                    });
                } else {
                    pending.enqueue(to.clone(), debounce);
                }
            }
        }
        EventKind::Remove(_) => {
            for from in event.paths {
                pending.renames.push_back(Rename { from, to: None });
            }
        }
        _ => {
            for path in event.paths {
                pending.enqueue(path, debounce);
            }
        }
    }
}

#[cfg(test)]
#[derive(Default)]
struct ScanFaults {
    walk: Option<PathBuf>,
    metadata: Option<PathBuf>,
}

struct Worker {
    engine: Arc<StorageEngine>,
    config: WatcherConfig,
    rules: IgnoreRules,
    watch_key: String,
    running: Arc<AtomicBool>,
    pending: Arc<Mutex<Pending>>,
    progress: Arc<Progress>,
    last_synced: HashMap<PathBuf, Instant>,
    cancellable: bool,
    dirty: bool,
    #[cfg(test)]
    faults: ScanFaults,
}

impl Worker {
    fn cancelled(&self) -> bool {
        self.cancellable && !self.running.load(Ordering::Relaxed)
    }

    fn pause(&self, duration: Duration) {
        let start = Instant::now();
        while start.elapsed() < duration && !self.cancelled() {
            thread::sleep(
                (duration - start.elapsed().min(duration)).min(Duration::from_millis(25)),
            );
        }
    }

    fn logical_name(&self, path: &Path) -> Option<String> {
        path.strip_prefix(&self.config.watch_dir)
            .ok()
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
    }

    fn included(&self, path: &Path) -> bool {
        self.logical_name(path)
            .is_some_and(|name| !self.rules.is_ignored(Path::new(&name)))
    }

    fn retry_delay(&self, previous: Duration) -> Duration {
        if previous.is_zero() {
            self.config
                .debounce_duration
                .clamp(Duration::from_millis(50), Duration::from_secs(30))
        } else {
            previous.saturating_mul(2).min(Duration::from_secs(30))
        }
    }

    fn defer_error(&self, path: &Path, previous: Duration, error: &OosLiteError) {
        self.progress
            .error(path.to_string_lossy().into_owned(), error.to_string());
        if WatcherService::is_sharing_violation(error)
            || matches!(error, OosLiteError::Io(e) if e.kind() == io::ErrorKind::WouldBlock)
        {
            let delay = self.retry_delay(previous);
            self.pending
                .lock()
                .unwrap()
                .updates
                .entry(path.to_path_buf())
                .or_insert(PendingUpdate {
                    due: Instant::now() + delay,
                    retry_delay: delay,
                });
        }
    }

    fn file_needs_put(
        &self,
        name: &str,
        path: &Path,
        metadata: &std::fs::Metadata,
    ) -> Result<bool> {
        let Some(id) = self.engine.metadata_store().resolve_name(name)? else {
            return Ok(true);
        };
        let Some(record) = self.engine.metadata_store().get_object(&id)? else {
            return Ok(true);
        };
        let Some(manifest) = self
            .engine
            .metadata_store()
            .get_manifest(record.latest_manifest_id())?
        else {
            return Ok(true);
        };
        if metadata.len() != manifest.total_size {
            return Ok(true);
        }
        let mut file = open_safe_read(path)?;
        let mut hasher = blake3::Hasher::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            if self.cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Watcher stopped during file read",
                )
                .into());
            }
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        Ok(hasher.finalize().as_bytes() != &manifest.content_hash)
    }

    fn ingest(&mut self, path: &Path, metadata: &std::fs::Metadata, cooldown: bool) -> Result<()> {
        let Some(name) = self.logical_name(path) else {
            return Ok(());
        };
        if !self.file_needs_put(&name, path, metadata)? {
            self.engine
                .metadata_store()
                .mark_watcher_file(&self.watch_key, &name)?;
            self.progress.clear_error(&path.to_string_lossy());
            return Ok(());
        }
        if cooldown {
            if let Some(last) = self.last_synced.get(path) {
                if last.elapsed() < self.config.cooldown_window {
                    self.pending
                        .lock()
                        .unwrap()
                        .updates
                        .entry(path.to_path_buf())
                        .or_insert(PendingUpdate {
                            due: *last + self.config.cooldown_window,
                            retry_delay: Duration::ZERO,
                        });
                    return Ok(());
                }
            }
        }
        self.engine.put_file_named(&name, path)?;
        self.engine
            .metadata_store()
            .mark_watcher_file(&self.watch_key, &name)?;
        self.engine.metadata_store().flush()?;
        if cooldown {
            self.last_synced.insert(path.to_path_buf(), Instant::now());
        }
        self.progress.status.lock().unwrap().saved_files += 1;
        self.progress.clear_error(&path.to_string_lossy());
        Ok(())
    }

    fn enqueue_tree(&self, root: &Path) -> Result<()> {
        for entry in WalkDir::new(root)
            .into_iter()
            .filter_entry(|e| self.included(e.path()))
        {
            let entry = entry.map_err(|e| io::Error::other(e.to_string()))?;
            if entry.file_type().is_file() {
                self.pending
                    .lock()
                    .unwrap()
                    .updates
                    .entry(entry.path().to_path_buf())
                    .or_insert(PendingUpdate {
                        due: Instant::now() + self.config.debounce_duration,
                        retry_delay: Duration::ZERO,
                    });
            }
        }
        Ok(())
    }

    fn process_renames(&mut self) -> Result<()> {
        loop {
            if self.cancelled() {
                return Ok(());
            }
            let action = {
                let mut pending = self.pending.lock().unwrap();
                pending.expire_renames(Instant::now());
                pending.renames.front().cloned()
            };
            let Some(action) = action else {
                break;
            };
            self.dirty = true;
            self.apply_rename(&action)?;
            self.pending.lock().unwrap().renames.pop_front();
            self.progress.clear_error("rename");
        }
        Ok(())
    }

    fn apply_rename(&mut self, action: &Rename) -> Result<()> {
        let from_name = self
            .logical_name(&action.from)
            .filter(|_| self.included(&action.from));
        let to_name = action
            .to
            .as_ref()
            .and_then(|p| self.logical_name(p))
            .filter(|name| !self.rules.is_ignored(Path::new(name)));
        if action.to.is_none() && from_name.is_some() {
            match std::fs::metadata(&action.from) {
                Ok(_) => {
                    self.pending
                        .lock()
                        .unwrap()
                        .enqueue(action.from.clone(), self.config.debounce_duration);
                    return Ok(());
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        let changes = if let Some(ref from) = from_name {
            self.engine.update_watched_prefix(
                &self.watch_key,
                from,
                to_name.as_deref(),
                |name| !self.rules.is_ignored(Path::new(name)),
            )?
        } else {
            Vec::new()
        };

        // Rename metadata first, then remap deferred writes to the same destination.
        let mut pending = self.pending.lock().unwrap();
        let paths: Vec<_> = pending
            .updates
            .keys()
            .filter(|p| p.starts_with(&action.from))
            .cloned()
            .collect();
        for old in paths {
            let update = pending.updates.remove(&old).unwrap();
            self.progress.clear_error(&old.to_string_lossy());
            if let Some(ref to) = action.to {
                let suffix = old.strip_prefix(&action.from).unwrap();
                let new = if suffix.as_os_str().is_empty() {
                    to.clone()
                } else {
                    to.join(suffix)
                };
                if self.included(&new) {
                    pending
                        .updates
                        .entry(new)
                        .and_modify(|existing| {
                            existing.due = existing.due.max(update.due);
                            existing.retry_delay = existing.retry_delay.max(update.retry_delay);
                        })
                        .or_insert(update);
                }
            }
        }
        for (old, new) in &changes {
            let old_path = self.config.watch_dir.join(old);
            self.progress.clear_error(&old_path.to_string_lossy());
            if let Some(last) = self.last_synced.remove(&old_path) {
                if let Some(new) = new {
                    self.last_synced
                        .insert(self.config.watch_dir.join(new), last);
                }
            }
            if let Some(new) = new {
                pending
                    .updates
                    .entry(self.config.watch_dir.join(new))
                    .or_insert(PendingUpdate {
                        due: Instant::now() + self.config.debounce_duration,
                        retry_delay: Duration::ZERO,
                    });
            }
        }
        drop(pending);
        self.progress.status.lock().unwrap().removed_files +=
            changes.iter().filter(|(_, new)| new.is_none()).count() as u64;

        if let Some(ref to) = action.to {
            if self.included(to) {
                match std::fs::metadata(to) {
                    Ok(metadata) if metadata.is_dir() => self.enqueue_tree(to)?,
                    Ok(metadata) if metadata.is_file() => {
                        self.pending
                            .lock()
                            .unwrap()
                            .updates
                            .entry(to.clone())
                            .or_insert(PendingUpdate {
                                due: Instant::now() + self.config.debounce_duration,
                                retry_delay: Duration::ZERO,
                            });
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {} // Next rename may already have moved it.
                    Err(e) => return Err(e.into()),
                }
            }
        }
        Ok(())
    }

    fn process_updates(&mut self) {
        loop {
            if self.cancelled() {
                return;
            }
            let next = {
                let mut pending = self.pending.lock().unwrap();
                if pending.has_renames() {
                    return;
                }
                let path = pending
                    .updates
                    .iter()
                    .find(|(_, update)| update.due <= Instant::now())
                    .map(|(path, _)| path.clone());
                path.map(|path| {
                    let update = pending.updates.remove(&path).unwrap();
                    (path, update)
                })
            };
            let Some((path, update)) = next else {
                return;
            };
            self.dirty = true;
            if !self.included(&path) {
                self.progress.clear_error(&path.to_string_lossy());
                continue;
            }
            let result = match std::fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => self.ingest(&path, &metadata, true),
                Ok(metadata) if metadata.is_dir() => self.enqueue_tree(&path),
                Ok(_) => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    self.progress.clear_error(&path.to_string_lossy());
                    Ok(())
                }
                Err(e) => Err(e.into()),
            };
            if let Err(error) = result {
                self.defer_error(&path, update.retry_delay, &error);
            }
        }
    }

    fn scan(&mut self) -> Result<()> {
        self.dirty = true;
        self.rules = IgnoreRules::load(&self.config.watch_dir);
        self.progress.clear_error("scan");
        self.progress.phase(WatcherPhase::Scanning);
        {
            let mut status = self.progress.status.lock().unwrap();
            status.scanned_files = 0;
            status.saved_files = 0;
            status.removed_files = 0;
        }
        let result = match self.scan_inner() {
            Ok(Some(file_error)) => Err(file_error),
            Ok(None) => Ok(()),
            Err(error) => {
                self.progress.error("scan".into(), error.to_string());
                Err(error)
            }
        };
        self.progress.refresh();
        result
    }

    fn scan_inner(&mut self) -> Result<Option<OosLiteError>> {
        if !std::fs::metadata(&self.config.watch_dir)?.is_dir() {
            return Err(io::Error::other("Watched root is no longer a directory").into());
        }
        let generation = self.pending.lock().unwrap().generation;
        let mut seen = HashSet::new();
        let mut scan_error = None;
        let mut ingest_error = None;
        let root = self.config.watch_dir.clone();
        let rules = IgnoreRules::load(&root);
        for entry in WalkDir::new(&root).into_iter().filter_entry(|e| {
            e.path().strip_prefix(&root).is_ok_and(|relative| {
                relative.as_os_str().is_empty() || !rules.is_ignored(relative)
            })
        }) {
            if self.cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Reconciliation scan cancelled",
                )
                .into());
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    scan_error.get_or_insert_with(|| {
                        OosLiteError::Io(io::Error::other(error.to_string()))
                    });
                    continue;
                }
            };
            let path = entry.path();
            #[cfg(test)]
            if self.faults.walk.as_deref() == Some(path) {
                scan_error.get_or_insert_with(|| {
                    OosLiteError::Io(io::Error::other("Injected directory traversal failure"))
                });
                continue;
            }
            // Metadata errors must not turn existing tracked files into deletion candidates.
            let metadata = self.scan_metadata(path);
            let metadata = match metadata {
                Ok(metadata) => metadata,
                Err(error) => {
                    scan_error.get_or_insert_with(|| OosLiteError::Io(error));
                    continue;
                }
            };
            if !metadata.is_file() {
                continue;
            }
            let name = self.logical_name(path).unwrap();
            seen.insert(name.clone());
            self.progress.status.lock().unwrap().scanned_files += 1;
            let blocked = {
                let pending = self.pending.lock().unwrap();
                pending.has_renames() || (self.cancellable && pending.updates.contains_key(path))
            };
            if blocked {
                // Queue existing files as well: a rename elsewhere can defer this scan entry.
                self.pending
                    .lock()
                    .unwrap()
                    .updates
                    .entry(path.to_path_buf())
                    .or_insert(PendingUpdate {
                        due: Instant::now() + self.config.debounce_duration,
                        retry_delay: Duration::ZERO,
                    });
                continue;
            }
            if !self.cancellable {
                self.pending.lock().unwrap().updates.remove(path);
            }
            if let Err(error) = self.ingest(path, &metadata, false) {
                self.defer_error(path, Duration::ZERO, &error);
                ingest_error.get_or_insert(error);
            }
            self.pause(Duration::from_millis(self.config.throttle_ms));
        }
        if self.cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Reconciliation scan cancelled",
            )
            .into());
        }
        if let Some(error) = scan_error {
            return Err(error);
        }
        if ingest_error.is_some() {
            return Ok(ingest_error);
        }

        let mut pending = self.pending.lock().unwrap();
        if pending.has_renames() || pending.generation != generation {
            return Ok(None);
        }
        let managed = self
            .engine
            .metadata_store()
            .list_watcher_files(&self.watch_key)?;
        let mut missing = Vec::new();
        for name in managed {
            if seen.contains(&name) {
                continue;
            }
            let path = root.join(&name);
            match std::fs::symlink_metadata(&path) {
                Ok(_) => {} // Existing ignored/non-regular files are not deletions.
                Err(error) if error.kind() == io::ErrorKind::NotFound => missing.push(path),
                Err(error) => return Err(error.into()),
            }
        }
        for path in missing {
            pending.renames.push_back(Rename {
                from: path,
                to: None,
            });
        }
        drop(pending);
        self.process_renames()?;
        self.engine.metadata_store().flush()?;
        Ok(None)
    }

    fn scan_metadata(&self, path: &Path) -> io::Result<std::fs::Metadata> {
        #[cfg(test)]
        if self.faults.metadata.as_deref() == Some(path) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Injected metadata failure",
            ));
        }
        std::fs::metadata(path)
    }

    fn finish_work(&mut self) {
        if self.cancelled() {
            return;
        }
        let idle = self.pending.lock().unwrap().len() == 0;
        if self.dirty && idle {
            match self.engine.metadata_store().flush() {
                Ok(()) => self.progress.clear_error("flush"),
                Err(error) => self.progress.error("flush".into(), error.to_string()),
            }
        }
        if self.progress.has_errors() {
            self.progress.phase(WatcherPhase::Degraded);
            return;
        }
        self.progress.phase(WatcherPhase::Watching);
        if self.dirty && idle {
            self.progress.status.lock().unwrap().last_sync_at = Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            );
            self.dirty = false;
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
