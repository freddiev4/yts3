//! # YouTube Upload Example
//!
//! Demonstrates a full yts3 round-trip through YouTube:
//!
//! ```text
//! input file
//!     → encode (chunking → fountain codes → DCT embed → FFV1/MKV)
//!     → upload to YouTube  (YouTube Data API v3 resumable upload)
//!     → download from YouTube  (yt-dlp)
//!     → decode (DCT extract → fountain decode → reassemble)
//! → output file
//! ```
//!
//! SHA-256 hashes of the input and output are compared at the end to verify
//! the round-trip was lossless.
//!
//! ## Prerequisites
//!
//! - A Google Cloud project with the **YouTube Data API v3** enabled.
//! - An OAuth2 access token with the `youtube.upload` scope.
//!   You can obtain one via the [OAuth2 Playground](https://developers.google.com/oauthplayground)
//!   or your own OAuth2 flow.
//! - [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) installed and on `$PATH`.
//! - [`curl`](https://curl.se/) installed and on `$PATH`.
//! - `ffmpeg` installed and on `$PATH` (required by yts3 core).
//!
//! ## Environment variables
//!
//! ```bash
//! export YOUTUBE_ACCESS_TOKEN="ya29.a0AfH6..."   # OAuth2 bearer token
//! ```
//!
//! ## Running
//!
//! ```bash
//! cargo run --example youtube_upload -- input.txt encoded.mkv output.txt
//! # optionally with encryption:
//! # YOUTUBE_ACCESS_TOKEN=... cargo run --example youtube_upload -- input.txt encoded.mkv output.txt mysecretpassword
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use yts3::{roundtrip, PipelineHook, Yts3Config};

// ---------------------------------------------------------------------------
// Credentials
// ---------------------------------------------------------------------------

/// OAuth2 credentials required to call the YouTube Data API v3.
struct YoutubeCredentials {
    /// Short-lived bearer token with the `youtube.upload` scope.
    /// Obtain one via the OAuth2 Playground or your own auth flow.
    access_token: String,
}

impl YoutubeCredentials {
    /// Load credentials from environment variables so secrets never appear in
    /// source code or command-line history.
    fn from_env() -> Result<Self> {
        let access_token = std::env::var("YOUTUBE_ACCESS_TOKEN")
            .context("YOUTUBE_ACCESS_TOKEN environment variable is not set")?;
        Ok(Self { access_token })
    }
}

// ---------------------------------------------------------------------------
// Hook implementation
// ---------------------------------------------------------------------------

/// A [`PipelineHook`] that performs a full YouTube round-trip between the
/// encode and decode steps.
///
/// `after_encode` is called by [`roundtrip`] with the path of the freshly
/// written `.mkv` file. This implementation:
///
/// 1. Uploads the file to YouTube as an *unlisted* video.
/// 2. Downloads it back with `yt-dlp`.
/// 3. Returns the local path of the downloaded copy so the decoder reads
///    the YouTube-processed version instead of the original local file.
struct YoutubeHook {
    credentials: YoutubeCredentials,
    /// Where to write the downloaded video before decoding.
    download_path: PathBuf,
}

impl YoutubeHook {
    fn new(credentials: YoutubeCredentials, download_path: impl Into<PathBuf>) -> Self {
        Self {
            credentials,
            download_path: download_path.into(),
        }
    }

    /// Upload `path` to YouTube using the **resumable upload** protocol.
    ///
    /// The resumable protocol is preferred for large files because:
    /// - It supports uploads larger than 5 GB.
    /// - Failed uploads can be resumed without re-sending already uploaded bytes.
    ///
    /// The upload happens in two HTTP round-trips:
    ///
    /// **Step 1 — initiate:** POST to the upload endpoint with video metadata.
    /// The response `Location` header contains a unique resumable upload URI
    /// that is valid for 24 hours.
    ///
    /// **Step 2 — upload:** PUT the raw video bytes to that URI. The response
    /// body is a JSON object whose `"id"` field is the YouTube video ID.
    ///
    /// Returns the YouTube video ID (e.g. `"dQw4w9WgXcQ"`).
    fn upload(&self, path: &Path) -> Result<String> {
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("encoded.mkv");

        let file_size = std::fs::metadata(path)
            .with_context(|| format!("cannot stat {}", path.display()))?
            .len();

        // ── Step 1: Initiate the resumable upload session ─────────────────
        //
        // Required headers:
        //   Authorization            – Bearer token
        //   Content-Type             – must be application/json for the metadata body
        //   X-Upload-Content-Type    – MIME type of the video we are about to send
        //   X-Upload-Content-Length  – byte length of the video
        //
        // The query parameter uploadType=resumable selects the resumable protocol.
        // part=snippet,status tells the API which resource parts we are setting.
        let metadata = format!(
            r#"{{"snippet":{{"title":"{filename}","description":"Encoded with yts3 — https://github.com/freddiev4/yts3","categoryId":"28"}},"status":{{"privacyStatus":"unlisted"}}}}"#
        );

        let initiate = Command::new("curl")
            .args([
                "-s",
                "-D", "-", // dump response headers to stdout so we can parse Location
                "-X", "POST",
                "https://www.googleapis.com/upload/youtube/v3/videos\
                 ?uploadType=resumable&part=snippet,status",
                "-H", &format!("Authorization: Bearer {}", self.credentials.access_token),
                "-H", "Content-Type: application/json; charset=UTF-8",
                "-H", "X-Upload-Content-Type: video/x-matroska",
                "-H", &format!("X-Upload-Content-Length: {file_size}"),
                "-d", &metadata,
            ])
            .output()
            .context("failed to spawn curl (is it installed and on $PATH?)")?;

        if !initiate.status.success() {
            bail!(
                "YouTube upload initiation failed:\n{}",
                String::from_utf8_lossy(&initiate.stderr)
            );
        }

        // The response is headers + blank line + body. We only need the
        // Location header from the headers section.
        let initiate_output = String::from_utf8_lossy(&initiate.stdout);
        let upload_uri = initiate_output
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("location:"))
            .and_then(|l| l.splitn(2, ':').nth(1))
            .map(|v| v.trim().to_string())
            .context("no Location header in YouTube upload-initiation response — \
                      check that your access token has the youtube.upload scope")?;

