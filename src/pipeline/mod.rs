pub mod decode;
pub mod encode;
pub mod hook;

use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::Result;
use sha2::{Digest, Sha256};

use crate::config::Yts3Config;
use hook::PipelineHook;

/// Result of a full encode → hook → decode roundtrip.
pub struct RoundtripResult {
    /// SHA-256 hex digest of the original input file.
    pub original_hash: String,
    /// SHA-256 hex digest of the decoded output file.
    pub decoded_hash: String,
    /// `true` if the hashes match (lossless round-trip).
    pub matched: bool,
}

/// Run a full encode → hook → decode roundtrip.
///
/// Steps:
/// 1. SHA-256 hashes `input`.
/// 2. Encodes `input` → `encoded_path`.
/// 3. Calls `hook.after_encode(encoded_path)` — upload/download happens here.
/// 4. Decodes the path returned by the hook → `output`.
/// 5. SHA-256 hashes `output` and compares with the original.
///
/// # Example
///
/// ```rust,no_run
/// use std::path::Path;
/// use yts3::{roundtrip, NoopHook, Yts3Config};
///
/// let result = roundtrip(
///     Path::new("input.txt"),
///     "encoded.mkv",
///     Path::new("output.txt"),
///     Some("my-password"),
///     &Yts3Config::default(),
///     &NoopHook,
/// ).unwrap();
///
/// assert!(result.matched, "round-trip failed: {} != {}", result.original_hash, result.decoded_hash);
/// ```
pub fn roundtrip<H: PipelineHook>(
    input: &Path,
    encoded_path: &str,
    output: &Path,
    password: Option<&str>,
    cfg: &Yts3Config,
    hook: &H,
) -> Result<RoundtripResult> {
    let original_hash = sha256_file(input)?;

    encode::encode_file(input, encoded_path, password, cfg)?;

    let decode_from = hook.after_encode(Path::new(encoded_path))?;

    decode::decode_file(
        decode_from.to_str().unwrap(),
        output,
        password,
        cfg,
    )?;

    let decoded_hash = sha256_file(output)?;
    let matched = original_hash == decoded_hash;

    Ok(RoundtripResult {
        original_hash,
        decoded_hash,
        matched,
    })
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
