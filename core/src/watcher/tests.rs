use super::*;
use crate::ObjectId;
use std::fs;
use tempfile::{tempdir, TempDir};

struct Fixture {
    worker: Worker,
    dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempdir().unwrap();
        let root = dir.path().join("watch");
        fs::create_dir(&root).unwrap();
        let engine = Arc::new(StorageEngine::open(dir.path().join("store")).unwrap());
        let service = WatcherService::new(
            engine,
            WatcherConfig::new(&root)
                .with_debounce(Duration::ZERO)
                .with_cooldown(Duration::ZERO)
                .with_throttle_ms(0),
        );
        Self {
            worker: service.worker(false).unwrap(),
            dir,
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.worker.config.watch_dir.join(name)
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        let path = self.path(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn id(&self, name: &str) -> Option<ObjectId> {
        self.worker
            .engine
            .metadata_store()
            .resolve_name(name)
            .unwrap()
    }

    fn rename(&self, from: &Path, to: &Path) {
        fs::rename(from, to).unwrap();
        receive_event(
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                .add_path(from.to_path_buf())
                .add_path(to.to_path_buf()),
            &self.worker.config.watch_dir,
            Duration::ZERO,
            &self.worker.pending,
        );
    }

    fn modify(&self, name: &str) {
        receive_event(
            Event::new(EventKind::Modify(ModifyKind::Any)).add_path(self.path(name)),
            &self.worker.config.watch_dir,
            Duration::ZERO,
            &self.worker.pending,
        );
    }

    fn drain(&mut self) {
        self.worker.process_renames().unwrap();
        self.worker.process_updates();
        self.worker.finish_work();
        assert!(!self.worker.progress.has_errors());
    }

    fn content(&self, target: &str, version: u32) -> Vec<u8> {
        let out = self.dir.path().join("restore");
        self.worker
            .engine
            .get_file_version(target, Some(version), &out)
            .unwrap();
        fs::read(out).unwrap()
    }
}

#[test]
fn rename_then_modify_keeps_the_original_object_and_versions() {
    let mut f = Fixture::new();
    f.write("a.txt", b"first");
    f.worker.scan().unwrap();
    let id = f.id("a.txt").unwrap();
    f.rename(&f.path("a.txt"), &f.path("b.txt"));
    f.write("b.txt", b"second");
    f.modify("b.txt");
    f.worker.process_updates();
    assert_eq!(f.id("b.txt"), None, "ingest must wait for rename");
    f.drain();
    assert_eq!(f.id("a.txt"), None);
    assert_eq!(f.id("b.txt"), Some(id));
    assert_eq!(f.worker.engine.get_versions("b.txt").unwrap().len(), 2);
    assert_eq!(f.content("b.txt", 1), b"first");
    assert_eq!(f.content("b.txt", 2), b"second");
}

#[test]
fn chained_file_renames_move_pending_updates_to_the_final_name() {
    let mut f = Fixture::new();
    f.write("a.txt", b"first");
    f.worker.scan().unwrap();
    let id = f.id("a.txt");
    f.modify("a.txt");
    f.rename(&f.path("a.txt"), &f.path("b.txt"));
    f.modify("b.txt");
    f.rename(&f.path("b.txt"), &f.path("c.txt"));
    f.write("c.txt", b"final");
    f.modify("c.txt");
    // Reconciliation must not turn the destination into a new object.
    f.worker.scan().unwrap();
    assert_eq!(f.id("a.txt"), id);
    assert_eq!(f.id("c.txt"), None);
    f.drain();
    assert_eq!(f.id("a.txt"), None);
    assert_eq!(f.id("b.txt"), None);
    assert_eq!(f.id("c.txt"), id);
    assert_eq!(f.worker.engine.get_versions("c.txt").unwrap().len(), 2);
    assert_eq!(f.worker.pending.lock().unwrap().len(), 0);
    assert_eq!(f.content("c.txt", 2), b"final");
}

#[test]
fn nested_directory_renames_preserve_boundaries_tracking_and_cooldown() {
    let mut f = Fixture::new();
    f.write("docs/a.txt", b"a");
    f.write("docs/nested/b.txt", b"b");
    f.write("docs-old/a.txt", b"unrelated");
    f.worker.scan().unwrap();
    let a = f.id("docs/a.txt");
    let b = f.id("docs/nested/b.txt");
    let other = f.id("docs-old/a.txt");
    let time = Instant::now();
    f.worker.last_synced.insert(f.path("docs/a.txt"), time);
    f.modify("docs/nested/b.txt");
    f.rename(&f.path("docs"), &f.path("intermediate"));
    f.rename(&f.path("intermediate"), &f.path("renamed"));
    f.drain();
    assert_eq!(f.id("renamed/a.txt"), a);
    assert_eq!(f.id("renamed/nested/b.txt"), b);
    assert_eq!(f.id("docs-old/a.txt"), other);
    assert_eq!(f.id("docs/a.txt"), None);
    assert_eq!(
        f.worker.last_synced.get(&f.path("renamed/a.txt")),
        Some(&time)
    );
    assert!(!f.worker.last_synced.contains_key(&f.path("docs/a.txt")));
    let tracked = f
        .worker
        .engine
        .metadata_store()
        .list_watcher_files(&f.worker.watch_key)
        .unwrap();
    assert_eq!(
        tracked,
        vec!["docs-old/a.txt", "renamed/a.txt", "renamed/nested/b.txt"]
    );
    assert_eq!(
        f.worker.engine.get_versions("renamed/a.txt").unwrap().len(),
        1
    );
    assert_eq!(
        f.worker
            .engine
            .get_versions("renamed/nested/b.txt")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn directory_moves_across_ignored_and_outside_roots_unbind_and_import_children() {
    for ignored in [false, true] {
        let mut f = Fixture::new();
        f.write("docs/nested/a.txt", b"retained history");
        f.worker.scan().unwrap();
        let id = f.id("docs/nested/a.txt").unwrap();
        let outside = if ignored {
            f.path("node_modules")
        } else {
            f.dir.path().join("outside")
        };
        f.rename(&f.path("docs"), &outside);
        f.drain();
        assert_eq!(f.id("docs/nested/a.txt"), None);
        assert!(f
            .worker
            .engine
            .metadata_store()
            .list_watcher_files(&f.worker.watch_key)
            .unwrap()
            .is_empty());
        f.worker.engine.gc().unwrap();
        assert_eq!(f.content(&id.to_string(), 1), b"retained history");
        f.rename(&outside, &f.path("returned"));
        f.drain();
        assert!(f.id("returned/nested/a.txt").is_some());
        assert_eq!(f.content("returned/nested/a.txt", 1), b"retained history");
    }
}

#[test]
fn deleting_directory_unbinds_only_managed_descendants() {
    let mut f = Fixture::new();
    f.write("docs/a.txt", b"watched");
    f.write("docs/nested/b.txt", b"watched too");
    f.write("docs-old/a.txt", b"other");
    f.worker.scan().unwrap();
    let source = f.dir.path().join("manual");
    fs::write(&source, b"manual history").unwrap();
    f.worker
        .engine
        .put_file_named("docs/manual.txt", &source)
        .unwrap();
    fs::remove_dir_all(f.path("docs")).unwrap();
    receive_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::Folder)).add_path(f.path("docs")),
        &f.worker.config.watch_dir,
        Duration::ZERO,
        &f.worker.pending,
    );
    f.drain();
    assert_eq!(f.id("docs/a.txt"), None);
    assert_eq!(f.id("docs/nested/b.txt"), None);
    assert!(f.id("docs/manual.txt").is_some());
    assert!(f.id("docs-old/a.txt").is_some());
}

#[test]
fn scan_faults_and_cancellation_never_delete_unseen_files() {
    for fault in ["walk", "metadata", "cancel"] {
        let mut f = Fixture::new();
        f.write("missing.txt", b"keep until complete scan");
        f.write("present.txt", b"still here");
        f.worker.scan().unwrap();
        let id = f.id("missing.txt");
        fs::remove_file(f.path("missing.txt")).unwrap();
        match fault {
            "walk" => f.worker.faults.walk = Some(f.path("present.txt")),
            "metadata" => f.worker.faults.metadata = Some(f.path("present.txt")),
            "cancel" => f.worker.cancellable = true,
            _ => unreachable!(),
        }
        assert!(f.worker.scan().is_err(), "{fault}");
        assert_eq!(f.id("missing.txt"), id, "{fault}");
        assert_eq!(
            f.worker.progress.status.lock().unwrap().phase,
            WatcherPhase::Degraded
        );
        f.worker.faults = ScanFaults::default();
        f.worker.cancellable = false;
        f.worker.scan().unwrap();
        f.worker.finish_work();
        assert_eq!(f.id("missing.txt"), None);
        assert!(!f.worker.progress.has_errors());
    }
}

#[test]
fn retry_backoff_is_bounded_and_success_clears_error_without_duplicate_version() {
    let mut f = Fixture::new();
    f.write("busy.txt", b"eventually readable");
    let path = f.path("busy.txt");
    let error = OosLiteError::Io(io::Error::new(io::ErrorKind::WouldBlock, "busy"));
    f.worker.progress.phase(WatcherPhase::Scanning);
    f.worker.defer_error(&path, Duration::ZERO, &error);
    let mut previous = Duration::from_millis(50);
    assert_eq!(
        f.worker.pending.lock().unwrap().updates[&path].retry_delay,
        previous
    );
    assert_eq!(
        f.worker.progress.status.lock().unwrap().phase,
        WatcherPhase::Degraded
    );
    for _ in 0..20 {
        let next = f.worker.retry_delay(previous);
        assert_eq!(
            next,
            previous.saturating_mul(2).min(Duration::from_secs(30))
        );
        previous = next;
    }
    assert_eq!(previous, Duration::from_secs(30));
    f.worker
        .pending
        .lock()
        .unwrap()
        .updates
        .get_mut(&path)
        .unwrap()
        .due = Instant::now();
    f.drain();
    assert_eq!(
        f.worker.progress.status.lock().unwrap().phase,
        WatcherPhase::Watching
    );
    assert!(f
        .worker
        .progress
        .status
        .lock()
        .unwrap()
        .last_error
        .is_none());
    f.modify("busy.txt");
    f.drain();
    assert_eq!(f.worker.engine.get_versions("busy.txt").unwrap().len(), 1);
}

#[test]
fn rename_replacement_keeps_both_object_histories_through_gc() {
    let mut f = Fixture::new();
    f.write("a.txt", b"source");
    f.write("b.txt", b"replaced");
    f.worker.scan().unwrap();
    let source = f.id("a.txt").unwrap();
    let replaced = f.id("b.txt").unwrap();
    fs::remove_file(f.path("b.txt")).unwrap();
    f.rename(&f.path("a.txt"), &f.path("b.txt"));
    f.drain();
    assert_eq!(f.id("b.txt"), Some(source));
    f.worker.engine.gc().unwrap();
    assert_eq!(f.content(&replaced.to_string(), 1), b"replaced");
    assert_eq!(f.content("b.txt", 1), b"source");
}

#[test]
fn paired_rename_events_and_deleted_paths_are_normalized_without_stat() {
    let mut f = Fixture::new();
    f.write("a.txt", b"history");
    f.worker.scan().unwrap();
    let id = f.id("a.txt");
    fs::rename(f.path("a.txt"), f.path("b.txt")).unwrap();
    let root = &f.worker.config.watch_dir;
    receive_event(
        Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From)))
            .add_path(root.join("absent/../a.txt"))
            .set_tracker(9),
        root,
        Duration::ZERO,
        &f.worker.pending,
    );
    receive_event(
        Event::new(EventKind::Modify(ModifyKind::Any)).add_path(f.path("b.txt")),
        root,
        Duration::ZERO,
        &f.worker.pending,
    );
    receive_event(
        Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To)))
            .add_path(f.path("b.txt"))
            .set_tracker(9),
        root,
        Duration::ZERO,
        &f.worker.pending,
    );
    f.drain();
    assert_eq!(f.id("b.txt"), id);
    assert_eq!(f.id("a.txt"), None);
    assert_eq!(f.worker.engine.get_versions("b.txt").unwrap().len(), 1);
}
