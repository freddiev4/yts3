pub const MAGIC: u32 = 0x59545333; // "YTS3"
pub const PACKET_VERSION: u8 = 2;

// Video parameters
pub const DEFAULT_FRAME_WIDTH: u32 = 3840;
pub const DEFAULT_FRAME_HEIGHT: u32 = 2160;
pub const DEFAULT_FPS: u32 = 30;
pub const BLOCK_SIZE: usize = 8;
pub const DEFAULT_BITS_PER_BLOCK: usize = 1;
pub const DEFAULT_COEFFICIENT_STRENGTH: f64 = 150.0;

// Data parameters
pub const DEFAULT_CHUNK_SIZE: usize = 1_048_576; // 1 MiB
pub const SYMBOL_SIZE: usize = 256;
pub const DEFAULT_REPAIR_OVERHEAD: f64 = 1.0; // 100% redundancy

// Encryption overhead: 16-byte poly1305 tag
pub const AEAD_TAG_SIZE: usize = 16;
// 4-byte plaintext size header prepended to ciphertext
pub const ENCRYPTED_HEADER_SIZE: usize = 4;
pub const ENCRYPTION_OVERHEAD: usize = AEAD_TAG_SIZE + ENCRYPTED_HEADER_SIZE;

// File ID size
pub const FILE_ID_SIZE: usize = 16;

// Nonce size for XChaCha20-Poly1305
pub const NONCE_SIZE: usize = 24;

// Argon2id parameters
pub const ARGON2_MEM_COST: u32 = 65536; // 64 MiB
pub const ARGON2_TIME_COST: u32 = 3;
pub const ARGON2_PARALLELISM: u32 = 4;
pub const ARGON2_OUTPUT_LEN: usize = 32;

// Packet header sizes
pub const PACKET_HEADER_SIZE: usize = 50;

// Packet flag bits
pub const FLAG_REPAIR_SYMBOL: u8 = 0x01;
pub const FLAG_LAST_CHUNK: u8 = 0x02;
pub const FLAG_ENCRYPTED: u8 = 0x04;

/// DCT coefficient positions used for embedding data in 8x8 blocks.
pub const EMBED_POSITIONS: [(usize, usize); 4] = [(0, 1), (1, 0), (1, 1), (0, 2)];

/// Compute the number of 8x8 blocks in a frame.
pub fn blocks_per_frame(width: u32, height: u32) -> usize {
    (width as usize / BLOCK_SIZE) * (height as usize / BLOCK_SIZE)
}

/// Compute how many data bytes fit in a single frame.
pub fn bytes_per_frame(width: u32, height: u32, bits_per_block: usize) -> usize {
    blocks_per_frame(width, height) * bits_per_block / 8
}

/// Compute the maximum chunk size for encryption (accounting for AEAD overhead).
pub fn chunk_size_for_encryption(chunk_size: usize) -> usize {
    chunk_size - ENCRYPTION_OVERHEAD
}

/// Runtime configuration for an encode/decode operation.
#[derive(Debug, Clone)]
pub struct Yts3Config {
    pub frame_width: u32,
    pub frame_height: u32,
    pub fps: u32,
    pub bits_per_block: usize,
    pub coefficient_strength: f64,
    pub chunk_size: usize,
    pub symbol_size: usize,
    pub repair_overhead: f64,
}

impl Default for Yts3Config {
    fn default() -> Self {
        Self {
            frame_width: DEFAULT_FRAME_WIDTH,
            frame_height: DEFAULT_FRAME_HEIGHT,
            fps: DEFAULT_FPS,
            bits_per_block: DEFAULT_BITS_PER_BLOCK,
            coefficient_strength: DEFAULT_COEFFICIENT_STRENGTH,
            chunk_size: DEFAULT_CHUNK_SIZE,
            symbol_size: SYMBOL_SIZE,
            repair_overhead: DEFAULT_REPAIR_OVERHEAD,
        }
    }
}
