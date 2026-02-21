use anyhow::{Context, Result};
use log::info;
use rayon::prelude::*;

use crate::config::{self, Yts3Config};
use crate::video::dct::DctTables;

/// Decode an FFV1/MKV video file back into raw packet bytes.
pub struct VideoDecoder {
    width: u32,
    height: u32,
    dct: DctTables,
    blocks_x: usize,
    blocks_y: usize,
    bytes_per_frame: usize,
}

impl VideoDecoder {
    pub fn new(cfg: &Yts3Config) -> Self {
        let dct = DctTables::new(cfg.coefficient_strength);
        let blocks_x = cfg.frame_width as usize / config::BLOCK_SIZE;
        let blocks_y = cfg.frame_height as usize / config::BLOCK_SIZE;
        let bytes_per_frame =
            config::bytes_per_frame(cfg.frame_width, cfg.frame_height, cfg.bits_per_block);

        Self {
            width: cfg.frame_width,
            height: cfg.frame_height,
            dct,
            blocks_x,
            blocks_y,
            bytes_per_frame,
        }
    }

    pub fn bytes_per_frame(&self) -> usize {
        self.bytes_per_frame
    }

    /// Decode all frames from a video file and return the concatenated packet data.
    pub fn decode_from_file(&self, input_path: &str) -> Result<Vec<u8>> {
        use std::process::{Command, Stdio};

        info!("decoding video: {}", input_path);

        let mut child = Command::new("ffmpeg")
            .args([
                "-i",
                input_path,
                "-f",
                "rawvideo",
                "-pixel_format",
                "gray",
                "-video_size",
                &format!("{}x{}", self.width, self.height),
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("failed to spawn ffmpeg for decoding")?;

        let stdout = child.stdout.as_mut().unwrap();
        let frame_size = self.width as usize * self.height as usize;
        let mut all_data = Vec::new();
        let mut frame_count = 0u64;

        // Read frames in batches from ffmpeg (I/O must be sequential) and extract
        // bits from each batch in parallel. Batch size matches the rayon thread pool
        // so all cores stay busy while we keep memory bounded to `threads * frame_size`.
        let batch_size = rayon::current_num_threads();
        let mut batch: Vec<Vec<u8>> = Vec::with_capacity(batch_size);

        loop {
            let mut frame_buf = vec![0u8; frame_size];
            match read_exact_or_eof(stdout, &mut frame_buf) {
                Ok(true) => {
                    batch.push(frame_buf);
                    frame_count += 1;

                    if batch.len() >= batch_size {
                        let extracted: Vec<Vec<u8>> = batch
                            .par_iter()
                            .map(|f| self.extract_frame(f))
                            .collect();
                        for frame_data in extracted {
                            all_data.extend_from_slice(&frame_data);
                        }
                        batch.clear();
                    }
                }
                Ok(false) => break, // EOF
                Err(e) => return Err(e.into()),
            }
        }

        // Process any remaining frames in the last (partial) batch
        if !batch.is_empty() {
            let extracted: Vec<Vec<u8>> = batch
                .par_iter()
                .map(|f| self.extract_frame(f))
                .collect();
            for frame_data in extracted {
                all_data.extend_from_slice(&frame_data);
            }
        }

        let status = child.wait().context("ffmpeg decode process failed")?;
        if !status.success() {
            anyhow::bail!("ffmpeg decode exited with status: {}", status);
        }

        info!("decoded {} frames, {} bytes total", frame_count, all_data.len());
        Ok(all_data)
    }

    /// Extract data bytes from a single grayscale frame.
    fn extract_frame(&self, pixels: &[u8]) -> Vec<u8> {
        let total_bits = self.blocks_x * self.blocks_y;
        let total_bytes = total_bits / 8;
        let mut data = vec![0u8; total_bytes];
        let mut bit_index = 0usize;

        for by in 0..self.blocks_y {
            for bx in 0..self.blocks_x {
                if bit_index / 8 >= total_bytes {
                    break;
                }

                // Extract the 8x8 block from the frame
                let px = bx * config::BLOCK_SIZE;
                let py = by * config::BLOCK_SIZE;
                let mut block = [0u8; 64];
                for row in 0..config::BLOCK_SIZE {
                    let frame_offset = (py + row) * self.width as usize + px;
                    let block_offset = row * config::BLOCK_SIZE;
                    block[block_offset..block_offset + config::BLOCK_SIZE]
                        .copy_from_slice(&pixels[frame_offset..frame_offset + config::BLOCK_SIZE]);
                }

                // Extract bit using DCT projection
                let bit = self.dct.extract_bit(&block);

                // Pack into output bytes (MSB first)
                let byte_idx = bit_index / 8;
                let bit_pos = 7 - (bit_index % 8);
                if byte_idx < data.len() {
                    data[byte_idx] |= bit << bit_pos;
                }
                bit_index += 1;
            }
        }

        // Trim to bytes_per_frame since not all block bits may carry data
        data.truncate(self.bytes_per_frame);
        data
    }
}

/// Read exactly `buf.len()` bytes, returning Ok(false) on clean EOF.
fn read_exact_or_eof(reader: &mut impl std::io::Read, buf: &mut [u8]) -> std::io::Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => {
                if filled == 0 {
                    return Ok(false); // Clean EOF
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "partial frame read",
                    ));
                }
            }
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}
