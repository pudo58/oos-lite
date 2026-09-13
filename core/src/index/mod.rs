//! Name, object, manifest, snapshot, and watcher metadata stored in redb.
//!
//! Existing sled metadata is migrated through staging, with a durable publication
//! marker and a persistent sled configuration blocker preventing downgrade access.

use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::fmt::Display;
use std::path::Path;
use tracing::info;

use crate::error::{OosLiteError, Result};
use crate::manifest::Manifest;
use crate::object::{ObjectId, ObjectRecord};
use crate::snapshot::Snapshot;

type BytesTable<'a> = redb::Table<'a, &'static [u8], &'static [u8]>;

const NAME_INDEX: TableDefinition<&'static [u8], &'static [u8]> =
    TableDefinition::new("name_index");
const OBJECT_INDEX: TableDefinition<&'static [u8], &'static [u8]> =
    TableDefinition::new("object_index");
const MANIFESTS: TableDefinition<&'static [u8], &'static [u8]> = TableDefinition::new("manifests");
const SNAPSHOTS: TableDefinition<&'static [u8], &'static [u8]> = TableDefinition::new("snapshots");
const WATCHER_FILES: TableDefinition<&'static [u8], &'static [u8]> =
    TableDefinition::new("watcher_files");
const WATCHER_CONFIG: TableDefinition<&'static [u8], &'static [u8]> =
    TableDefinition::new("watcher_config");
const MIGRATION_MARKER: &[u8] = b"__oos_lite_metadata_backend";
const MIGRATION_VERSION: &[u8] = b"redb-v1";

fn redb_error(error: impl Display) -> OosLiteError {
    OosLiteError::Redb(error.to_string())
}

mod migration;

pub struct MetadataStore {
    db: Database,
    _migration_lock: std::fs::File,
}

