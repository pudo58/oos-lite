use super::{redb_error, MetadataStore, MIGRATION_VERSION};
use crate::error::{OosLiteError, Result};
use redb::Database;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const BLOCKER: &[u8] = b"OOS-Lite redb: opening this metadata with sled is prohibited\n";

fn failure(message: impl Into<String>) -> OosLiteError {
    OosLiteError::Internal(message.into())
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    // Windows publication uses MoveFileExW with WRITE_THROUGH below.
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn publish(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // Both buffers are NUL-terminated and remain alive for the call.
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(not(windows))]
    fs::rename(source, destination)?;
    sync_directory(destination.parent().unwrap())
}

fn write_marker(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = sibling(path, ".tmp");
    let mut file = File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    publish(&temporary, path)
}

fn check_marker(path: &Path) -> Result<bool> {
    match fs::read(path) {
        Ok(bytes) if bytes == MIGRATION_VERSION => Ok(true),
        Ok(_) => Err(failure(format!(
            "Unsupported or corrupt migration state: {}",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn block_sled(directory: &Path) -> Result<()> {
    let conf = directory.join("conf");
    let backup = directory.join("conf.sled-backup");
    if conf.exists() {
        let bytes = fs::read(&conf)?;
        if bytes == BLOCKER {
            return Ok(());
        }
        // Preserve the original settings before atomically installing the blocker.
        write_marker(&backup, &bytes)?;
    } else if directory.join("db").exists() && !backup.exists() {
        return Err(failure(
            "Legacy sled configuration is missing; refusing migration",
        ));
    }
    write_marker(&conf, BLOCKER)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return Err(failure("Symlinks are not supported in legacy metadata"));
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else {
        fs::copy(source, destination)?;
    }
    Ok(())
}

fn legacy_copy(directory: &Path) -> Result<Option<sled::Db>> {
    if !directory.join("db").exists() {
        return Ok(None);
    }
    // This disposable copy is only read during migration; original sled data stays put.
    let copy = directory.join(".redb-sled-import");
    if copy.exists() {
        fs::remove_dir_all(&copy)?;
    }
    fs::create_dir(&copy)?;
    fs::copy(directory.join("conf.sled-backup"), copy.join("conf"))?;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == "db" || name_str == "blobs" || name_str.starts_with("snap.") {
            copy_tree(&entry.path(), &copy.join(name))?;
        }
    }
    Ok(Some(sled::open(copy)?))
}

pub(super) fn open(input: &Path) -> Result<(Database, File)> {
    let input = std::path::absolute(input)?;
    fs::create_dir_all(input.parent().unwrap())?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(sibling(&input, ".open.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)?;

    // Accept the earlier file layout, but never reconstruct its missing database
    // from the stale .sled archive. There is no reliable migration state in that layout.
    let file_state = sibling(&input, ".redb-active");
    if input.is_file() || file_state.exists() {
        let db = Database::open(&input).map_err(redb_error)?;
        MetadataStore::validate_database(&db)?;
        if !check_marker(&file_state)? {
            write_marker(&file_state, MIGRATION_VERSION)?;
        }
        return Ok((db, lock));
    }
    if sibling(&input, ".sled").exists() {
        return Err(failure("Ambiguous legacy migration archive: active metadata is missing or incomplete; manual recovery is required"));
    }

    fs::create_dir_all(&input)?;
    let active = input.join("metadata.redb");
    let staging = input.join("metadata.redb.staging");
    let committed = input.join(".redb-active");
    let migrating = input.join(".redb-migrating");

    if check_marker(&committed)? {
        if !active.exists() && staging.exists() {
            let db = Database::open(&staging).map_err(redb_error)?;
            MetadataStore::validate_database(&db)?;
            drop(db);
            publish(&staging, &active)?;
        }
        let db = Database::open(&active).map_err(redb_error)?;
        MetadataStore::validate_database(&db)?;
        block_sled(&input)?;
        return Ok((db, lock));
    }
    if active.exists() {
        // Adopt a valid database from the initial nested-redb implementation.
        // Even an empty/truncated file here is corruption, not permission to reimport.
        let db = Database::open(&active).map_err(redb_error)?;
        MetadataStore::validate_database(&db)?;
        block_sled(&input)?;
        write_marker(&committed, MIGRATION_VERSION)?;
        return Ok((db, lock));
    }
    if !check_marker(&migrating)? {
        if staging.exists() || input.join("metadata.redb.corrupt").exists() {
            return Err(failure(
                "Untracked metadata artifacts; refusing automatic migration",
            ));
        }
        write_marker(&migrating, MIGRATION_VERSION)?;
    }
    block_sled(&input)?;
    checkpoint("blocked")?;
    // No database has been published yet, so an interrupted staging file can be rebuilt.
    if staging.exists() {
        fs::remove_file(&staging)?;
    }
    let legacy = legacy_copy(&input)?;
    let db = Database::create(&staging).map_err(redb_error)?;
    checkpoint("created")?;
    MetadataStore::initialize_database(&db, legacy.as_ref())?;
    drop(legacy);
    MetadataStore::validate_database(&db)?;
    drop(db);
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(&staging)?
        .sync_all()?;
    checkpoint("staged")?;
    // Commit intent BEFORE publication. Once present, missing/corrupt metadata
    // must fail closed, never fall back to importing the legacy source.
    write_marker(&committed, MIGRATION_VERSION)?;
    checkpoint("committed")?;
    publish(&staging, &active)?;
    checkpoint("published")?;
    let db = Database::open(&active).map_err(redb_error)?;
    Ok((db, lock))
}

fn checkpoint(_point: &str) -> Result<()> {
    #[cfg(test)]
    if std::env::var("OOS_TEST_MIGRATION_STOP").as_deref() == Ok(_point) {
        std::process::exit(86);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{object::ObjectId, StorageEngine};
    use redb::{ReadableDatabase, ReadableTable, TableDefinition};
    use tempfile::tempdir;

    // Make a genuine sled fixture from serialized engine metadata, including WAL,
    // segments, version histories and snapshots. No old binary is required in CI.
    fn seed(root: &Path, encrypted: bool) -> ObjectId {
        let engine = if encrypted {
            StorageEngine::open_with_password(root, "test-password").unwrap()
        } else {
            StorageEngine::open(root).unwrap()
        };
        let source = root.join("source");
        fs::write(&source, b"version one").unwrap();
        engine.put_file_named("doc", &source).unwrap();
        fs::write(&source, b"version two").unwrap();
        engine.put_file_named("doc", &source).unwrap();
        engine.create_snapshot("before-migration").unwrap();
        engine
            .metadata_store()
            .mark_watcher_file("root", "doc")
            .unwrap();
        engine
            .metadata_store()
            .save_watcher_config("root", 3, 60, 0)
            .unwrap();
        let id = engine
            .metadata_store()
            .resolve_name("doc")
            .unwrap()
            .unwrap();
        drop(engine);
        let directory = root.join("metadata.db");
        let fixture = root.join("sled-fixture");
        let legacy = sled::open(&fixture).unwrap();
        let db = Database::open(directory.join("metadata.redb")).unwrap();
        let tx = db.begin_read().unwrap();
        for name in [
            "name_index",
            "object_index",
            "manifests",
            "snapshots",
            "watcher_files",
            "watcher_config",
        ] {
            let tree = legacy.open_tree(name).unwrap();
            let table = tx
                .open_table(TableDefinition::<&[u8], &[u8]>::new(name))
                .unwrap();
            for item in table.iter().unwrap() {
                let (key, value) = item.unwrap();
                if key.value() != super::super::MIGRATION_MARKER {
                    tree.insert(key.value(), value.value()).unwrap();
                }
            }
        }
        legacy.flush().unwrap();
        drop(legacy);
        drop(tx);
        drop(db);
        fs::remove_dir_all(&directory).unwrap();
        fs::rename(fixture, directory).unwrap();
        id
    }

    fn verify(root: &Path, encrypted: bool, id: ObjectId) {
        let engine = if encrypted {
            StorageEngine::open_with_password(root, "test-password").unwrap()
        } else {
            StorageEngine::open(root).unwrap()
        };
        assert_eq!(engine.get_versions("doc").unwrap().len(), 2);
        assert_eq!(
            engine.metadata_store().resolve_name("doc").unwrap(),
            Some(id)
        );
        assert!(engine
            .metadata_store()
            .get_snapshot("before-migration")
            .unwrap()
            .is_some());
        assert_eq!(
            engine.metadata_store().list_watcher_files("root").unwrap(),
            vec!["doc"]
        );
        assert_eq!(
            engine.metadata_store().load_watcher_config().unwrap(),
            Some(("root".into(), 3, 60, 0))
        );
        assert_eq!(engine.gc().unwrap().chunks_reclaimed, 0);
        let output = root.join("restored");
        engine.get_file("doc", &output).unwrap();
        assert_eq!(fs::read(output).unwrap(), b"version two");
        engine
            .get_file_version("doc", Some(1), root.join("v1"))
            .unwrap();
        assert_eq!(fs::read(root.join("v1")).unwrap(), b"version one");
    }

    #[test]
    fn migration_child() {
        if let Ok(root) = std::env::var("OOS_TEST_MIGRATION_ROOT") {
            let _store = MetadataStore::open(Path::new(&root).join("metadata.db")).unwrap();
            panic!("child did not stop at requested checkpoint");
        }
    }

    #[test]
    fn process_exit_at_each_migration_boundary_resumes_and_blocks_sled() {
        for encrypted in [false, true] {
            for point in ["blocked", "created", "staged", "committed", "published"] {
                let temp = tempdir().unwrap();
                let root = temp.path().join("store");
                let id = seed(&root, encrypted);
                let status = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "index::migration::tests::migration_child"])
                    .env("OOS_TEST_MIGRATION_ROOT", &root)
                    .env("OOS_TEST_MIGRATION_STOP", point)
                    .output()
                    .unwrap();
                assert_eq!(
                    status.status.code(),
                    Some(86),
                    "{point}: {}",
                    String::from_utf8_lossy(&status.stdout)
                );
                assert!(sled::open(root.join("metadata.db")).is_err(), "{point}");
                if !encrypted {
                    if let Some(binary) = std::env::var_os("OOS_TEST_LEGACY_BINARY") {
                        let old = std::process::Command::new(binary)
                            .arg("--store-dir")
                            .arg(&root)
                            .arg("gc")
                            .env_remove("OOS_PASSWORD")
                            .output()
                            .unwrap();
                        assert!(!old.status.success(), "old GC succeeded at {point}");
                        println!(
                            "Old binary GC blocked at {point}: {}",
                            String::from_utf8_lossy(&old.stderr)
                        );
                    }
                }
                verify(&root, encrypted, id);
                verify(&root, encrypted, id);
            }
        }
    }

    #[test]
    fn missing_or_empty_committed_database_never_reimports_sled() {
        for missing in [false, true] {
            let temp = tempdir().unwrap();
            let root = temp.path().join("store");
            seed(&root, false);
            let engine = StorageEngine::open(&root).unwrap();
            let source = root.join("source");
            fs::write(&source, b"redb-only version three").unwrap();
            let put = engine.put_file_named("doc", &source).unwrap();
            let manifest = engine
                .metadata_store()
                .get_manifest(&put.manifest_id)
                .unwrap()
                .unwrap();
            drop(engine);
            let active = root.join("metadata.db/metadata.redb");
            let saved = root.join("saved.redb");
            fs::copy(&active, &saved).unwrap();
            if missing {
                fs::remove_file(&active).unwrap();
            } else {
                fs::write(&active, []).unwrap();
            }
            for _ in 0..2 {
                assert!(StorageEngine::open(&root).is_err());
            }
            assert!(sled::open(root.join("metadata.db")).is_err());
            fs::copy(saved, &active).unwrap();
            let engine = StorageEngine::open(&root).unwrap();
            assert_eq!(engine.get_versions("doc").unwrap().len(), 3);
            assert_eq!(engine.gc().unwrap().chunks_reclaimed, 0);
            for chunk in manifest.chunks {
                assert!(engine.segment_store().has_chunk(&chunk));
            }
            engine.get_file("doc", root.join("restored")).unwrap();
            assert_eq!(
                fs::read(root.join("restored")).unwrap(),
                b"redb-only version three"
            );
        }
    }

    #[test]
    fn incomplete_staging_is_rebuilt_only_before_publication() {
        for committed in [false, true] {
            for bytes in [b"".as_slice(), b"partial redb header".as_slice()] {
                let temp = tempdir().unwrap();
                let root = temp.path().join("store");
                let id = seed(&root, false);
                let directory = root.join("metadata.db");
                write_marker(&directory.join(".redb-migrating"), MIGRATION_VERSION).unwrap();
                block_sled(&directory).unwrap();
                fs::write(directory.join("metadata.redb.staging"), bytes).unwrap();
                if committed {
                    write_marker(&directory.join(".redb-active"), MIGRATION_VERSION).unwrap();
                    assert!(StorageEngine::open(&root).is_err());
                    assert_eq!(
                        fs::read(directory.join("metadata.redb.staging")).unwrap(),
                        bytes
                    );
                    assert!(!directory.join("metadata.redb").exists());
                } else {
                    verify(&root, false, id);
                }
            }
        }
    }

    #[test]
    fn earlier_file_layout_is_adopted_and_missing_file_fails_closed() {
        let temp = tempdir().unwrap();
        let input = temp.path().join("metadata.db");
        let store = MetadataStore::open(&input).unwrap();
        let id = ObjectId::generate();
        store.bind_name("doc", &id).unwrap();
        drop(store);
        let archive = temp.path().join("metadata.db.sled");
        fs::rename(&input, &archive).unwrap();
        fs::rename(archive.join("metadata.redb"), &input).unwrap();
        let store = MetadataStore::open(&input).unwrap();
        assert_eq!(store.resolve_name("doc").unwrap(), Some(id));
        drop(store);
        fs::rename(&input, temp.path().join("saved.redb")).unwrap();
        assert!(MetadataStore::open(&input).is_err());
        assert!(!input.exists());
        assert!(!StorageEngine::is_store_empty(temp.path()).unwrap());
    }

    #[test]
    fn ambiguous_previous_rename_interruption_requires_manual_recovery() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("store");
        seed(&root, false);
        fs::rename(root.join("metadata.db"), root.join("metadata.db.sled")).unwrap();
        assert!(StorageEngine::open(&root).is_err());
        assert!(!root.join("metadata.db").exists());
    }
}