        // ── Step 2: Stream the video bytes to the resumable upload URI ─────
        //
        // Content-Type must match X-Upload-Content-Type from step 1.
        // --data-binary @<file> streams the file without buffering it in memory.
        let upload = Command::new("curl")
            .args([
                "-s",
                "-X", "PUT",
                &upload_uri,
                "-H", "Content-Type: video/x-matroska",
                "--data-binary", &format!("@{}", path.display()),
            ])
            .output()
            .context("failed to spawn curl for video upload")?;

        if !upload.status.success() {
            bail!(
                "YouTube video upload failed:\n{}",
                String::from_utf8_lossy(&upload.stderr)
            );
        }

        // Response body is a Videos resource JSON object.
        // We do a simple text scan for the "id" field rather than pulling in
        // a JSON parser dependency.
        let body = String::from_utf8_lossy(&upload.stdout);
        let video_id = body
            .lines()
            .find(|l| l.trim_start().starts_with("\"id\""))
            .and_then(|l| l.split('"').nth(3))
            .map(|s| s.to_string())
            .context("could not parse video ID from YouTube upload response")?;

        println!("Uploaded → https://www.youtube.com/watch?v={video_id}");
        Ok(video_id)
    }

    /// Download a YouTube video by ID using `yt-dlp`.
    ///
    /// `yt-dlp` is used for downloading rather than the YouTube API because:
    /// - The Data API v3 does not provide a download endpoint for uploaded videos.
    /// - `yt-dlp` selects the best available format and handles rate-limiting.
    /// - It supports resumable downloads out of the box.
    ///
    /// We request the best video-only stream (`-f bestvideo`) to avoid an
    /// audio track being muxed in alongside the data-carrying video stream.
    fn download(&self, video_id: &str) -> Result<PathBuf> {
        let url = format!("https://www.youtube.com/watch?v={video_id}");

        let status = Command::new("yt-dlp")
            .args([
                "--no-playlist",
                "-f", "bestvideo",   // best quality video-only stream
                "--no-part",         // write directly to final file, no .part temp file
                "-o", &self.download_path.to_string_lossy(),
                &url,
            ])
            .status()
            .context("failed to spawn yt-dlp (is it installed and on $PATH?)")?;

        if !status.success() {
            bail!("yt-dlp exited with non-zero status: {status}");
        }

        println!("Downloaded → {}", self.download_path.display());
        Ok(self.download_path.clone())
    }
}

impl PipelineHook for YoutubeHook {
    /// Called by [`roundtrip`] after encoding completes and before decoding begins.
    ///
    /// Uploads the encoded MKV to YouTube, then downloads it back. The decoder
    /// will read the downloaded copy, which has been processed by YouTube's
    /// ingest pipeline — exactly the real-world scenario yts3 is built for.
    fn after_encode(&self, encoded_path: &Path) -> Result<PathBuf> {
        println!("Uploading {} …", encoded_path.display());
        let video_id = self.upload(encoded_path)?;

        // YouTube needs a moment to finish processing before the video is
        // downloadable. In production you would poll the Videos.list endpoint
        // checking `processingDetails.processingStatus == "succeeded"`.
        // For brevity, a short sleep is shown here; replace with a real poll.
        println!("Waiting for YouTube to process the upload …");
        std::thread::sleep(std::time::Duration::from_secs(30));

        println!("Downloading video {video_id} …");
        self.download(&video_id)
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Basic argument parsing — in a real CLI you would use clap.
    let (input, encoded, output, password) = match args.len() {
        4 => (&args[1], &args[2], &args[3], None),
        5 => (&args[1], &args[2], &args[3], Some(args[4].as_str())),
        _ => {
            eprintln!(
                "Usage: {} <input> <encoded.mkv> <output> [password]",
                args[0]
            );
            std::process::exit(1);
        }
    };

    // Load OAuth2 credentials from the environment.
    let credentials = YoutubeCredentials::from_env()?;

    // The hook uploads to YouTube after encoding and downloads back before
    // decoding. The downloaded file is written to "downloaded.mkv".
    let hook = YoutubeHook::new(credentials, "downloaded.mkv");

    // Use the default 4K/30fps config. All parameters can be customised via
    // Yts3Config fields — see the crate docs for the full list.
    let cfg = Yts3Config::default();

    println!("Starting yts3 round-trip via YouTube …");

    // roundtrip orchestrates the full pipeline:
    //   encode_file → hook.after_encode → decode_file → hash comparison
    let result = roundtrip(
        Path::new(input),
        encoded,
        Path::new(output),
        password,
        &cfg,
        &hook,
    )?;

    if result.matched {
        println!("Round-trip OK  SHA-256: {}", result.original_hash);
    } else {
        eprintln!(
            "Hash mismatch after round-trip!\n  original : {}\n  decoded  : {}",
            result.original_hash, result.decoded_hash
        );
        std::process::exit(1);
    }

    Ok(())
}
