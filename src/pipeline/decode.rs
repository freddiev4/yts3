use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use log::info;
use rayon::prelude::*;

use crate::config::Yts3Config;
use crate::crypto;
use crate::fountain;
use crate::packet;
use crate::video::decoder::VideoDecoder;

/// Full decode pipeline: video -> packets -> fountain decode -> [decrypt] -> reassemble file.
pub fn decode_file(
    input_path: &str,
    output_path: &Path,
    password: Option<&str>,
    cfg: &Yts3Config,
) -> Result<()> {
    // Step 1: Decode video frames into raw packet data
    info!("decoding video: {}", input_path);
    let decoder = VideoDecoder::new(cfg);
    let raw_data = decoder.decode_from_file(input_path)?;

    // Step 2: Scan for and parse packets
    info!("scanning for packets...");
    let packets = packet::scan_for_packets(&raw_data);
    info!("found {} valid packets", packets.len());

    if packets.is_empty() {
        anyhow::bail!("no valid packets found in video");
    }

    // Extract file ID from first packet
    let file_id = packets[0].header.file_id;
    let encrypted = packets[0].header.is_encrypted();

    // Derive encryption key if needed
    let key = if encrypted {
        let pw = password.ok_or_else(|| {
            anyhow::anyhow!("file is encrypted but no password provided")
        })?;
        Some(crypto::derive_key(pw.as_bytes(), &file_id)?)
    } else {
        None
    };

    // Step 3: Group packets by chunk index
    let mut chunk_packets: HashMap<u32, Vec<&packet::Packet>> = HashMap::new();
    let mut chunk_metadata: HashMap<u32, (u32, u32, u32, bool)> = HashMap::new(); // (k, chunk_size, original_size, is_last)

    for pkt in &packets {
        let ci = pkt.header.chunk_index;
        chunk_packets.entry(ci).or_default().push(pkt);
        chunk_metadata.entry(ci).or_insert((
            pkt.header.k,
            pkt.header.chunk_size,
            pkt.header.original_size,
            pkt.header.is_last_chunk(),
        ));
    }

    let num_chunks = chunk_packets.len();
    info!("found {} chunks to decode", num_chunks);

    let progress = ProgressBar::new(num_chunks as u64);
    progress.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.green/black} {pos}/{len} chunks ({eta})")
            .unwrap()
            .progress_chars("##-"),
    );

    // Step 4: Fountain-decode each chunk
    let mut chunk_indices: Vec<u32> = chunk_packets.keys().copied().collect();
    chunk_indices.sort();

    let decoded_chunks: Vec<(u32, Vec<u8>)> = chunk_indices
        .par_iter()
        .map(|&ci| {
            let pkts = &chunk_packets[&ci];
            let (k, chunk_size, _original_size, _is_last) = chunk_metadata[&ci];

            let mut fdecoder = fountain::ChunkDecoder::new(k as usize, pkts[0].header.symbol_size as usize);

            for pkt in pkts {
                fdecoder.add_symbol(pkt.header.esi, pkt.payload.clone(), pkt.header.is_repair());
            }

            let recovered = fdecoder
                .recover(chunk_size as usize)
                .expect("fountain decoding failed for chunk");

            // Decrypt if needed
            let chunk_data = if let Some(ref k) = key {
                crypto::decrypt_chunk(k, &file_id, ci, &recovered)
                    .expect("decryption failed for chunk")
            } else {
                recovered
            };

            progress.inc(1);
            (ci, chunk_data)
        })
        .collect();

    progress.finish_with_message("decoding complete");

    // Step 5: Reassemble file in chunk order
    info!("reassembling file: {}", output_path.display());
    let mut sorted_chunks = decoded_chunks;
    sorted_chunks.sort_by_key(|(idx, _)| *idx);

    let mut outfile =
        File::create(output_path).context("failed to create output file")?;
    for (_, data) in &sorted_chunks {
        outfile
            .write_all(data)
            .context("failed to write output data")?;
    }
    outfile.flush()?;

    // Securely zero the key
    if let Some(mut k) = key {
        crypto::secure_zero(&mut k);
    }

    info!("decode complete! output: {}", output_path.display());
    Ok(())
}
