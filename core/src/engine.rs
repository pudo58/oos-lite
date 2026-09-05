use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

use crate::chunk::{ChunkId, StreamChunker};
use crate::error::{OosLiteError, Result};
use crate::index::MetadataStore;
use crate::gc::{GarbageCollector, GcStats};
use crate::manifest::Manifest;
use crate::object::{ObjectId, ObjectRecord, ObjectVersion};
use crate::segment::SegmentStore;
use crate::snapshot::{Snapshot, SnapshotEntry};
use crate::wal::{Wal, WalPutPayload, WalRecordPayload};

#[derive(Debug, Clone)]
pub struct PutSummary {
    pub object_id: ObjectId,
    pub version: u32,
    pub manifest_id: String,
    pub total_bytes: u64,
    pub chunk_count: usize,
    pub new_chunks: usize,
    pub dedup_chunks: usize,
}

#[derive(Debug, Clone)]
pub struct EngineStats {
    pub total_chunks: usize,
    pub total_manifests: usize,
    pub total_objects: usize,
    pub total_snapshots: usize,
    pub logical_bytes: u64,
    pub latest_logical_bytes: u64,
    pub unique_chunks_bytes: u64,
    pub physical_disk_bytes: u64,
    pub dedup_ratio: f64,
    pub space_savings_pct: f64,
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

fn check_crash_point(point: &str) {
    if let Ok(target) = std::env::var("OOS_CRASH_AT") {
        if target == point {
            warn!(point = point, "Simulating abrupt process crash (kill -9)");
            std::process::abort();
        }
    }
}

pub struct StorageEngine {
    root_dir: PathBuf,
    segment_store: SegmentStore,
    metadata_store: MetadataStore,
    wal: Mutex<Wal>,
    op_lock: RwLock<()>,
    gc_lock: Mutex<()>,
}

impl StorageEngine {
    pub fn open<P: AsRef<Path>>(root_dir: P) -> Result<Self> {
        let root_dir = root_dir.as_ref().to_path_buf();
        fs::create_dir_all(&root_dir)?;

        let segments_dir = root_dir.join("segments");
        let metadata_dir = root_dir.join("metadata.db");
        let wal_dir = root_dir.join("wal");

        let segment_store = SegmentStore::new(segments_dir)?;
        let metadata_store = MetadataStore::open(metadata_dir)?;
        let mut wal = Wal::open(wal_dir)?;

        // WAL REDO Recovery on startup
        let uncheckpointed = wal.read_uncheckpointed_records()?;
        if !uncheckpointed.is_empty() {
            info!(
                count = uncheckpointed.len(),
                "Replaying uncheckpointed WAL records for crash consistency"
            );
            let mut max_lsn = wal.checkpoint_lsn();
            for record in uncheckpointed {
                if let WalRecordPayload::PutObject(put) = record.payload {
                    // Step 1: Replay chunks into SegmentStore (if not already present)
                    for (chunk_id, chunk_data) in &put.chunks {
                        if !chunk_data.is_empty() && !segment_store.has_chunk(chunk_id) {
                            let _ = segment_store.put_chunk(chunk_data)?;
                        }
                    }
                    segment_store.sync()?;

                    // Step 2: Replay manifest into MetadataStore
                    let manifest_id = metadata_store.save_manifest(&put.manifest)?;

                    // Step 3: Replay object index update (idempotent)
                    let mut obj_record = match metadata_store.get_object(&put.object_id)? {
                        Some(rec) => rec,
                        None => ObjectRecord {
                            object_id: put.object_id,
                            latest_version: put.version,
                            versions: Vec::new(),
                        },
                    };
                    if !obj_record.versions.iter().any(|v| v.version == put.version) {
                        obj_record.latest_version = put.version;
                        obj_record.versions.push(ObjectVersion {
                            version: put.version,
                            manifest_id: manifest_id.clone(),
                            size_bytes: put.manifest.total_size,
                            created_at: put.manifest.created_at,
                        });
                        metadata_store.put_object(&obj_record)?;
                    }

                    // Step 4: Replay name index update
                    metadata_store.bind_name(&put.name, &put.object_id)?;
                }
                if record.lsn > max_lsn {
                    max_lsn = record.lsn;
                }
            }
            metadata_store.flush()?;
            wal.checkpoint(max_lsn)?;
            info!(
                checkpoint_lsn = max_lsn,
                "WAL recovery replay completed successfully"
            );
        }

        Ok(Self {
            root_dir,
            segment_store,
            metadata_store,
            wal: Mutex::new(wal),
            op_lock: RwLock::new(()),
            gc_lock: Mutex::new(()),
        })
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn metadata_store(&self) -> &MetadataStore {
        &self.metadata_store
    }



    /// Stores a file with an associated logical name (e.g. "a.txt" or "docs/photo.jpg").
    /// If the name already exists, creates a new version of the existing ObjectID.
    pub fn put_file<P: AsRef<Path>>(&self, file_path: P) -> Result<PutSummary> {
        let file_path = file_path.as_ref();
        let name = file_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed_file");
        self.put_file_named(name, file_path)
    }

    pub fn put_file_named<P: AsRef<Path>>(&self, name: &str, file_path: P) -> Result<PutSummary> {
        let file_path = file_path.as_ref();
        if !file_path.exists() {
            return Err(OosLiteError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File not found: {}", file_path.display()),
            )));
        }

