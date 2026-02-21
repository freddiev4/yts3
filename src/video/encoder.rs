use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use log::info;
use rayon::prelude::*;

use crate::config::{self, Yts3Config};
use crate::video::dct::DctTables;

/// Encode a sequence of packet byte streams into an FFV1/MKV video file.
///
/// Each frame is a grayscale 8-bit image where data is embedded in 8x8 DCT blocks.
/// Uses the ffmpeg CLI to produce the final video.
pub struct VideoEncoder {
    width: u32,
    height: u32,
    fps: u32,
    dct: DctTables,
    blocks_x: usize,
    blocks_y: usize,
    bytes_per_frame: usize,
}

impl VideoEncoder {
    pub fn new(cfg: &Yts3Config) -> Self {
        let dct = DctTables::new(cfg.coefficient_strength);
        let blocks_x = cfg.frame_width as usize / config::BLOCK_SIZE;
        let blocks_y = cfg.frame_height as usize / config::BLOCK_SIZE;
        let bytes_per_frame =
            config::bytes_per_frame(cfg.frame_width, cfg.frame_height, cfg.bits_per_block);

        Self {
            width: cfg.frame_width,
            height: cfg.frame_height,
            fps: cfg.fps,
            dct,
            blocks_x,
            blocks_y,
            bytes_per_frame,
        }
    }

    pub fn bytes_per_frame(&self) -> usize {
        self.bytes_per_frame
    }

    /// Encode all packet data into a video file.
    /// `packet_data` is the concatenation of all serialized packets.
    pub fn encode_to_file(&self, output_path: &str, packet_data: &[u8]) -> Result<()> {
        let num_frames = (packet_data.len() + self.bytes_per_frame - 1) / self.bytes_per_frame;
        info!(
            "encoding {} bytes into {} frames ({}x{} @ {} fps)",
            packet_data.len(),
            num_frames,
            self.width,
            self.height,
            self.fps
        );

        // Scale FFV1 slice count to available threads for better intra-frame parallelism
        // inside ffmpeg. Clamped to 16 (a reasonable FFV1 upper bound).
        let ffv1_slices = rayon::current_num_threads().min(16).to_string();

        let mut child = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "rawvideo",
                "-pixel_format",
                "gray",
                "-video_size",
                &format!("{}x{}", self.width, self.height),
                "-framerate",
                &self.fps.to_string(),
                "-i",
                "pipe:0",
                "-c:v",
                "ffv1",
                "-level",
                "3",
                "-slices",
                &ffv1_slices,
                "-slicecrc",
                "1",
                output_path,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("failed to spawn ffmpeg process — is ffmpeg installed?")?;

        let stdin = child.stdin.as_mut().unwrap();

        // Render frames in parallel batches, then write each batch to ffmpeg in order.
        // Batch size matches the rayon thread pool so we keep all cores busy without
        // holding more than `threads * frame_size` bytes of rendered pixel data at once.
        let batch_size = rayon::current_num_threads();
        let mut frame_idx = 0;
        while frame_idx < num_frames {
            let batch_end = (frame_idx + batch_size).min(num_frames);
            let frames: Vec<Vec<u8>> = (frame_idx..batch_end)
                .into_par_iter()
                .map(|idx| {
                    let data_offset = idx * self.bytes_per_frame;
                    let data_end = (data_offset + self.bytes_per_frame).min(packet_data.len());
                    let frame_data = if data_offset < packet_data.len() {
                        &packet_data[data_offset..data_end]
                    } else {
                        &[]
                    };
                    self.render_frame(frame_data)
                })
                .collect();

            for frame_pixels in &frames {
                stdin
                    .write_all(frame_pixels)
                    .context("failed to write frame data to ffmpeg")?;
            }
            frame_idx = batch_end;
        }

        drop(child.stdin.take());
        let status = child.wait().context("ffmpeg process failed")?;
        if !status.success() {
            anyhow::bail!("ffmpeg exited with status: {}", status);
        }

        info!("video encoding complete: {}", output_path);
        Ok(())
    }

    /// Render a single frame: embed data bytes into 8x8 DCT blocks.
    /// Returns a flat array of grayscale pixels (width * height).
    fn render_frame(&self, data: &[u8]) -> Vec<u8> {
        let frame_size = self.width as usize * self.height as usize;
        let mut pixels = vec![128u8; frame_size]; // mid-gray background

        let mut bit_index = 0usize;
        let total_bits = data.len() * 8;

        for by in 0..self.blocks_y {
            for bx in 0..self.blocks_x {
                if bit_index >= total_bits {
                    break;
                }

                let byte_idx = bit_index / 8;
                let bit_pos = 7 - (bit_index % 8); // MSB first
                let bit = (data[byte_idx] >> bit_pos) & 1;
                bit_index += 1;

                let block = &self.dct.embed_blocks[bit as usize];

                let px = bx * config::BLOCK_SIZE;
                let py = by * config::BLOCK_SIZE;
                for row in 0..config::BLOCK_SIZE {
                    let frame_offset = (py + row) * self.width as usize + px;
                    let block_offset = row * config::BLOCK_SIZE;
                    pixels[frame_offset..frame_offset + config::BLOCK_SIZE]
                        .copy_from_slice(&block[block_offset..block_offset + config::BLOCK_SIZE]);
                }
            }
        }

        pixels
    }
}
