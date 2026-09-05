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

/// Streaming chunker that processes arbitrary `std::io::Read` streams with bounded memory (O(MAX_CHUNK_SIZE) RAM).
pub struct StreamChunker<R> {
    reader: R,
    buffer: Vec<u8>,
    min: usize,
    target: usize,
    max: usize,
    eof_reached: bool,
}

impl<R: std::io::Read> StreamChunker<R> {
    pub fn new(reader: R) -> Self {
        Self::with_sizes(reader, MIN_CHUNK_SIZE, TARGET_CHUNK_SIZE, MAX_CHUNK_SIZE)
    }

    pub fn with_sizes(reader: R, min: usize, target: usize, max: usize) -> Self {
        Self {
            reader,
            buffer: Vec::with_capacity(max * 4),
            min,
            target,
            max,
            eof_reached: false,
        }
    }

    /// Pulls the next chunk from the stream.
    /// Memory consumption is strictly bounded by ~1 MiB regardless of total stream size.
    pub fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, std::io::Error> {
        loop {
            // When we have at least `max` bytes, or EOF has been reached:
            if self.buffer.len() >= self.max || (self.eof_reached && !self.buffer.is_empty()) {
                if self.buffer.len() <= self.min && self.eof_reached {
                    let chunk = std::mem::take(&mut self.buffer);
                    return Ok(Some(chunk));
                }

                let cdc = FastCDC::new(&self.buffer, self.min, self.target, self.max);
                if let Some(entry) = cdc.into_iter().next() {
                    let chunk = self.buffer[entry.offset..entry.offset + entry.length].to_vec();
                    let consumed = entry.offset + entry.length;
                    self.buffer.drain(..consumed);
                    return Ok(Some(chunk));
                } else if self.eof_reached {
                    if !self.buffer.is_empty() {
                        let chunk = std::mem::take(&mut self.buffer);
                        return Ok(Some(chunk));
                    }
                    return Ok(None);
                }
            }

            if self.eof_reached {
                if !self.buffer.is_empty() {
                    let chunk = std::mem::take(&mut self.buffer);
                    return Ok(Some(chunk));
                }
                return Ok(None);
            }

            // Read the next block from reader
            let prev_len = self.buffer.len();
            let read_size = (self.max * 2).max(512 * 1024);
            self.buffer.resize(prev_len + read_size, 0);
            let n = self.reader.read(&mut self.buffer[prev_len..])?;
            self.buffer.truncate(prev_len + n);
            if n == 0 {
                self.eof_reached = true;
            }
        }
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

    #[test]
    fn test_stream_chunker_matches_slice_chunker() {
        // 512 KiB payload
        let mut data = Vec::with_capacity(512 * 1024);
        for i in 0..(512 * 1024) {
            data.push(((i * 41 + 19) % 256) as u8);
        }

        let slice_chunks = Chunker::new(&data).chunks();

        let mut stream_chunker = StreamChunker::new(&data[..]);
        let mut streamed_chunks = Vec::new();
        while let Some(chunk) = stream_chunker.next_chunk().unwrap() {
            streamed_chunks.push(chunk);
        }

        assert_eq!(slice_chunks.len(), streamed_chunks.len(), "Chunk count mismatch");
        for (i, (sc, st)) in slice_chunks.iter().zip(streamed_chunks.iter()).enumerate() {
            assert_eq!(*sc, &st[..], "Chunk #{} content mismatch", i);
        }
    }
}