impl MetadataStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let (db, lock) = migration::open(path.as_ref())?;
        info!("MetadataStore opened at: {}", path.as_ref().display());
        Ok(Self {
            db,
            _migration_lock: lock,
        })
    }

    fn validate_database(db: &Database) -> Result<()> {
        let read_txn = db.begin_read().map_err(redb_error)?;
        read_txn.open_table(NAME_INDEX).map_err(redb_error)?;
        read_txn.open_table(OBJECT_INDEX).map_err(redb_error)?;
        read_txn.open_table(MANIFESTS).map_err(redb_error)?;
        read_txn.open_table(SNAPSHOTS).map_err(redb_error)?;
        read_txn.open_table(WATCHER_FILES).map_err(redb_error)?;
        let config = read_txn.open_table(WATCHER_CONFIG).map_err(redb_error)?;
        match config.get(MIGRATION_MARKER).map_err(redb_error)? {
            Some(value) if value.value() == MIGRATION_VERSION => Ok(()),
            Some(value) => Err(OosLiteError::Internal(format!(
                "Unsupported metadata backend version: {}",
                String::from_utf8_lossy(value.value())
            ))),
            None => Err(OosLiteError::Internal(
                "Metadata database has no migration marker; refusing to overwrite it".to_string(),
            )),
        }
    }

    fn initialize_database(db: &Database, legacy: Option<&sled::Db>) -> Result<()> {
        let write_txn = db.begin_write().map_err(redb_error)?;
        {
            let mut names = write_txn.open_table(NAME_INDEX).map_err(redb_error)?;
            let mut objects = write_txn.open_table(OBJECT_INDEX).map_err(redb_error)?;
            let mut manifests = write_txn.open_table(MANIFESTS).map_err(redb_error)?;
            let mut snapshots = write_txn.open_table(SNAPSHOTS).map_err(redb_error)?;
            let mut watcher_files = write_txn.open_table(WATCHER_FILES).map_err(redb_error)?;
            let mut watcher_config = write_txn.open_table(WATCHER_CONFIG).map_err(redb_error)?;
            if let Some(legacy) = legacy {
                copy_legacy_tree(&legacy.open_tree("name_index")?, &mut names)?;
                copy_legacy_tree(&legacy.open_tree("object_index")?, &mut objects)?;
                copy_legacy_tree(&legacy.open_tree("manifests")?, &mut manifests)?;
                copy_legacy_tree(&legacy.open_tree("snapshots")?, &mut snapshots)?;
                copy_legacy_tree(&legacy.open_tree("watcher_files")?, &mut watcher_files)?;
                copy_legacy_tree(&legacy.open_tree("watcher_config")?, &mut watcher_config)?;
            }
            watcher_config
                .insert(MIGRATION_MARKER, MIGRATION_VERSION)
                .map_err(redb_error)?;
        }
        write_txn.commit().map_err(redb_error)?;
        Ok(())
    }

    fn read_value(
        &self,
        definition: TableDefinition<&'static [u8], &'static [u8]>,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        let read_txn = self.db.begin_read().map_err(redb_error)?;
        let table = read_txn.open_table(definition).map_err(redb_error)?;
        let value = table
            .get(key)
            .map_err(redb_error)?
            .map(|value| value.value().to_vec());
        Ok(value)
    }

    fn write_value(
        &self,
        definition: TableDefinition<&'static [u8], &'static [u8]>,
        key: &[u8],
        value: &[u8],
    ) -> Result<()> {
        let write_txn = self.db.begin_write().map_err(redb_error)?;
        {
            let mut table = write_txn.open_table(definition).map_err(redb_error)?;
            table.insert(key, value).map_err(redb_error)?;
        }
        write_txn.commit().map_err(redb_error)?;
        Ok(())
    }

    fn remove_value(
        &self,
        definition: TableDefinition<&'static [u8], &'static [u8]>,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        let write_txn = self.db.begin_write().map_err(redb_error)?;
        let previous = {
            let mut table = write_txn.open_table(definition).map_err(redb_error)?;
            let previous = table.remove(key).map_err(redb_error)?;
            previous.map(|value| value.value().to_vec())
        };
        write_txn.commit().map_err(redb_error)?;
        Ok(previous)
    }

    pub fn resolve_name(&self, name: &str) -> Result<Option<ObjectId>> {
        let Some(value) = self.read_value(NAME_INDEX, name.as_bytes())? else {
            return Ok(None);
        };
        if value.len() == 16 {
            let mut bytes = [0u8; 16];
            bytes.copy_from_slice(&value);
            return Ok(Some(ObjectId::from_raw(bytes)));
        }
        Ok(None)
    }

    pub fn bind_name(&self, name: &str, id: &ObjectId) -> Result<()> {
        self.write_value(NAME_INDEX, name.as_bytes(), id.as_bytes().as_slice())
    }

    pub fn unbind_name(&self, name: &str) -> Result<Option<ObjectId>> {
        let Some(value) = self.remove_value(NAME_INDEX, name.as_bytes())? else {
            return Ok(None);
        };
        if value.len() == 16 {
            let mut bytes = [0u8; 16];
            bytes.copy_from_slice(&value);
            return Ok(Some(ObjectId::from_raw(bytes)));
        }
        Ok(None)
    }

    pub fn rename_name_binding(&self, old_name: &str, new_name: &str) -> Result<bool> {
        let write_txn = self.db.begin_write().map_err(redb_error)?;
        let moved = {
            let mut names = write_txn.open_table(NAME_INDEX).map_err(redb_error)?;
            let previous = names
                .remove(old_name.as_bytes())
                .map_err(redb_error)?
                .map(|value| value.value().to_vec());
            if let Some(bytes) = previous {
                names
                    .insert(new_name.as_bytes(), bytes.as_slice())
                    .map_err(redb_error)?;
                true
            } else {
                false
            }
        };
        write_txn.commit().map_err(redb_error)?;
        Ok(moved)
    }

    pub fn delete_named_object(&self, name: &str) -> Result<Option<ObjectId>> {
        let write_txn = self.db.begin_write().map_err(redb_error)?;
        let removed = {
            let mut names = write_txn.open_table(NAME_INDEX).map_err(redb_error)?;
            let mut objects = write_txn.open_table(OBJECT_INDEX).map_err(redb_error)?;
            let previous = names.remove(name.as_bytes()).map_err(redb_error)?;
            if let Some(value) = previous {
                let bytes = value.value().to_vec();
                drop(value);
                if bytes.len() == 16 {
                    let mut object_id = [0u8; 16];
                    object_id.copy_from_slice(&bytes);
                    objects.remove(object_id.as_slice()).map_err(redb_error)?;
                    Some(ObjectId::from_raw(object_id))
                } else {
                    None
                }
            } else {
                None
            }
        };
        write_txn.commit().map_err(redb_error)?;
        Ok(removed)
    }

    pub fn delete_object(&self, id: &ObjectId) -> Result<()> {
        self.remove_value(OBJECT_INDEX, id.as_bytes().as_slice())?;
        Ok(())
    }

    pub fn delete_manifest(&self, manifest_id: &str) -> Result<()> {
        self.remove_value(MANIFESTS, manifest_id.as_bytes())?;
        Ok(())
    }

    pub fn list_all_manifest_ids(&self) -> Result<Vec<String>> {
        let read_txn = self.db.begin_read().map_err(redb_error)?;
        let table = read_txn.open_table(MANIFESTS).map_err(redb_error)?;
        let mut list = Vec::new();
        for item in table.iter().map_err(redb_error)? {
            let (key, _) = item.map_err(redb_error)?;
            list.push(String::from_utf8_lossy(key.value()).to_string());
        }
        Ok(list)
    }

    pub fn delete_snapshot(&self, label: &str) -> Result<bool> {
        Ok(self.remove_value(SNAPSHOTS, label.as_bytes())?.is_some())
    }

    pub fn get_object(&self, id: &ObjectId) -> Result<Option<ObjectRecord>> {
        let Some(value) = self.read_value(OBJECT_INDEX, id.as_bytes().as_slice())? else {
            return Ok(None);
        };
        Ok(Some(ObjectRecord::from_bytes(&value)?))
    }

    pub(crate) fn visit_objects(
        &self,
        mut visitor: impl FnMut(ObjectRecord) -> Result<()>,
    ) -> Result<()> {
        let read_txn = self.db.begin_read().map_err(redb_error)?;
        let table = read_txn.open_table(OBJECT_INDEX).map_err(redb_error)?;
        for item in table.iter().map_err(redb_error)? {
            let (_, value) = item.map_err(redb_error)?;
            visitor(ObjectRecord::from_bytes(value.value())?)?;
        }
        Ok(())
    }

    pub(crate) fn update_watcher_bindings(
        &self,
        watch_root: &str,
        changes: &[(String, Option<String>)],
    ) -> Result<usize> {
        let write_txn = self.db.begin_write().map_err(redb_error)?;
        let count = {
            let mut names = write_txn.open_table(NAME_INDEX).map_err(redb_error)?;
            let mut tracked = write_txn.open_table(WATCHER_FILES).map_err(redb_error)?;
            let mut moved = Vec::new();
            for (old, new) in changes {
                let id = names.remove(old.as_bytes()).map_err(redb_error)?;
                let old_key = Self::watcher_file_key(watch_root, old);
                tracked.remove(old_key.as_slice()).map_err(redb_error)?;
                if let Some(id) = id {
                    moved.push((new, id.value().to_vec()));
                }
            }
            for (new, id) in &moved {
                if let Some(new) = new {
                    names
                        .insert(new.as_bytes(), id.as_slice())
                        .map_err(redb_error)?;
                    let new_key = Self::watcher_file_key(watch_root, new);
                    tracked
                        .insert(new_key.as_slice(), &[] as &[u8])
                        .map_err(redb_error)?;
                }
            }
            moved.len()
        };
        write_txn.commit().map_err(redb_error)?;
        Ok(count)
    }

    pub fn put_object(&self, record: &ObjectRecord) -> Result<()> {
        self.write_value(
            OBJECT_INDEX,
            record.object_id.as_bytes().as_slice(),
            &record.to_bytes(),
        )
    }

    pub fn save_manifest(&self, manifest: &Manifest) -> Result<String> {
        let id = manifest.content_id();
        self.write_value(MANIFESTS, id.as_bytes(), &manifest.to_bytes())?;
        Ok(id)
    }

    pub fn get_manifest(&self, manifest_id: &str) -> Result<Option<Manifest>> {
        let Some(value) = self.read_value(MANIFESTS, manifest_id.as_bytes())? else {
            return Ok(None);
        };
        Ok(Some(Manifest::from_bytes(&value)?))
    }

    pub fn list_named_objects(&self) -> Result<Vec<(String, ObjectId, ObjectRecord)>> {
        let entries = {
            let read_txn = self.db.begin_read().map_err(redb_error)?;
            let table = read_txn.open_table(NAME_INDEX).map_err(redb_error)?;
            let mut entries = Vec::new();
            for item in table.iter().map_err(redb_error)? {
                let (key, value) = item.map_err(redb_error)?;
                if value.value().len() == 16 {
                    let mut bytes = [0u8; 16];
                    bytes.copy_from_slice(value.value());
                    entries.push((
                        String::from_utf8_lossy(key.value()).to_string(),
                        ObjectId::from_raw(bytes),
                    ));
                }
            }
            entries
        };

        let mut result = Vec::new();
        for (name, id) in entries {
            if let Some(record) = self.get_object(&id)? {
                result.push((name, id, record));
            }
        }
        Ok(result)
    }

    fn watcher_file_prefix(watch_root: &str) -> Vec<u8> {
        let mut prefix = watch_root.as_bytes().to_vec();
        prefix.push(0);
        prefix
    }

    fn watcher_file_key(watch_root: &str, logical_name: &str) -> Vec<u8> {
        let mut key = Self::watcher_file_prefix(watch_root);
        key.extend_from_slice(logical_name.as_bytes());
        key
    }

    pub fn mark_watcher_file(&self, watch_root: &str, logical_name: &str) -> Result<()> {
        self.write_value(
            WATCHER_FILES,
            &Self::watcher_file_key(watch_root, logical_name),
            &[],
        )
    }

    pub fn unmark_watcher_file(&self, watch_root: &str, logical_name: &str) -> Result<()> {
        self.remove_value(
            WATCHER_FILES,
            &Self::watcher_file_key(watch_root, logical_name),
        )?;
        Ok(())
    }

    pub fn list_watcher_files(&self, watch_root: &str) -> Result<Vec<String>> {
        let prefix = Self::watcher_file_prefix(watch_root);
        let read_txn = self.db.begin_read().map_err(redb_error)?;
        let table = read_txn.open_table(WATCHER_FILES).map_err(redb_error)?;
        let mut files = Vec::new();
        for item in table.range(prefix.as_slice()..).map_err(redb_error)? {
            let (key, _) = item.map_err(redb_error)?;
            let key = key.value();
            if !key.starts_with(&prefix) {
                break;
            }
            files.push(String::from_utf8_lossy(&key[prefix.len()..]).to_string());
        }
        Ok(files)
    }

    pub fn save_watcher_config(
        &self,
        watch_dir: &str,
        debounce_secs: u64,
        cooldown_secs: u64,
        throttle_ms: u64,
    ) -> Result<()> {
        let write_txn = self.db.begin_write().map_err(redb_error)?;
        {
            let mut table = write_txn.open_table(WATCHER_CONFIG).map_err(redb_error)?;
            table
                .insert(&b"watch_dir"[..], watch_dir.as_bytes())
                .map_err(redb_error)?;
            table
                .insert(&b"debounce_secs"[..], &debounce_secs.to_le_bytes()[..])
                .map_err(redb_error)?;
            table
                .insert(&b"cooldown_secs"[..], &cooldown_secs.to_le_bytes()[..])
                .map_err(redb_error)?;
            table
                .insert(&b"throttle_ms"[..], &throttle_ms.to_le_bytes()[..])
                .map_err(redb_error)?;
        }
        write_txn.commit().map_err(redb_error)?;
        Ok(())
    }

    pub fn load_watcher_config(&self) -> Result<Option<(String, u64, u64, u64)>> {
        let Some(dir) = self.read_value(WATCHER_CONFIG, b"watch_dir")? else {
            return Ok(None);
        };
        let read_u64 = |key: &[u8], default: u64| -> Result<u64> {
            let Some(value) = self.read_value(WATCHER_CONFIG, key)? else {
                return Ok(default);
            };
            if value.len() != 8 {
                return Ok(default);
            }
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&value);
            Ok(u64::from_le_bytes(bytes))
        };
        Ok(Some((
            String::from_utf8_lossy(&dir).to_string(),
            read_u64(b"debounce_secs", 3)?,
            read_u64(b"cooldown_secs", 60)?,
            read_u64(b"throttle_ms", 10)?,
        )))
    }

    pub fn save_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        self.write_value(SNAPSHOTS, snapshot.label.as_bytes(), &snapshot.to_bytes())
    }

    pub fn get_snapshot(&self, label: &str) -> Result<Option<Snapshot>> {
        let Some(value) = self.read_value(SNAPSHOTS, label.as_bytes())? else {
            return Ok(None);
        };
        Ok(Some(Snapshot::from_bytes(&value)?))
    }

    pub fn list_snapshots(&self) -> Result<Vec<Snapshot>> {
        let read_txn = self.db.begin_read().map_err(redb_error)?;
        let table = read_txn.open_table(SNAPSHOTS).map_err(redb_error)?;
        let mut results = Vec::new();
        for item in table.iter().map_err(redb_error)? {
            let (_, value) = item.map_err(redb_error)?;
            results.push(Snapshot::from_bytes(value.value())?);
        }
        results.sort_by_key(|snapshot| snapshot.created_at);
        Ok(results)
    }

    pub fn count_snapshots(&self) -> usize {
        self.table_len(SNAPSHOTS)
    }

    pub fn count_manifests(&self) -> usize {
        self.table_len(MANIFESTS)
    }

    fn table_len(&self, definition: TableDefinition<&'static [u8], &'static [u8]>) -> usize {
        let Ok(read_txn) = self.db.begin_read() else {
            return 0;
        };
        let Ok(table) = read_txn.open_table(definition) else {
            return 0;
        };
        table
            .len()
            .ok()
            .and_then(|len| usize::try_from(len).ok())
            .unwrap_or(0)
    }

    /// redb commits are durable by default, so this compatibility method is intentionally a no-op.
    pub fn flush(&self) -> Result<()> {
        Ok(())
    }

    #[cfg(test)]
    fn overwrite_manifest_bytes_for_test(&self, manifest_id: &str, bytes: &[u8]) -> Result<()> {
        self.write_value(MANIFESTS, manifest_id.as_bytes(), bytes)
    }
}

