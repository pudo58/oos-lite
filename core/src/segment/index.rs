use std::collections::HashMap;
use std::sync::RwLock;
use crate::chunk::ChunkId;
use super::format::ChunkLocation;

#[derive(Debug, Default)]
pub struct SegmentIndex {
    locations: RwLock<HashMap<ChunkId, ChunkLocation>>,
}

impl SegmentIndex {
    pub fn new() -> Self {
        Self {
            locations: RwLock::new(HashMap::new()),
        }
    }

    pub fn insert(&self, chunk_id: ChunkId, location: ChunkLocation) {
        let mut map = self.locations.write().unwrap();
        map.insert(chunk_id, location);
    }

    pub fn get(&self, chunk_id: &ChunkId) -> Option<ChunkLocation> {
        let map = self.locations.read().unwrap();
        map.get(chunk_id).copied()
    }

    pub fn contains(&self, chunk_id: &ChunkId) -> bool {
        let map = self.locations.read().unwrap();
        map.contains_key(chunk_id)
    }

    pub fn len(&self) -> usize {
        let map = self.locations.read().unwrap();
        map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn remove(&self, chunk_id: &ChunkId) -> Option<ChunkLocation> {
        let mut map = self.locations.write().unwrap();
        map.remove(chunk_id)
    }

    pub fn retain<F>(&self, mut f: F) -> usize
    where
        F: FnMut(&ChunkId, &ChunkLocation) -> bool,
    {
        let mut map = self.locations.write().unwrap();
        let initial_len = map.len();
        map.retain(|k, v| f(k, v));
        initial_len - map.len()
    }

    pub fn all_chunk_ids(&self) -> Vec<ChunkId> {
        let map = self.locations.read().unwrap();
        map.keys().copied().collect()
    }

    pub fn entries(&self) -> Vec<(ChunkId, ChunkLocation)> {
        let map = self.locations.read().unwrap();
        map.iter().map(|(k, v)| (*k, *v)).collect()
    }

    pub fn clear(&self) {
        let mut map = self.locations.write().unwrap();
        map.clear();
    }

    pub fn replace_with(&self, other: SegmentIndex) {
        let mut map = self.locations.write().unwrap();
        let other_map = other.locations.into_inner().unwrap();
        *map = other_map;
    }

    pub fn total_payload_bytes(&self) -> u64 {
        let map = self.locations.read().unwrap();
        map.values().map(|loc| loc.payload_len as u64).sum()
    }
}
