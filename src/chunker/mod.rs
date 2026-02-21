use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

use crate::config;

/// A chunk of file data with its index and whether it is the last chunk.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub index: u32,
    pub data: Vec<u8>,
    pub is_last: bool,
}

/// Read a file and split it into fixed-size chunks.
/// Uses buffered I/O to avoid loading the entire file into memory at once.
pub fn chunk_file(path: &Path, chunk_size: usize) -> io::Result<Vec<Chunk>> {
    let file = File::open(path)?;
    let file_len = file.metadata()?.len() as usize;
    let mut reader = BufReader::with_capacity(chunk_size, file);
    let mut chunks = Vec::new();
    let mut index: u32 = 0;
    let mut total_read = 0usize;

    loop {
        let mut buf = vec![0u8; chunk_size];
        let mut filled = 0;

        // Read exactly chunk_size bytes (or until EOF)
        while filled < chunk_size {
            match reader.read(&mut buf[filled..]) {
                Ok(0) => break, // EOF
                Ok(n) => filled += n,
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }

        if filled == 0 {
            // If the file was an exact multiple of chunk_size, mark the previous
            // chunk as last.
            if let Some(last) = chunks.last_mut() {
                let last: &mut Chunk = last;
                last.is_last = true;
            }
            break;
        }

        buf.truncate(filled);
        total_read += filled;
        let is_last = total_read >= file_len;

        chunks.push(Chunk {
            index,
            data: buf,
            is_last,
        });

        index += 1;

        if is_last {
            break;
        }
    }

    // Handle empty file
    if chunks.is_empty() {
        chunks.push(Chunk {
            index: 0,
            data: Vec::new(),
            is_last: true,
        });
    }

    Ok(chunks)
}

/// Split an in-memory byte buffer into chunks.
pub fn chunk_bytes(data: &[u8], chunk_size: usize) -> Vec<Chunk> {
    if data.is_empty() {
        return vec![Chunk {
            index: 0,
            data: Vec::new(),
            is_last: true,
        }];
    }

    let num_chunks = (data.len() + chunk_size - 1) / chunk_size;
    let mut chunks = Vec::with_capacity(num_chunks);

    for (i, slice) in data.chunks(chunk_size).enumerate() {
        chunks.push(Chunk {
            index: i as u32,
            data: slice.to_vec(),
            is_last: i == num_chunks - 1,
        });
    }

    chunks
}

/// Compute the effective chunk size when encryption is enabled.
pub fn effective_chunk_size(chunk_size: usize, encrypted: bool) -> usize {
    if encrypted {
        config::chunk_size_for_encryption(chunk_size)
    } else {
        chunk_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_chunk_bytes_single() {
        let data = vec![1u8; 100];
        let chunks = chunk_bytes(&data, 1024);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].index, 0);
        assert_eq!(chunks[0].data.len(), 100);
        assert!(chunks[0].is_last);
    }

    #[test]
    fn test_chunk_bytes_multiple() {
        let data = vec![0xABu8; 2500];
        let chunks = chunk_bytes(&data, 1000);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].data.len(), 1000);
        assert_eq!(chunks[1].data.len(), 1000);
        assert_eq!(chunks[2].data.len(), 500);
        assert!(!chunks[0].is_last);
        assert!(!chunks[1].is_last);
        assert!(chunks[2].is_last);
    }

    #[test]
    fn test_chunk_bytes_exact_multiple() {
        let data = vec![0u8; 2048];
        let chunks = chunk_bytes(&data, 1024);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[1].is_last);
    }

    #[test]
    fn test_chunk_bytes_empty() {
        let chunks = chunk_bytes(&[], 1024);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].is_last);
        assert!(chunks[0].data.is_empty());
    }

    #[test]
    fn test_chunk_file_roundtrip() {
        let dir = std::env::temp_dir().join("yts3_test_chunker");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_input.bin");

        let data: Vec<u8> = (0..5000).map(|i| (i % 256) as u8).collect();
        {
            let mut f = File::create(&path).unwrap();
            f.write_all(&data).unwrap();
        }

        let chunks = chunk_file(&path, 2000).unwrap();
        assert_eq!(chunks.len(), 3);

        let mut reassembled = Vec::new();
        for c in &chunks {
            reassembled.extend_from_slice(&c.data);
        }
        assert_eq!(reassembled, data);

        assert!(chunks.last().unwrap().is_last);
        std::fs::remove_dir_all(&dir).ok();
    }
}