        // Prevent race condition with concurrent GC
        let _op_guard = self.op_lock.read().map_err(|e| {
            OosLiteError::Internal(format!("StorageEngine op_lock poisoned: {e}"))
        })?;

        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        let mut stream_chunker = StreamChunker::new(reader);

        let mut hasher = blake3::Hasher::new();
        let mut total_bytes = 0u64;
        let mut chunk_ids = Vec::new();
        let mut new_chunks_for_wal = Vec::new();

        while let Some(chunk) = stream_chunker.next_chunk()? {
            total_bytes += chunk.len() as u64;
            hasher.update(&chunk);
            let cid = ChunkId::from_data(&chunk);
            chunk_ids.push(cid);
            if !self.segment_store.has_chunk(&cid) {
                new_chunks_for_wal.push((cid, chunk));
            }
            // Deduplicated chunks are dropped immediately from memory here
        }

        let content_hash = *hasher.finalize().as_bytes();
        let manifest = Manifest::new(chunk_ids.clone(), total_bytes, content_hash);

        // Determine ObjectId & version target
        let (object_id, version) = match self.metadata_store.resolve_name(name)? {
            Some(existing_id) => {
                let existing = self.metadata_store.get_object(&existing_id)?.ok_or_else(|| {
                    OosLiteError::Internal(format!(
                        "Inconsistent state: Name {} points to non-existing Object {}",
                        name, existing_id
                    ))
                })?;
                (existing_id, existing.versions.len() as u32 + 1)
            }
            None => (ObjectId::generate(), 1),
        };

        // Step 1: WAL append + fsync
        let wal_payload = WalPutPayload {
            name: name.to_string(),
            object_id,
            version,
            manifest: manifest.clone(),
            chunks: new_chunks_for_wal.clone(),
        };

        let lsn = {
            let mut wal_guard = self.wal.lock().map_err(|e| {
                OosLiteError::Internal(format!("WAL mutex poisoned: {e}"))
            })?;
            wal_guard.append_put_and_sync(wal_payload)?
        };

        check_crash_point("after_wal_fsync");

        // Step 2: Write chunks into SegmentStore + sync directly from memory slices
        let mut new_chunks = 0;
        for (_cid, chunk_data) in &new_chunks_for_wal {
            let (_id, is_new) = self.segment_store.put_chunk(chunk_data)?;
            if is_new {
                new_chunks += 1;
            }
        }
        let dedup_chunks = chunk_ids.len().saturating_sub(new_chunks);
        self.segment_store.sync()?;

        check_crash_point("after_chunk_write");

        // Step 3: Save manifest into MetadataStore
        let manifest_id = self.metadata_store.save_manifest(&manifest)?;

        // Step 4: Update ObjectRecord
        let mut obj_record = match self.metadata_store.get_object(&object_id)? {
            Some(rec) => rec,
            None => ObjectRecord {
                object_id,
                latest_version: version,
                versions: Vec::new(),
            },
        };

        if !obj_record.versions.iter().any(|v| v.version == version) {
            obj_record.latest_version = version;
            obj_record.versions.push(ObjectVersion {
                version,
                manifest_id: manifest_id.clone(),
                size_bytes: total_bytes,
                created_at: manifest.created_at,
            });
            self.metadata_store.put_object(&obj_record)?;
        }

        // Step 5: Update Name Index & flush MetadataStore
        self.metadata_store.bind_name(name, &object_id)?;
        self.metadata_store.flush()?;

        check_crash_point("after_metadata_update");