fn copy_legacy_tree(legacy: &sled::Tree, target: &mut BytesTable<'_>) -> Result<()> {
    for item in legacy.iter() {
        let (key, value) = item?;
        target
            .insert(key.as_ref(), value.as_ref())
            .map_err(redb_error)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn corrupt_root_manifest_aborts_gc_without_sweeping() {
        for snapshot_only in [false, true] {
            let dir = tempdir().unwrap();
            let engine = crate::StorageEngine::open(dir.path().join("store")).unwrap();
            let source = dir.path().join("source");
            std::fs::write(&source, b"root content").unwrap();
            let root = engine.put_file_named("root", &source).unwrap();
            if snapshot_only {
                engine.create_snapshot("backup").unwrap();
                engine.delete_file("root").unwrap();
            } else {
                engine.unbind_file("root").unwrap();
            }
            std::fs::write(&source, b"orphan content").unwrap();
            let orphan = engine.put_file_named("orphan", &source).unwrap();
            engine.delete_file("orphan").unwrap();
            let metadata = engine.metadata_store();
            let mut bytes = metadata
                .read_value(MANIFESTS, root.manifest_id.as_bytes())
                .unwrap()
                .unwrap();
            bytes[16] ^= 0xff;
            metadata
                .overwrite_manifest_bytes_for_test(&root.manifest_id, &bytes)
                .unwrap();
            let count = engine.segment_store().chunk_count();
            assert!(engine.gc().is_err());
            assert_eq!(engine.segment_store().chunk_count(), count);
            assert!(metadata
                .get_manifest(&orphan.manifest_id)
                .unwrap()
                .is_some());
            assert_eq!(
                metadata
                    .read_value(MANIFESTS, root.manifest_id.as_bytes())
                    .unwrap()
                    .unwrap(),
                bytes
            );
        }
    }

    #[test]
    fn test_name_and_object_index_workflow() {
        let dir = tempdir().expect("tempdir failed");
        let store = MetadataStore::open(dir.path()).expect("store open failed");
        let name = "backup/file.txt";
        assert!(store.resolve_name(name).unwrap().is_none());

        let obj_id = ObjectId::generate();
        let mut record = ObjectRecord::new(obj_id, "manifest_v1_hash".to_string(), 512);
        store.bind_name(name, &obj_id).unwrap();
        store.put_object(&record).unwrap();
        store.flush().unwrap();

        let resolved_id = store.resolve_name(name).unwrap().expect("should resolve");
        assert_eq!(resolved_id, obj_id);
        let loaded = store
            .get_object(&resolved_id)
            .unwrap()
            .expect("should get object");
        assert_eq!(loaded.latest_version, 1);
        assert_eq!(loaded.latest_manifest_id(), "manifest_v1_hash");

        record.add_version("manifest_v2_hash".to_string(), 1024);
        store.put_object(&record).unwrap();
        store.flush().unwrap();
        drop(store);

        let store2 = MetadataStore::open(dir.path()).expect("store re-open failed");
        let resolved2 = store2
            .resolve_name(name)
            .unwrap()
            .expect("should resolve after restart");
        assert_eq!(resolved2, obj_id);
        let loaded2 = store2
            .get_object(&resolved2)
            .unwrap()
            .expect("should get object after restart");
        assert_eq!(loaded2.latest_version, 2);
        assert_eq!(loaded2.latest_manifest_id(), "manifest_v2_hash");
        assert_eq!(loaded2.versions.len(), 2);
        assert_eq!(loaded2.versions[0].manifest_id, "manifest_v1_hash");
        assert_eq!(loaded2.versions[1].manifest_id, "manifest_v2_hash");
    }

    #[test]
    fn test_watcher_state_survives_restart_and_preserves_zero_throttle() {
        let dir = tempdir().expect("tempdir failed");
        let store = MetadataStore::open(dir.path()).expect("store open failed");
        store.mark_watcher_file("root-a", "docs/a.txt").unwrap();
        store.mark_watcher_file("root-a", "docs/b.txt").unwrap();
        store.mark_watcher_file("root-b", "other.txt").unwrap();
        store
            .save_watcher_config("C:\\Users\\Example\\Documents", 3, 60, 0)
            .unwrap();
        drop(store);

        let reopened = MetadataStore::open(dir.path()).expect("store re-open failed");
        assert_eq!(
            reopened.list_watcher_files("root-a").unwrap(),
            vec!["docs/a.txt".to_string(), "docs/b.txt".to_string()]
        );
        assert_eq!(
            reopened.list_watcher_files("root-b").unwrap(),
            vec!["other.txt".to_string()]
        );
        assert_eq!(
            reopened.load_watcher_config().unwrap(),
            Some(("C:\\Users\\Example\\Documents".to_string(), 3, 60, 0))
        );
        reopened
            .unmark_watcher_file("root-a", "docs/a.txt")
            .unwrap();
        assert_eq!(
            reopened.list_watcher_files("root-a").unwrap(),
            vec!["docs/b.txt".to_string()]
        );
    }

    #[test]
    fn migrates_existing_sled_metadata_once() {
        let dir = tempdir().unwrap();
        let object_id = ObjectId::generate();
        let legacy = sled::open(dir.path()).unwrap();
        legacy
            .open_tree("name_index")
            .unwrap()
            .insert(b"legacy.txt", object_id.as_bytes().as_slice())
            .unwrap();
        drop(legacy);

        let store = MetadataStore::open(dir.path()).unwrap();
        assert_eq!(store.resolve_name("legacy.txt").unwrap(), Some(object_id));
        assert!(dir.path().join("metadata.redb").is_file());
        drop(store);

        let reopened = MetadataStore::open(dir.path()).unwrap();
        assert_eq!(
            reopened.resolve_name("legacy.txt").unwrap(),
            Some(object_id)
        );
    }

    #[test]
    fn engine_layout_migration_blocks_old_sled_opening() {
        let dir = tempdir().unwrap();
        let store_path = dir.path().join("store");
        let metadata_path = store_path.join("metadata.db");
        std::fs::create_dir_all(&store_path).unwrap();
        let legacy = sled::open(&metadata_path).unwrap();
        legacy
            .open_tree("name_index")
            .unwrap()
            .insert(b"legacy.txt", &[7u8; 16])
            .unwrap();
        legacy.flush().unwrap();
        drop(legacy);

        let store = MetadataStore::open(&metadata_path).unwrap();
        assert!(metadata_path.join("metadata.redb").is_file());
        assert!(metadata_path.join("conf.sled-backup").is_file());
        assert!(sled::open(&metadata_path).is_err());
        assert!(store.resolve_name("legacy.txt").unwrap().is_some());
    }

    #[test]
    fn unsupported_metadata_marker_is_rejected_without_reimport() {
        let dir = tempdir().unwrap();
        let store = MetadataStore::open(dir.path()).unwrap();
        store
            .write_value(WATCHER_CONFIG, MIGRATION_MARKER, b"redb-v2")
            .unwrap();
        drop(store);

        let error = match MetadataStore::open(dir.path()) {
            Ok(_) => panic!("unsupported metadata marker was accepted"),
            Err(error) => error,
        };
        assert!(error
            .to_string()
            .contains("Unsupported metadata backend version"));
    }

    #[test]
    fn missing_required_table_is_rejected_on_open() {
        let dir = tempdir().unwrap();
        let store = MetadataStore::open(dir.path()).unwrap();
        drop(store);
        let db = Database::open(dir.path().join("metadata.redb")).unwrap();
        let write_txn = db.begin_write().unwrap();
        write_txn.delete_table(OBJECT_INDEX).unwrap();
        write_txn.commit().unwrap();
        drop(db);

        let error = match MetadataStore::open(dir.path()) {
            Ok(_) => panic!("missing required table was accepted"),
            Err(error) => error,
        };
        assert!(matches!(error, OosLiteError::Redb(_)));
    }

    #[test]
    fn empty_engine_database_is_rejected_without_reimport() {
        let dir = tempdir().unwrap();
        let store_path = dir.path().join("store");
        let metadata_path = store_path.join("metadata.db");
        let legacy_path = store_path.join("metadata.db.sled");
        std::fs::create_dir_all(&legacy_path).unwrap();
        let legacy = sled::open(&legacy_path).unwrap();
        legacy
            .open_tree("name_index")
            .unwrap()
            .insert(b"legacy.txt", &[9u8; 16])
            .unwrap();
        legacy.flush().unwrap();
        drop(legacy);
        std::fs::File::create(&metadata_path).unwrap();

        assert!(MetadataStore::open(&metadata_path).is_err());
        assert!(metadata_path.is_file());
        assert_eq!(std::fs::metadata(&metadata_path).unwrap().len(), 0);
        assert!(!store_path.join("metadata.db.corrupt").exists());
    }
}
