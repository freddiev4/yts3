use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use yts3::config::{
    DEFAULT_BITS_PER_BLOCK, DEFAULT_CHUNK_SIZE, DEFAULT_COEFFICIENT_STRENGTH,
    DEFAULT_FPS, DEFAULT_FRAME_HEIGHT, DEFAULT_FRAME_WIDTH, DEFAULT_REPAIR_OVERHEAD,
};
use yts3::pipeline;
use yts3::Yts3Config;

/// yts3 — YouTube as S3: encode arbitrary files into lossless video for cloud storage.
#[derive(Parser)]
#[command(name = "yts3", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Encode a file into a video
    Encode {
        /// Input file path
        #[arg(short, long)]
        input: PathBuf,

        /// Output video path (.mkv)
        #[arg(short, long)]
        output: String,

        /// Encrypt the file with a password
        #[arg(short, long)]
        password: Option<String>,

        /// Frame width (default: 3840)
        #[arg(long, default_value_t = DEFAULT_FRAME_WIDTH)]
        width: u32,

        /// Frame height (default: 2160)
        #[arg(long, default_value_t = DEFAULT_FRAME_HEIGHT)]
        height: u32,

        /// Frames per second (default: 30)
        #[arg(long, default_value_t = DEFAULT_FPS)]
        fps: u32,

        /// Bits embedded per 8x8 block (default: 1)
        #[arg(long, default_value_t = DEFAULT_BITS_PER_BLOCK)]
        bits_per_block: usize,

        /// DCT coefficient strength (default: 150.0)
        #[arg(long, default_value_t = DEFAULT_COEFFICIENT_STRENGTH)]
        coefficient_strength: f64,

        /// Chunk size in bytes (default: 1048576)
        #[arg(long, default_value_t = DEFAULT_CHUNK_SIZE)]
        chunk_size: usize,

        /// Fountain code repair overhead as a fraction (default: 1.0 = 100%)
        #[arg(long, default_value_t = DEFAULT_REPAIR_OVERHEAD)]
        repair_overhead: f64,
    },

    /// Decode a video back into the original file
    Decode {
        /// Input video path (.mkv)
        #[arg(short, long)]
        input: String,

        /// Output file path
        #[arg(short, long)]
        output: PathBuf,

        /// Decryption password (required if file was encrypted)
        #[arg(short, long)]
        password: Option<String>,

        /// Frame width (must match encoding)
        #[arg(long, default_value_t = DEFAULT_FRAME_WIDTH)]
        width: u32,

        /// Frame height (must match encoding)
        #[arg(long, default_value_t = DEFAULT_FRAME_HEIGHT)]
        height: u32,

        /// Bits per block (must match encoding)
        #[arg(long, default_value_t = DEFAULT_BITS_PER_BLOCK)]
        bits_per_block: usize,

        /// DCT coefficient strength (must match encoding)
        #[arg(long, default_value_t = DEFAULT_COEFFICIENT_STRENGTH)]
        coefficient_strength: f64,
    },
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Encode {
            input,
            output,
            password,
            width,
            height,
            fps,
            bits_per_block,
            coefficient_strength,
            chunk_size,
            repair_overhead,
        } => {
            let cfg = Yts3Config {
                frame_width: width,
                frame_height: height,
                fps,
                bits_per_block,
                coefficient_strength,
                chunk_size,
                repair_overhead,
                ..Default::default()
            };

            pipeline::encode::encode_file(
                &input,
                &output,
                password.as_deref(),
                &cfg,
            )?;
        }

        Commands::Decode {
            input,
            output,
            password,
            width,
            height,
            bits_per_block,
            coefficient_strength,
        } => {
            let cfg = Yts3Config {
                frame_width: width,
                frame_height: height,
                bits_per_block,
                coefficient_strength,
                ..Default::default()
            };

            pipeline::decode::decode_file(
                &input,
                &output,
                password.as_deref(),
                &cfg,
            )?;
        }
    }

    Ok(())
}