        // Step 6: Checkpoint WAL
        {
            let mut wal_guard = self.wal.lock().map_err(|e| {
                OosLiteError::Internal(format!("WAL mutex poisoned: {e}"))
            })?;
            wal_guard.checkpoint(lsn)?;
        }

        info!(
            name = %name,
            object_id = %object_id,
            version = version,
            manifest_id = %manifest_id,
            total_bytes = total_bytes,
            new_chunks = new_chunks,
            dedup_chunks = dedup_chunks,
            "Successfully persisted file version with WAL checkpoint"
        );

        Ok(PutSummary {
            object_id,
            version,
            manifest_id,
            total_bytes,
            chunk_count: manifest.chunks.len(),
            new_chunks,
            dedup_chunks,
        })
    }

    /// Resolves target (can be a name string, ObjectId hex, or ManifestId hex) to Manifest.
    fn resolve_manifest(&self, target: &str) -> Result<Manifest> {
        let target = target.trim();

        // 1. Try Name Index
        if let Some(obj_id) = self.metadata_store.resolve_name(target)? {
            if let Some(record) = self.metadata_store.get_object(&obj_id)? {
                let m_id = record.latest_manifest_id();
                if let Some(manifest) = self.metadata_store.get_manifest(m_id)? {
                    return Ok(manifest);
                }
            }
        }

        // 2. Try ObjectId parse
        if let Ok(obj_id) = target.parse::<ObjectId>() {
            if let Some(record) = self.metadata_store.get_object(&obj_id)? {
                let m_id = record.latest_manifest_id();
                if let Some(manifest) = self.metadata_store.get_manifest(m_id)? {
                    return Ok(manifest);
                }
            }
        }

        // 3. Try direct Manifest ID
        if let Some(manifest) = self.metadata_store.get_manifest(target)? {
            return Ok(manifest);
        }

        Err(OosLiteError::ObjectNotFound(format!(
            "Target '{}' could not be resolved to any valid file name, ObjectId, or ManifestId",
            target
        )))
    }

    /// Internal helper to assemble and verify chunks into an output file.
    pub fn extract_manifest_to_file(&self, manifest: &Manifest, out_path: &Path) -> Result<u64> {
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let thread_id = std::thread::current().id();
        let tmp_path = out_path.with_extension(format!(
            "tmp.{}.{:?}.{}",
            std::process::id(),
            thread_id,
            now_ns
        ));
        let mut hasher = blake3::Hasher::new();
        let mut written_bytes = 0u64;

        {
            let mut out_file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp_path)?;

            for chunk_id in &manifest.chunks {
                let chunk_data = self.segment_store.get_chunk(chunk_id)?;
                hasher.update(&chunk_data);
                out_file.write_all(&chunk_data)?;
                written_bytes += chunk_data.len() as u64;
            }

            out_file.sync_all()?;
        }

        let actual_hash = hasher.finalize();
        if actual_hash.as_bytes() != &manifest.content_hash {
            let _ = fs::remove_file(&tmp_path);
            return Err(OosLiteError::Internal(format!(
                "Reconstructed file BLAKE3 mismatch: expected {}, got {}",
                manifest.content_id(),
                actual_hash.to_hex()
            )));
        }

