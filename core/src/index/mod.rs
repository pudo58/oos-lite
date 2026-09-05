//! Name index (path -> ObjectID) and Object index (ObjectID -> manifest) powered by sled.

use std::path::Path;
use sled::{Db, Transactional, Tree};
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
}

impl MetadataStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db_path = path.as_ref();
        let db = sled::open(db_path)?;

        let tree_names = db.open_tree("name_index")?;
        let tree_objects = db.open_tree("object_index")?;
        let tree_manifests = db.open_tree("manifests")?;
        let tree_snapshots = db.open_tree("snapshots")?;

        info!("MetadataStore opened at: {}", db_path.display());

        Ok(Self {
            db,
            tree_names,
            tree_objects,
            tree_manifests,
            tree_snapshots,
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
        self.tree_names.insert(name.as_bytes(), id.as_bytes().as_slice())?;
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
        let res: TransactionResult<bool, OosLiteError> =
            self.tree_names.transaction(|names| {
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

    /// Saves or updates an ObjectRecord in the object_index tree.
    pub fn put_object(&self, record: &ObjectRecord) -> Result<()> {
        let bytes = record.to_bytes();
        self.tree_objects.insert(record.object_id.as_bytes().as_slice(), bytes)?;
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

    pub fn save_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        let bytes = snapshot.to_bytes();
        self.tree_snapshots.insert(snapshot.label.as_bytes(), bytes)?;
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

        let loaded = store.get_object(&resolved_id).unwrap().expect("should get object");
        assert_eq!(loaded.latest_version, 1);
        assert_eq!(loaded.latest_manifest_id(), "manifest_v1_hash");

        // Add version 2
        record.add_version("manifest_v2_hash".to_string(), 1024);
        store.put_object(&record).unwrap();
        store.flush().unwrap();

        // Reload store across restart
        drop(store);
        let store2 = MetadataStore::open(dir.path()).expect("store re-open failed");
        let resolved2 = store2.resolve_name(name).unwrap().expect("should resolve after restart");
        assert_eq!(resolved2, obj_id);

        let loaded2 = store2.get_object(&resolved2).unwrap().expect("should get object after restart");
        assert_eq!(loaded2.latest_version, 2);
        assert_eq!(loaded2.latest_manifest_id(), "manifest_v2_hash");
        assert_eq!(loaded2.versions.len(), 2);
        assert_eq!(loaded2.versions[0].manifest_id, "manifest_v1_hash");
        assert_eq!(loaded2.versions[1].manifest_id, "manifest_v2_hash");
    }
}
