use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use crate::chunk::ChunkId;

/// Thread-safe LRU cache for decompressed chunk payloads with a strict memory byte cap.
/// Prevents Out-Of-Memory (OOM) and unbounded memory growth when seeking through large media files.
pub struct DecompressedChunkCache {
    inner: Mutex<LruState>,
}

struct LruState {
    chunks: HashMap<ChunkId, Arc<Vec<u8>>>,
    access_order: VecDeque<ChunkId>,
    current_bytes: usize,
    max_bytes: usize,
}

impl DecompressedChunkCache {
    /// Creates a new cache with a maximum capacity in bytes.
    pub fn new(max_bytes: usize) -> Self {
        Self {
            inner: Mutex::new(LruState {
                chunks: HashMap::new(),
                access_order: VecDeque::new(),
                current_bytes: 0,
                max_bytes: if max_bytes == 0 { 128 * 1024 * 1024 } else { max_bytes },
            }),
        }
    }

    /// Retrieves a chunk if present in cache, promoting it to most-recently-used.
    pub fn get(&self, id: &ChunkId) -> Option<Arc<Vec<u8>>> {
        let mut state = self.inner.lock().unwrap();
        if let Some(data) = state.chunks.get(id).cloned() {
            // Promote to MRU: remove from current position and push to back
            if let Some(pos) = state.access_order.iter().position(|k| k == id) {
                state.access_order.remove(pos);
            }
            state.access_order.push_back(*id);
            Some(data)
        } else {
            None
        }
    }

    /// Inserts a chunk into the cache, evicting oldest chunks if memory limit is exceeded.
    pub fn insert(&self, id: ChunkId, data: Vec<u8>) -> Arc<Vec<u8>> {
        let mut state = self.inner.lock().unwrap();

        // If already present, update and promote
        if let Some(existing) = state.chunks.get(&id) {
            let data_arc = Arc::clone(existing);
            if let Some(pos) = state.access_order.iter().position(|k| k == &id) {
                state.access_order.remove(pos);
            }
            state.access_order.push_back(id);
            return data_arc;
        }

        let chunk_size = data.len();

        // Evict LRU entries until enough room exists
        while state.current_bytes + chunk_size > state.max_bytes && !state.access_order.is_empty() {
            if let Some(old_id) = state.access_order.pop_front() {
                if let Some(evicted) = state.chunks.remove(&old_id) {
                    state.current_bytes = state.current_bytes.saturating_sub(evicted.len());
                }
            }
        }

        let data_arc = Arc::new(data);
        state.chunks.insert(id, Arc::clone(&data_arc));
        state.access_order.push_back(id);
        state.current_bytes += chunk_size;

        data_arc
    }

    /// Returns current byte usage in cache.
    pub fn current_bytes(&self) -> usize {
        self.inner.lock().unwrap().current_bytes
    }

    /// Returns total number of cached chunks.
    pub fn chunk_count(&self) -> usize {
        self.inner.lock().unwrap().chunks.len()
    }

    /// Clears all entries from cache.
    pub fn clear(&self) {
        let mut state = self.inner.lock().unwrap();
        state.chunks.clear();
        state.access_order.clear();
        state.current_bytes = 0;
    }
}