        fs::rename(&tmp_path, out_path)?;
        Ok(written_bytes)
    }

    /// High-level API to restore a file by its Name, ObjectId, or ManifestId, with optional version number.
    pub fn get_file_version<P: AsRef<Path>>(
        &self,
        target: &str,
        version: Option<u32>,
        out_path: P,
    ) -> Result<u64> {
        let manifest = if let Some(v) = version {
            let obj_id = if let Some(id) = self.metadata_store.resolve_name(target)? {
                id
            } else if let Ok(id) = target.parse::<ObjectId>() {
                id
            } else {
                return Err(OosLiteError::ObjectNotFound(format!(
                    "File '{}' not found for version query",
                    target
                )));
            };

            let record = self.metadata_store.get_object(&obj_id)?.ok_or_else(|| {
                OosLiteError::ObjectNotFound(obj_id.to_string())
            })?;

            let version_entry = record
                .versions
                .iter()
                .find(|entry| entry.version == v)
                .ok_or_else(|| {
                    OosLiteError::ObjectNotFound(format!(
                        "Version #{} not found for file '{}'",
                        v, target
                    ))
                })?;

            self.metadata_store
                .get_manifest(&version_entry.manifest_id)?
                .ok_or_else(|| {
                    OosLiteError::Internal(format!(
                        "Manifest {} missing for version #{}",
                        version_entry.manifest_id, v
                    ))
                })?
        } else {
            self.resolve_manifest(target)?
        };

        let out_path = out_path.as_ref();
        let written_bytes = self.extract_manifest_to_file(&manifest, out_path)?;

        info!(
            target = %target,
            version = ?version,
            written_bytes = written_bytes,
            out_path = %out_path.display(),
            "Successfully extracted file"
        );

        Ok(written_bytes)
    }

    /// High-level API to restore a file by its Name, ObjectId, or ManifestId (latest version).
    pub fn get_file<P: AsRef<Path>>(&self, target: &str, out_path: P) -> Result<u64> {
        self.get_file_version(target, None, out_path)
    }

    /// Creates an O(1) point-in-time snapshot of the entire name index namespace.
    /// Does NOT copy any physical chunks (zero-copy / reference-only).
    pub fn create_snapshot(&self, label: &str) -> Result<Snapshot> {
        let _op_guard = self.op_lock.read().map_err(|e| {
            OosLiteError::Internal(format!("StorageEngine op_lock poisoned: {e}"))
        })?;

        let label = label.trim();
        if label.is_empty() {
            return Err(OosLiteError::Internal("Snapshot label cannot be empty".to_string()));
        }

        if self.metadata_store.get_snapshot(label)?.is_some() {
            return Err(OosLiteError::Internal(format!(
                "Snapshot '{}' already exists",
                label
            )));
        }

        let named_objects = self.metadata_store.list_named_objects()?;
        let mut entries = Vec::with_capacity(named_objects.len());

        for (name, id, record) in named_objects {
            if let Some(latest) = record.versions.last() {
                entries.push(SnapshotEntry {
                    name,
                    object_id: id,
                    version: latest.version,
                    manifest_id: latest.manifest_id.clone(),
                    size_bytes: latest.size_bytes,
                });
            }
        }

        let snapshot = Snapshot::new(label.to_string(), entries);
        self.metadata_store.save_snapshot(&snapshot)?;
        self.metadata_store.flush()?;

        info!(
            label = %label,
            entries = snapshot.entries.len(),
            "Successfully created snapshot"
        );

        Ok(snapshot)
    }

    pub fn list_snapshots(&self) -> Result<Vec<Snapshot>> {
        self.metadata_store.list_snapshots()
    }

    pub fn get_snapshot(&self, label: &str) -> Result<Option<Snapshot>> {
        self.metadata_store.get_snapshot(label.trim())
    }

    /// Restores all files captured in a snapshot into target_dir.
    pub fn restore_snapshot<P: AsRef<Path>>(&self, label: &str, target_dir: P) -> Result<usize> {
        let _op_guard = self.op_lock.read().map_err(|e| {
            OosLiteError::Internal(format!("StorageEngine op_lock poisoned: {e}"))
        })?;

        let label = label.trim();
        let snapshot = self.metadata_store.get_snapshot(label)?.ok_or_else(|| {
            OosLiteError::SnapshotNotFound(label.to_string())
        })?;

        let target_dir = target_dir.as_ref();
        fs::create_dir_all(target_dir)?;

        for entry in &snapshot.entries {
            let manifest = self
                .metadata_store
                .get_manifest(&entry.manifest_id)?
                .ok_or_else(|| {
                    OosLiteError::Internal(format!(
                        "Manifest {} not found for snapshot entry {}",
                        entry.manifest_id, entry.name
                    ))
                })?;

            // Sanitize entry.name to prevent path traversal (e.g. '../' or absolute paths)
            let safe_relative_path = Path::new(&entry.name)
                .components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(p) => Some(p),
                    _ => None,
                })
                .collect::<PathBuf>();

            let out_path = target_dir.join(safe_relative_path);
            self.extract_manifest_to_file(&manifest, &out_path)?;
        }

        info!(
            label = %label,
            count = snapshot.entries.len(),
            target_dir = %target_dir.display(),
            "Successfully restored snapshot"
        );

        Ok(snapshot.entries.len())
    }

    /// Retrieves full version history of a file by name or ObjectId.
    pub fn get_versions(&self, target: &str) -> Result<Vec<ObjectVersion>> {
        let target = target.trim();

        let obj_id = if let Some(id) = self.metadata_store.resolve_name(target)? {
            id
        } else if let Ok(id) = target.parse::<ObjectId>() {
            id
        } else {
            return Err(OosLiteError::ObjectNotFound(format!(
                "Object not found for versions query: {}",
                target
            )));
        };

        let record = self
            .metadata_store
            .get_object(&obj_id)?
            .ok_or_else(|| OosLiteError::ObjectNotFound(obj_id.to_string()))?;

        Ok(record.versions)
    }

    /// Lists all named objects currently tracked in store.
    pub fn list_files(&self) -> Result<Vec<(String, ObjectId, ObjectRecord)>> {
        self.metadata_store.list_named_objects()
    }

    pub fn stats(&self) -> EngineStats {
        let named_objects = self.metadata_store.list_named_objects().unwrap_or_default();
        let total_objects = named_objects.len();

        let mut logical_bytes = 0u64;
        let mut latest_logical_bytes = 0u64;

        for (_name, _id, record) in &named_objects {
            if let Some(latest) = record.latest() {
                latest_logical_bytes += latest.size_bytes;
            }
            for v in &record.versions {
                logical_bytes += v.size_bytes;
            }
        }

        let total_chunks = self.segment_store.chunk_count();
        let unique_chunks_bytes = self.segment_store.unique_raw_bytes();


        let seg_disk = self.segment_store.physical_disk_bytes().unwrap_or(0);
        let meta_disk = dir_size(&self.root_dir.join("metadata.db"));
        let wal_disk = dir_size(&self.root_dir.join("wal"));
        let physical_disk_bytes = seg_disk + meta_disk + wal_disk;

        let dedup_ratio = if unique_chunks_bytes > 0 {
            logical_bytes as f64 / unique_chunks_bytes as f64
        } else {
            1.0
        };

        let space_savings_pct = if logical_bytes > 0 && logical_bytes >= unique_chunks_bytes {
            ((logical_bytes - unique_chunks_bytes) as f64 / logical_bytes as f64) * 100.0
        } else {
            0.0
        };

        EngineStats {
            total_chunks,
            total_manifests: self.metadata_store.count_manifests(),
            total_objects,
            total_snapshots: self.metadata_store.count_snapshots(),
            logical_bytes,
            latest_logical_bytes,
            unique_chunks_bytes,
            physical_disk_bytes,
            dedup_ratio,
            space_savings_pct,
        }
    }

    /// Checks the overall health, CRC32C, and BLAKE3 consistency of the entire store.
    pub fn fsck(&self) -> Result<crate::fsck::FsckReport> {
        crate::fsck::FsckRunner::check(
            self.segment_store.segments_dir(),
            &self.segment_store,
            &self.metadata_store,
        )
    }

    /// Deletes a file entry from the name index and cleans up its associated object record.
    pub fn delete_file(&self, name: &str) -> Result<bool> {
        let _op_guard = self.op_lock.write().map_err(|e| {
            OosLiteError::Internal(format!("StorageEngine op_lock poisoned: {e}"))
        })?;

        let name = name.trim();
        if let Some(object_id) = self.metadata_store.unbind_name(name)? {
            let _ = self.metadata_store.delete_object(&object_id);
            self.metadata_store.flush()?;
            info!(name = %name, object_id = %object_id, "Successfully unlinked file");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Deletes a snapshot by label.
    pub fn delete_snapshot(&self, label: &str) -> Result<bool> {
        let _op_guard = self.op_lock.write().map_err(|e| {
            OosLiteError::Internal(format!("StorageEngine op_lock poisoned: {e}"))
        })?;

        let deleted = self.metadata_store.delete_snapshot(label.trim())?;
        if deleted {
            self.metadata_store.flush()?;
            info!(label = %label, "Successfully deleted snapshot");
        }
        Ok(deleted)
    }

    /// Runs a full Mark-and-Sweep Garbage Collection cycle.
    pub fn gc(&self) -> Result<GcStats> {
        // 1. Serialize GC invocations to prevent staging directory corruption
        let _gc_guard = self.gc_lock.lock().map_err(|e| {
            OosLiteError::Internal(format!("StorageEngine gc_lock poisoned: {e}"))
        })?;

        // 2. Block all concurrent Put operations during both Mark & Sweep phases
        let _op_guard = self.op_lock.write().map_err(|e| {
            OosLiteError::Internal(format!("StorageEngine op_lock poisoned: {e}"))
        })?;

        GarbageCollector::collect(&self.segment_store, &self.metadata_store)
    }

    pub fn segment_store(&self) -> &SegmentStore {
        &self.segment_store
    }
}
