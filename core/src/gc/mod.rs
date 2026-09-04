//! Mark-and-sweep garbage collection over live snapshots and versions.

use std::collections::HashSet;
use tracing::info;

use crate::chunk::ChunkId;
use crate::error::Result;
use crate::index::MetadataStore;
use crate::segment::SegmentStore;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GcStats {
    pub live_roots: usize,
    pub reachable_chunks: usize,
    pub chunks_reclaimed: usize,
    pub manifests_reclaimed: usize,
    pub active_chunks_retained: usize,
}

pub struct GarbageCollector;

impl GarbageCollector {
    /// Mark Phase: Scans all live roots (Name Index + Snapshots) and identifies
    /// all reachable ChunkIds and ManifestIds.
    pub fn mark(
        metadata_store: &MetadataStore,
    ) -> Result<(HashSet<ChunkId>, HashSet<String>, usize)> {
        let mut reachable_chunks = HashSet::new();
        let mut reachable_manifests = HashSet::new();
        let mut live_roots = 0;

        // 1. Scan Name Index (all active named files and all their historical versions)
        let named_objects = metadata_store.list_named_objects()?;
        for (_name, _id, record) in named_objects {
            live_roots += 1;
            for version in &record.versions {
                reachable_manifests.insert(version.manifest_id.clone());
                if let Some(manifest) = metadata_store.get_manifest(&version.manifest_id)? {
                    for cid in manifest.chunks {
                        reachable_chunks.insert(cid);
                    }
                }
            }
        }

        // 2. Scan Snapshots (all historical references preserved by snapshots)
        let snapshots = metadata_store.list_snapshots()?;
        for snap in snapshots {
            live_roots += 1;
            for entry in snap.entries {
                reachable_manifests.insert(entry.manifest_id.clone());
                if let Some(manifest) = metadata_store.get_manifest(&entry.manifest_id)? {
                    for cid in manifest.chunks {
                        reachable_chunks.insert(cid);
                    }
                }
            }
        }

        info!(
            live_roots = live_roots,
            reachable_chunks = reachable_chunks.len(),
            reachable_manifests = reachable_manifests.len(),
            "GC Mark phase completed"
        );

        Ok((reachable_chunks, reachable_manifests, live_roots))
    }

    /// Sweep Phase: Reclaims unreachable chunks from SegmentStore and manifests from MetadataStore.
    pub fn sweep(
        segment_store: &SegmentStore,
        metadata_store: &MetadataStore,
        reachable_chunks: &HashSet<ChunkId>,
        reachable_manifests: &HashSet<String>,
        live_roots: usize,
    ) -> Result<GcStats> {
        // 1. Sweep dead manifests
        let all_manifest_ids = metadata_store.list_all_manifest_ids()?;
        let mut manifests_reclaimed = 0;
        for mid in all_manifest_ids {
            if !reachable_manifests.contains(&mid) {
                metadata_store.delete_manifest(&mid)?;
                manifests_reclaimed += 1;
            }
        }
        metadata_store.flush()?;

        // 2. Sweep dead chunks and compact SegmentStore on disk
        let chunks_reclaimed = segment_store.compact_and_reclaim(reachable_chunks)?;
        let active_chunks_retained = segment_store.chunk_count();

        info!(
            chunks_reclaimed = chunks_reclaimed,
            manifests_reclaimed = manifests_reclaimed,
            active_chunks_retained = active_chunks_retained,
            "GC Sweep phase completed"
        );

        Ok(GcStats {
            live_roots,
            reachable_chunks: reachable_chunks.len(),
            chunks_reclaimed,
            manifests_reclaimed,
            active_chunks_retained,
        })
    }

    /// Full GC cycle: Mark followed by Sweep.
    pub fn collect(
        segment_store: &SegmentStore,
        metadata_store: &MetadataStore,
    ) -> Result<GcStats> {
        let (reachable_chunks, reachable_manifests, live_roots) = Self::mark(metadata_store)?;
        Self::sweep(
            segment_store,
            metadata_store,
            &reachable_chunks,
            &reachable_manifests,
            live_roots,
        )
    }
}
