//! Name index (path -> ObjectID) and Object index (ObjectID -> manifest) powered by sled.

use sled::{Db, Transactional, Tree};
use std::path::Path;
use tracing::info;

use crate::error::{OosLiteError, Result};
use crate::manifest::Manifest;
use crate::object::{ObjectId, ObjectRecord};

use crate::snapshot::Snapshot;

pub struct MetadataStore {
    db: Db,
    tree_names: Tree,
    tree_objects: Tree,
    tree_manifests: Tree,
    tree_snapshots: Tree,
    tree_watcher_files: Tree,
    tree_watcher_config: Tree,
}

impl MetadataStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db_path = path.as_ref();
        let db = sled::open(db_path)?;

        let tree_names = db.open_tree("name_index")?;
        let tree_objects = db.open_tree("object_index")?;
        let tree_manifests = db.open_tree("manifests")?;
        let tree_snapshots = db.open_tree("snapshots")?;
        let tree_watcher_files = db.open_tree("watcher_files")?;
        let tree_watcher_config = db.open_tree("watcher_config")?;

        info!("MetadataStore opened at: {}", db_path.display());

        Ok(Self {
            db,
            tree_names,
            tree_objects,
            tree_manifests,
            tree_snapshots,
            tree_watcher_files,
            tree_watcher_config,
        })
    }

    /// Resolves a user-provided file name / path string to its persistent ObjectId.
    pub fn resolve_name(&self, name: &str) -> Result<Option<ObjectId>> {
        if let Some(ivec) = self.tree_names.get(name.as_bytes())? {
            if ivec.len() == 16 {
                let mut bytes = [0u8; 16];
                bytes.copy_from_slice(&ivec);
                return Ok(Some(ObjectId::from_raw(bytes)));
            }
        }
        Ok(None)
    }

    /// Associates a user name with an ObjectId.
    pub fn bind_name(&self, name: &str, id: &ObjectId) -> Result<()> {
        self.tree_names
            .insert(name.as_bytes(), id.as_bytes().as_slice())?;
        Ok(())
    }

    /// Removes a name binding from name_index, returning the previously associated ObjectId.
    pub fn unbind_name(&self, name: &str) -> Result<Option<ObjectId>> {
        if let Some(ivec) = self.tree_names.remove(name.as_bytes())? {
            if ivec.len() == 16 {
                let mut bytes = [0u8; 16];
                bytes.copy_from_slice(&ivec);
                return Ok(Some(ObjectId::from_raw(bytes)));
            }
        }
        Ok(None)
    }

    /// Atomically renames a logical name binding in a single transaction, preserving the ObjectId.
    pub fn rename_name_binding(&self, old_name: &str, new_name: &str) -> Result<bool> {
        use sled::transaction::TransactionResult;
        let res: TransactionResult<bool, OosLiteError> = self.tree_names.transaction(|names| {
            if let Some(ivec) = names.remove(old_name.as_bytes())? {
                names.insert(new_name.as_bytes(), ivec)?;
                Ok(true)
            } else {
                Ok(false)
            }
        });

        match res {
            Ok(b) => Ok(b),
            Err(sled::transaction::TransactionError::Abort(e)) => Err(e),
            Err(sled::transaction::TransactionError::Storage(e)) => Err(OosLiteError::Database(e)),
        }
    }

    /// Atomically unbinds name and deletes associated object record in a single transaction.
    pub fn delete_named_object(&self, name: &str) -> Result<Option<ObjectId>> {
        use sled::transaction::TransactionResult;
        let res: TransactionResult<Option<ObjectId>, OosLiteError> =
            (&self.tree_names, &self.tree_objects).transaction(|(names, objects)| {
                if let Some(ivec) = names.remove(name.as_bytes())? {
                    if ivec.len() == 16 {
                        let mut bytes = [0u8; 16];
                        bytes.copy_from_slice(&ivec);
                        objects.remove(bytes.as_slice())?;
                        return Ok(Some(ObjectId::from_raw(bytes)));
                    }
                }
                Ok(None)
            });

        match res {
            Ok(opt) => Ok(opt),
            Err(sled::transaction::TransactionError::Abort(e)) => Err(e),
            Err(sled::transaction::TransactionError::Storage(e)) => Err(OosLiteError::Database(e)),
        }
    }

    /// Removes an ObjectRecord from object_index.
    pub fn delete_object(&self, id: &ObjectId) -> Result<()> {
        self.tree_objects.remove(id.as_bytes().as_slice())?;
        Ok(())
    }

    /// Removes a Manifest from manifests tree.
    pub fn delete_manifest(&self, manifest_id: &str) -> Result<()> {
        self.tree_manifests.remove(manifest_id.as_bytes())?;
        Ok(())
    }

    /// Lists all manifest IDs currently stored.
    pub fn list_all_manifest_ids(&self) -> Result<Vec<String>> {
        let mut list = Vec::new();
        for item in self.tree_manifests.iter() {
            let (k, _) = item?;
            list.push(String::from_utf8_lossy(&k).to_string());
        }
        Ok(list)
    }

    /// Deletes a snapshot by label.
    pub fn delete_snapshot(&self, label: &str) -> Result<bool> {
        let removed = self.tree_snapshots.remove(label.as_bytes())?;
        Ok(removed.is_some())
    }

    /// Retrieves the full ObjectRecord (including complete version history) by ObjectId.
    pub fn get_object(&self, id: &ObjectId) -> Result<Option<ObjectRecord>> {
        if let Some(ivec) = self.tree_objects.get(id.as_bytes().as_slice())? {
            let record = ObjectRecord::from_bytes(&ivec)?;
            return Ok(Some(record));
        }
        Ok(None)
    }

    pub(crate) fn all_objects(&self) -> impl Iterator<Item = Result<ObjectRecord>> + '_ {
        self.tree_objects.iter().map(|item| {
            let (_, bytes) = item?;
            ObjectRecord::from_bytes(&bytes)
        })
    }

    // The engine holds its namespace write lock while constructing and applying this batch.
    pub(crate) fn update_watcher_bindings(
        &self,
        watch_root: &str,
        changes: &[(String, Option<String>)],
    ) -> Result<usize> {
        use sled::transaction::TransactionResult;
        let result: TransactionResult<usize, OosLiteError> =
            (&self.tree_names, &self.tree_watcher_files).transaction(|(names, tracked)| {
                let mut moved = Vec::new();
                for (old, new) in changes {
                    let id = names.remove(old.as_bytes())?;
                    tracked.remove(Self::watcher_file_key(watch_root, old))?;
                    if let Some(id) = id {
                        moved.push((new, id));
                    }
                }
                for (new, id) in &moved {
                    if let Some(new) = new {
                        names.insert(new.as_bytes(), id.clone())?;
                        tracked.insert(Self::watcher_file_key(watch_root, new), &[])?;
                    }
                }
                Ok(moved.len())
            });
        let count = match result {
            Ok(count) => count,
            Err(sled::transaction::TransactionError::Abort(e)) => return Err(e),
            Err(sled::transaction::TransactionError::Storage(e)) => return Err(e.into()),
        };
        self.flush()?;
        Ok(count)
    }

    /// Saves or updates an ObjectRecord in the object_index tree.
    pub fn put_object(&self, record: &ObjectRecord) -> Result<()> {
        let bytes = record.to_bytes();
        self.tree_objects
            .insert(record.object_id.as_bytes().as_slice(), bytes)?;
        Ok(())
    }

    /// Stores a Manifest into the manifests sled tree, keyed by its content ID.
    pub fn save_manifest(&self, manifest: &Manifest) -> Result<String> {
        let id = manifest.content_id();
        let bytes = manifest.to_bytes();
        self.tree_manifests.insert(id.as_bytes(), bytes)?;
        Ok(id)
    }

    /// Loads a Manifest from the manifests sled tree.
    pub fn get_manifest(&self, manifest_id: &str) -> Result<Option<Manifest>> {
        if let Some(ivec) = self.tree_manifests.get(manifest_id.as_bytes())? {
            let manifest = Manifest::from_bytes(&ivec)?;
            return Ok(Some(manifest));
        }
        Ok(None)
    }

    /// Lists all entries currently registered in the name index along with their latest ObjectRecord.
    pub fn list_named_objects(&self) -> Result<Vec<(String, ObjectId, ObjectRecord)>> {
        let mut result = Vec::new();
        for item in self.tree_names.iter() {
            let (k, v) = item?;
            let name = String::from_utf8_lossy(&k).to_string();
            if v.len() == 16 {
                let mut bytes = [0u8; 16];
                bytes.copy_from_slice(&v);
                let id = ObjectId::from_raw(bytes);
                if let Some(record) = self.get_object(&id)? {
                    result.push((name, id, record));
                }
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
        self.tree_watcher_files
            .insert(Self::watcher_file_key(watch_root, logical_name), &[])?;
        Ok(())
    }

    pub fn unmark_watcher_file(&self, watch_root: &str, logical_name: &str) -> Result<()> {
        self.tree_watcher_files
            .remove(Self::watcher_file_key(watch_root, logical_name))?;
        Ok(())
    }

    pub fn list_watcher_files(&self, watch_root: &str) -> Result<Vec<String>> {
        let prefix = Self::watcher_file_prefix(watch_root);
        let mut files = Vec::new();
        for item in self.tree_watcher_files.scan_prefix(&prefix) {
            let (key, _) = item?;
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
        self.tree_watcher_config
            .insert("watch_dir", watch_dir.as_bytes())?;
        self.tree_watcher_config
            .insert("debounce_secs", &debounce_secs.to_le_bytes())?;
        self.tree_watcher_config
            .insert("cooldown_secs", &cooldown_secs.to_le_bytes())?;
        self.tree_watcher_config
            .insert("throttle_ms", &throttle_ms.to_le_bytes())?;
        self.flush()
    }

    pub fn load_watcher_config(&self) -> Result<Option<(String, u64, u64, u64)>> {
        let Some(dir) = self.tree_watcher_config.get("watch_dir")? else {
            return Ok(None);
        };
        let read_u64 = |key: &str, default: u64| -> Result<u64> {
            let Some(value) = self.tree_watcher_config.get(key)? else {
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
            read_u64("debounce_secs", 3)?,
            read_u64("cooldown_secs", 60)?,
            read_u64("throttle_ms", 10)?,
        )))
    }

    pub fn save_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        let bytes = snapshot.to_bytes();
        self.tree_snapshots
            .insert(snapshot.label.as_bytes(), bytes)?;
        Ok(())
    }

    pub fn get_snapshot(&self, label: &str) -> Result<Option<Snapshot>> {
        if let Some(ivec) = self.tree_snapshots.get(label.as_bytes())? {
            let snap = Snapshot::from_bytes(&ivec)?;
            return Ok(Some(snap));
        }
        Ok(None)
    }

    pub fn list_snapshots(&self) -> Result<Vec<Snapshot>> {
        let mut results = Vec::new();
        for item in self.tree_snapshots.iter() {
            let (_k, v) = item?;
            let snap = Snapshot::from_bytes(&v)?;
            results.push(snap);
        }
        // Sort by created_at ascending
        results.sort_by_key(|s| s.created_at);
        Ok(results)
    }

    pub fn count_snapshots(&self) -> usize {
        self.tree_snapshots.len()
    }

    pub fn count_manifests(&self) -> usize {
        self.tree_manifests.len()
    }

    pub fn flush(&self) -> Result<()> {
        self.db.flush()?;
        Ok(())
    }
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
            let mut bytes = metadata.tree_manifests.get(root.manifest_id.as_bytes()).unwrap().unwrap().to_vec();
            bytes[16] ^= 0xff;
            metadata.tree_manifests.insert(root.manifest_id.as_bytes(), bytes.clone()).unwrap();
            let count = engine.segment_store().chunk_count();
            assert!(engine.gc().is_err());
            assert_eq!(engine.segment_store().chunk_count(), count);
            assert!(metadata.get_manifest(&orphan.manifest_id).unwrap().is_some());
            assert_eq!(metadata.tree_manifests.get(root.manifest_id.as_bytes()).unwrap().unwrap().as_ref(), bytes);
        }
    }

    #[test]
    fn test_name_and_object_index_workflow() {
        let dir = tempdir().expect("tempdir failed");
        let store = MetadataStore::open(dir.path()).expect("store open failed");

        let name = "backup/file.txt";
        assert!(store.resolve_name(name).unwrap().is_none());

        // Create object v1
        let obj_id = ObjectId::generate();
        let mut record = ObjectRecord::new(obj_id, "manifest_v1_hash".to_string(), 512);

        store.bind_name(name, &obj_id).unwrap();
        store.put_object(&record).unwrap();
        store.flush().unwrap();

        // Check resolve
        let resolved_id = store.resolve_name(name).unwrap().expect("should resolve");
        assert_eq!(resolved_id, obj_id);

        let loaded = store
            .get_object(&resolved_id)
            .unwrap()
            .expect("should get object");
        assert_eq!(loaded.latest_version, 1);
        assert_eq!(loaded.latest_manifest_id(), "manifest_v1_hash");

        // Add version 2
        record.add_version("manifest_v2_hash".to_string(), 1024);
        store.put_object(&record).unwrap();
        store.flush().unwrap();

        // Reload store across restart
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
}
