use argon2::Argon2;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use rand::RngCore;
use thiserror::Error;

use crate::config;

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("key derivation failed: {0}")]
    KeyDerivation(String),
    #[error("encryption failed: {0}")]
    Encryption(String),
    #[error("decryption failed: {0}")]
    Decryption(String),
}

/// Generate a cryptographically random 16-byte file ID.
pub fn generate_file_id() -> [u8; config::FILE_ID_SIZE] {
    let mut id = [0u8; config::FILE_ID_SIZE];
    rand::thread_rng().fill_bytes(&mut id);
    id
}

/// Derive a 32-byte encryption key from a password and file ID using Argon2id.
pub fn derive_key(
    password: &[u8],
    file_id: &[u8; config::FILE_ID_SIZE],
) -> Result<[u8; config::ARGON2_OUTPUT_LEN], CryptoError> {
    let params = argon2::Params::new(
        config::ARGON2_MEM_COST,
        config::ARGON2_TIME_COST,
        config::ARGON2_PARALLELISM,
        Some(config::ARGON2_OUTPUT_LEN),
    )
    .map_err(|e| CryptoError::KeyDerivation(e.to_string()))?;

    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut key = [0u8; config::ARGON2_OUTPUT_LEN];
    argon2
        .hash_password_into(password, file_id, &mut key)
        .map_err(|e| CryptoError::KeyDerivation(e.to_string()))?;

    Ok(key)
}

/// Build a deterministic 24-byte nonce from file_id (16 bytes) + chunk_index (4 bytes) + 4 zero bytes.
fn build_nonce(file_id: &[u8; config::FILE_ID_SIZE], chunk_index: u32) -> [u8; config::NONCE_SIZE] {
    let mut nonce = [0u8; config::NONCE_SIZE];
    nonce[..16].copy_from_slice(file_id);
    nonce[16..20].copy_from_slice(&chunk_index.to_le_bytes());
    nonce
}

/// Encrypt a chunk using XChaCha20-Poly1305.
/// Returns: [plaintext_size_le(4 bytes)] || [ciphertext + tag]
pub fn encrypt_chunk(
    key: &[u8; config::ARGON2_OUTPUT_LEN],
    file_id: &[u8; config::FILE_ID_SIZE],
    chunk_index: u32,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let key = chacha20poly1305::Key::from_slice(key);
    let cipher = XChaCha20Poly1305::new(key);
    let nonce_bytes = build_nonce(file_id, chunk_index);
    let nonce = XNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| CryptoError::Encryption(e.to_string()))?;

    let plaintext_len = plaintext.len() as u32;
    let mut result = Vec::with_capacity(4 + ciphertext.len());
    result.extend_from_slice(&plaintext_len.to_le_bytes());
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Decrypt a chunk. Input format: [plaintext_size_le(4 bytes)] || [ciphertext + tag]
pub fn decrypt_chunk(
    key: &[u8; config::ARGON2_OUTPUT_LEN],
    file_id: &[u8; config::FILE_ID_SIZE],
    chunk_index: u32,
    encrypted: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if encrypted.len() < config::ENCRYPTED_HEADER_SIZE {
        return Err(CryptoError::Decryption("data too short".into()));
    }

    let _plaintext_len = u32::from_le_bytes(
        encrypted[..4]
            .try_into()
            .map_err(|_| CryptoError::Decryption("invalid header".into()))?,
    );
    let ciphertext = &encrypted[4..];

    let key = chacha20poly1305::Key::from_slice(key);
    let cipher = XChaCha20Poly1305::new(key);
    let nonce_bytes = build_nonce(file_id, chunk_index);
    let nonce = XNonce::from_slice(&nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| CryptoError::Decryption(e.to_string()))
}

/// Securely zero a key buffer.
pub fn secure_zero(buf: &mut [u8]) {
    for byte in buf.iter_mut() {
        unsafe {
            std::ptr::write_volatile(byte, 0);
        }
    }
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_file_id_is_random() {
        let id1 = generate_file_id();
        let id2 = generate_file_id();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_key_derivation() {
        let file_id = generate_file_id();
        let key1 = derive_key(b"password123", &file_id).unwrap();
        let key2 = derive_key(b"password123", &file_id).unwrap();
        assert_eq!(key1, key2);

        let key3 = derive_key(b"different", &file_id).unwrap();
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let file_id = generate_file_id();
        let key = derive_key(b"test_password", &file_id).unwrap();
        let plaintext = b"Hello, YouTube S3!";

        let encrypted = encrypt_chunk(&key, &file_id, 0, plaintext).unwrap();
        assert_ne!(&encrypted[4..], plaintext.as_slice());

        let decrypted = decrypt_chunk(&key, &file_id, 0, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_wrong_key_fails() {
        let file_id = generate_file_id();
        let key1 = derive_key(b"correct", &file_id).unwrap();
        let key2 = derive_key(b"wrong", &file_id).unwrap();

        let encrypted = encrypt_chunk(&key1, &file_id, 0, b"secret data").unwrap();
        let result = decrypt_chunk(&key2, &file_id, 0, &encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_different_chunk_indices() {
        let file_id = generate_file_id();
        let key = derive_key(b"password", &file_id).unwrap();
        let plaintext = b"same data";

        let enc1 = encrypt_chunk(&key, &file_id, 0, plaintext).unwrap();
        let enc2 = encrypt_chunk(&key, &file_id, 1, plaintext).unwrap();
        assert_ne!(enc1, enc2);
    }

    #[test]
    fn test_secure_zero() {
        let mut buf = [0xFFu8; 32];
        secure_zero(&mut buf);
        assert_eq!(buf, [0u8; 32]);
    }
}
