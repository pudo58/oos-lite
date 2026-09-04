use fastcdc::FastCDC;

pub const MIN_CHUNK_SIZE: usize = 16 * 1024;   // 16 KiB
pub const TARGET_CHUNK_SIZE: usize = 64 * 1024; // 64 KiB
pub const MAX_CHUNK_SIZE: usize = 256 * 1024;  // 256 KiB

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkCut {
    pub offset: usize,
    pub length: usize,
}

pub struct Chunker<'a> {
    data: &'a [u8],
    min: usize,
    target: usize,
    max: usize,
}

impl<'a> Chunker<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self::with_sizes(data, MIN_CHUNK_SIZE, TARGET_CHUNK_SIZE, MAX_CHUNK_SIZE)
    }

    pub fn with_sizes(data: &'a [u8], min: usize, target: usize, max: usize) -> Self {
        Self { data, min, target, max }
    }

    pub fn chunks(&self) -> Vec<&'a [u8]> {
        if self.data.is_empty() {
            return Vec::new();
        }

        if self.data.len() <= self.min {
            return vec![self.data];
        }

        let cdc = FastCDC::new(self.data, self.min, self.target, self.max);
        let mut result = Vec::new();
        for entry in cdc {
            result.push(&self.data[entry.offset..entry.offset + entry.length]);
        }
        result
    }

    pub fn cuts(&self) -> Vec<ChunkCut> {
        if self.data.is_empty() {
            return Vec::new();
        }

        if self.data.len() <= self.min {
            return vec![ChunkCut { offset: 0, length: self.data.len() }];
        }

        let cdc = FastCDC::new(self.data, self.min, self.target, self.max);
        let mut result = Vec::new();
        for entry in cdc {
            result.push(ChunkCut {
                offset: entry.offset,
                length: entry.length,
            });
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunker_empty_and_small() {
        let empty: &[u8] = b"";
        assert!(Chunker::new(empty).chunks().is_empty());

        let small = b"Hello small payload";
        let chunks = Chunker::new(small).chunks();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], small);
    }

    #[test]
    fn test_chunker_deterministic_boundaries() {
        // Create 1 MiB of repeatable pseudo-random data
        let mut data = Vec::with_capacity(1024 * 1024);
        for i in 0..(1024 * 1024) {
            data.push(((i * 31 + 17) % 256) as u8);
        }

        let cuts1 = Chunker::new(&data).cuts();
        let cuts2 = Chunker::new(&data).cuts();

        // Must produce identical deterministic cuts
        assert_eq!(cuts1, cuts2);
        assert!(cuts1.len() > 1);

        // Verify bounds
        for cut in &cuts1 {
            assert!(cut.length >= MIN_CHUNK_SIZE, "Cut length {} < min {}", cut.length, MIN_CHUNK_SIZE);
            assert!(cut.length <= MAX_CHUNK_SIZE, "Cut length {} > max {}", cut.length, MAX_CHUNK_SIZE);
        }

        // Verify concatenated chunks match original data exactly
        let mut reconstructed = Vec::new();
        for chunk in Chunker::new(&data).chunks() {
            reconstructed.extend_from_slice(chunk);
        }
        assert_eq!(reconstructed, data);
    }

    #[test]
    fn test_content_defined_deduplication_on_modification() {
        // Base 1 MiB data
        let mut base = Vec::with_capacity(1024 * 1024);
        for i in 0..(1024 * 1024) {
            base.push(((i * 37 + 13) % 256) as u8);
        }

        let base_chunks = Chunker::new(&base).chunks();
        assert!(base_chunks.len() >= 3, "Expected at least 3 chunks, got {}", base_chunks.len());

        // Modified: change bytes in the middle
        let mut modified = base.clone();
        let mid = modified.len() / 2;
        for i in 0..100 {
            modified[mid + i] ^= 0xAA;
        }

        let mod_chunks = Chunker::new(&modified).chunks();

        // The first chunk and last chunk MUST remain identical due to CDC property
        assert_eq!(base_chunks[0], mod_chunks[0], "First chunk must be identical");
        assert_eq!(
            base_chunks.last().unwrap(),
            mod_chunks.last().unwrap(),
            "Last chunk must be identical"
        );
    }
}
