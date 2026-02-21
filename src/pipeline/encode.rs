use std::path::Path;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use log::info;
use rayon::prelude::*;

use crate::chunker;
use crate::config::{self, Yts3Config};
use crate::crypto;
use crate::fountain;
use crate::packet;
use crate::video::encoder::VideoEncoder;

/// Full encode pipeline: file -> chunks -> [encrypt] -> fountain -> packets -> video.
pub fn encode_file(
    input_path: &Path,
    output_path: &str,
    password: Option<&str>,
    cfg: &Yts3Config,
) -> Result<()> {
    let file_id = crypto::generate_file_id();
    let encrypted = password.is_some();

    // Derive encryption key if needed
    let key = if let Some(pw) = password {
        Some(crypto::derive_key(pw.as_bytes(), &file_id)?)
    } else {
        None
    };

    let effective_chunk_size = chunker::effective_chunk_size(cfg.chunk_size, encrypted);

    // Step 1: Chunk the file
    info!("chunking input file: {}", input_path.display());
    let chunks = chunker::chunk_file(input_path, effective_chunk_size)
        .context("failed to chunk input file")?;
    let num_chunks = chunks.len();
    info!("split into {} chunks", num_chunks);

    let progress = ProgressBar::new(num_chunks as u64);
    progress.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} chunks ({eta})")
            .unwrap()
            .progress_chars("##-"),
    );

    // Step 2 & 3: Encrypt (if needed) and fountain-encode each chunk, then serialize packets.
    // Process chunks in parallel.
    let all_chunk_packets: Vec<Vec<Vec<u8>>> = chunks
        .par_iter()
        .map(|chunk| {
            let chunk_data = if let Some(ref k) = key {
                crypto::encrypt_chunk(k, &file_id, chunk.index, &chunk.data)
                    .expect("encryption failed")
            } else {
                chunk.data.clone()
            };

            let symbols =
                fountain::encode_chunk(&chunk_data, cfg.symbol_size, cfg.repair_overhead)
                    .expect("fountain encoding failed");

            let k = ((chunk_data.len() + cfg.symbol_size - 1) / cfg.symbol_size) as u32;

            let mut flags = 0u8;
            if encrypted {
                flags |= config::FLAG_ENCRYPTED;
            }
            if chunk.is_last {
                flags |= config::FLAG_LAST_CHUNK;
            }

            let mut chunk_packets = Vec::new();
            for sym in &symbols {
                let mut sym_flags = flags;
                if sym.is_repair {
                    sym_flags |= config::FLAG_REPAIR_SYMBOL;
                }

                let pkt = packet::serialize_packet(
                    &file_id,
                    chunk.index,
                    chunk_data.len() as u32,
                    chunk.data.len() as u32,
                    cfg.symbol_size as u16,
                    k,
                    sym.esi,
                    sym_flags,
                    &sym.data,
                );
                chunk_packets.push(pkt);
            }

            progress.inc(1);
            chunk_packets
        })
        .collect();

    progress.finish_with_message("chunking complete");

    // Flatten all packets into a single byte stream
    let mut packet_stream = Vec::new();
    for chunk_pkts in &all_chunk_packets {
        for pkt in chunk_pkts {
            packet_stream.extend_from_slice(pkt);
        }
    }
    info!("total packet data: {} bytes", packet_stream.len());

    // Step 4: Encode packets into video
    info!("encoding to video: {}", output_path);
    let encoder = VideoEncoder::new(cfg);
    encoder.encode_to_file(output_path, &packet_stream)?;

    // Securely zero the key
    if let Some(mut k) = key {
        crypto::secure_zero(&mut k);
    }

    info!("encode complete!");
    Ok(())
}
